// Serve a fixed topology and scheduled SSE updates while proxying release assets.
import { createServer } from "node:http";
import {
  processInfo,
  processSummary,
  memoryMap,
  fdEndpoint,
  fdRelation,
  spaceActivity,
  cpuActivity,
} from "../tests/support/fixtures.js";

export async function profileFixture(backend: string, busy: boolean) {
  const processes = Array.from({ length: 256 }, (_, i) =>
    processInfo({
      identity: { pid: i + 1, start_time_ticks: 100 },
      parent_id: i
        ? { pid: Math.floor((i - 1) / 4) + 1, start_time_ticks: 100 }
        : null,
      name: `profile-process-${i + 1}`,
      maps: Array.from({ length: 16 }, (_, j) =>
        memoryMap({
          start: `0x${((j + 1) * 65536).toString(16)}`,
          end: `0x${((j + 1) * 65536 + 16384).toString(16)}`,
          pathname: `/profile/library-${j}.so`,
        }),
      ),
    }),
  );
  const relations = processes.slice(1).map((p, i) => {
    const resource = {
      kind: "pipe" as const,
      device: { major: 0, minor: 1 },
      inode: String(i + 1000),
    };
    return fdRelation({
      id: `profile-pipe-${i}`,
      endpoint: fdEndpoint({ process_id: processes[i].identity, resource }),
      peer: fdEndpoint({ process_id: p.identity, resource }),
    });
  });
  const server = createServer(async (request, response) => {
    try {
      if (request.url === "/api/system/events") {
        response.writeHead(200, {
          "Content-Type": "text/event-stream",
          "Cache-Control": "no-cache",
        });
        let sequence = 0;
        const snapshot = () => {
          sequence++;
          const changed = processes.map((p, i) => ({
            ...p,
            maps_epoch: busy ? sequence : 0,
            maps: p.maps.map((m, j) => ({
              ...m,
              writable: busy && (sequence + i + j) % 2 === 0,
            })),
          }));
          response.write(
            `event: snapshot\ndata: ${JSON.stringify({ kind: sequence === 1 ? "full" : "delta", sequence, base_sequence: sequence - 1, processes: changed, fd_relations: relations })}\n\n`,
          );
        };
        snapshot();
        const snapshots = setInterval(snapshot, 1000);
        let tick = 0;
        const activity = setInterval(() => {
          tick++;
          const updates = busy
            ? {
                cpu: processes.slice(0, 64).map((p) =>
                  cpuActivity({
                    process_id: p.identity,
                    runtime_ns: 20_000_000,
                    running_threads: 1,
                  }),
                ),
                ipc: relations.slice(0, 64).map((r) => ({
                  process_id: r.endpoint.process_id,
                  resource: r.endpoint.resource,
                  write: true,
                  count: 8,
                  bytes: 4096,
                })),
                signals:
                  tick % 10 === 0
                    ? [
                        {
                          timestamp_ns: tick * 100_000_000,
                          src_pid: 1,
                          dst_pid: 2,
                          signal: 10,
                          source_id: processes[0].identity,
                          destination_id: processes[1].identity,
                        },
                      ]
                    : [],
              }
            : {};
          response.write(
            `event: activity\ndata: ${JSON.stringify(spaceActivity(updates))}\n\n`,
          );
        }, 100);
        response.on("close", () => {
          clearInterval(snapshots);
          clearInterval(activity);
        });
      } else if (request.url === "/api/processes") {
        response.setHeader("Content-Type", "application/json");
        response.end(
          JSON.stringify(
            processes.map((p) =>
              processSummary({ identity: p.identity, name: p.name }),
            ),
          ),
        );
      } else {
        const upstream = await fetch(new URL(request.url ?? "/", backend));
        response.writeHead(upstream.status, {
          "Content-Type":
            upstream.headers.get("content-type") ?? "application/octet-stream",
        });
        response.end(Buffer.from(await upstream.arrayBuffer()));
      }
    } catch (error) {
      response.writeHead(502);
      response.end(String(error));
    }
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  if (!address || typeof address === "string")
    throw new Error("Missing fixture address");
  return {
    url: `http://127.0.0.1:${address.port}`,
    counts: {
      processes: processes.length,
      maps: processes.length * 16,
      relations: relations.length,
    },
    async close() {
      server.closeAllConnections();
      await new Promise<void>((resolve, reject) =>
        server.close((error) => (error ? reject(error) : resolve())),
      );
    },
  };
}
