// Apply full/delta snapshots and validate the sequence and process-map baselines.
import { key } from "./model.js";
import type {
  SystemSnapshot,
  SystemSnapshotUpdate,
} from "../shared/api-types.js";

/** Omitted maps retain the previous mapping for this exact process identity. */
export function mergeSnapshot(
  previous: SystemSnapshot,
  update: SystemSnapshotUpdate,
): SystemSnapshot {
  if (update.kind !== "full" && update.kind !== "delta")
    throw new Error("Unknown snapshot kind");
  if (
    update.kind === "delta" &&
    (previous.sequence === undefined ||
      update.base_sequence !== previous.sequence)
  )
    throw new Error("Snapshot baseline mismatch");
  const known = new Map(previous.processes.map((p) => [key(p.identity), p]));
  const relations = new Map(
    (update.kind === "full" ? [] : previous.fd_relations).map((r) => [r.id, r]),
  );
  if (update.fd_relations_delta) {
    for (const id of update.fd_relations_delta.remove) relations.delete(id);
    for (const r of update.fd_relations_delta.upsert) relations.set(r.id, r);
  }
  const handles = new Map(
    (update.kind === "full" ? [] : (previous.device_handles ?? [])).map((h) => [
      h.id,
      h,
    ]),
  );
  if (update.device_handles_delta) {
    for (const id of update.device_handles_delta.remove) handles.delete(id);
    for (const h of update.device_handles_delta.upsert) handles.set(h.id, h);
  }
  const rdma = new Map(
    (update.kind === "full" ? [] : (previous.rdma_relations ?? [])).map((r) => [
      r.id,
      r,
    ]),
  );
  if (update.rdma_relations_delta) {
    for (const id of update.rdma_relations_delta.remove) rdma.delete(id);
    for (const r of update.rdma_relations_delta.upsert) rdma.set(r.id, r);
  }
  const {
    device_handles_delta: _handles,
    rdma_relations_delta: _rdma,
    ...rest
  } = update;
  return {
    ...rest,
    fd_relations: update.fd_relations ?? [...relations.values()],
    rdma_relations: update.rdma_relations ?? [...rdma.values()],
    rdma_ports: update.rdma_ports ?? previous.rdma_ports ?? [],
    hardware_devices:
      update.hardware_devices ?? previous.hardware_devices ?? [],
    hardware_paths: update.hardware_paths ?? previous.hardware_paths ?? [],
    device_handles: update.device_handles ?? [...handles.values()],
    processes: update.processes.map((p) => {
      const old =
        update.kind === "full" ? undefined : known.get(key(p.identity));
      let maps = p.maps ?? old?.maps ?? [];
      if (p.maps_delta) {
        if (!old) throw new Error("Missing process maps baseline");
        const entries = new Map(old.maps.map((m) => [m.start, m]));
        for (const address of p.maps_delta.remove) entries.delete(address);
        for (const m of p.maps_delta.upsert) entries.set(m.start, m);
        maps = [...entries.values()].sort((a, b) => {
          const startA = BigInt(a.start),
            startB = BigInt(b.start);
          return startA < startB ? -1 : startA > startB ? 1 : 0;
        });
      }
      const { maps_delta: _delta, ...process } = p;
      return {
        ...process,
        maps,
        maps_epoch:
          p.maps === undefined && p.maps_delta === undefined
            ? (old?.maps_epoch ?? 0)
            : p.maps_epoch,
      };
    }),
  };
}

if (import.meta.vitest) {
  const { test } = import.meta.vitest;
  test("snapshot merging", async () => {
    const assert: typeof import("node:assert/strict") = (
      await import("node:assert/strict")
    ).default;
    const { processInfo, memoryMap, fdRelation } =
      await import("../../tests/support/fixtures.js");
    const original = processInfo({
      identity: { pid: 1, start_time_ticks: 10 },
      maps: [memoryMap({ start: "0x1000", end: "0x2000" })],
      maps_epoch: 10,
      maps_error: null,
    });
    const base = { processes: [original], fd_relations: [], sequence: 1 };
    const delta = (
      extra: Pick<SystemSnapshotUpdate, "processes">,
    ): SystemSnapshotUpdate => ({
      kind: "delta",
      sequence: 2,
      base_sequence: 1,
      fd_relations: [],
      ...extra,
    });
    const { maps: _maps, ...originalWithoutMaps } = original;
    const omitted = {
      ...originalWithoutMaps,
      identity: original.identity,
      maps_epoch: 20,
      maps_error: "read failed",
    };
    const retained = mergeSnapshot(base, delta({ processes: [omitted] }));
    assert.deepEqual(retained.processes[0].maps, original.maps);
    assert.equal(retained.processes[0].maps_epoch, 10);
    assert.equal(retained.processes[0].maps_error, "read failed");
    assert.deepEqual(
      mergeSnapshot(base, delta({ processes: [{ ...omitted, maps: [] }] }))
        .processes[0].maps,
      [],
    );
    assert.deepEqual(
      mergeSnapshot(base, delta({ processes: [] })).processes,
      [],
    );
    assert.deepEqual(
      mergeSnapshot(
        base,
        delta({
          processes: [
            { ...omitted, identity: { pid: 1, start_time_ticks: 11 } },
          ],
        }),
      ).processes[0].maps,
      [],
    );
    assert.equal(
      mergeSnapshot(retained, { ...base, kind: "full" }).processes[0]
        .maps_epoch,
      10,
    );

    const changedMap = memoryMap({
      start: "0x1000",
      end: "0x1800",
      writable: true,
    });
    const added = memoryMap({ start: "0x1800", end: "0x2000" });
    const split = mergeSnapshot(
      base,
      delta({
        processes: [
          {
            ...omitted,
            maps_delta: { upsert: [added, changedMap], remove: [] },
          },
        ],
      }),
    );
    assert.deepEqual(split.processes[0].maps, [changedMap, added]);
    assert.equal(split.processes[0].maps_epoch, 20);
    const joined = mergeSnapshot(split, {
      kind: "delta",
      sequence: 3,
      base_sequence: 2,
      processes: [
        {
          ...omitted,
          maps_delta: { upsert: [original.maps[0]], remove: ["0x1800"] },
        },
      ],
      fd_relations_delta: {
        upsert: [fdRelation({ id: "new", label: "pipe" })],
        remove: [],
      },
    });
    assert.deepEqual(joined.processes[0].maps, original.maps);
    assert.deepEqual(joined.fd_relations, [
      fdRelation({ id: "new", label: "pipe" }),
    ]);
    const removed = mergeSnapshot(joined, {
      kind: "delta",
      sequence: 4,
      base_sequence: 3,
      processes: [],
      fd_relations_delta: { upsert: [], remove: ["new"] },
    });
    assert.deepEqual(removed.fd_relations, []);
    assert.throws(
      () =>
        mergeSnapshot(base, {
          kind: "delta",
          sequence: 9,
          base_sequence: 8,
          processes: [],
        }),
      /baseline mismatch/,
    );
    assert.throws(
      () =>
        mergeSnapshot(
          base,
          delta({
            processes: [
              {
                ...omitted,
                identity: { pid: 1, start_time_ticks: 99 },
                maps_delta: { upsert: [], remove: [] },
              },
            ],
          }),
        ),
      /Missing process maps baseline/,
    );
    assert.deepEqual(
      mergeSnapshot(joined, {
        kind: "full",
        sequence: 10,
        processes: [],
        fd_relations: [],
      }).fd_relations,
      [],
    );
  });
}
