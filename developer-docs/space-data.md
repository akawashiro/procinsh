# SPACE data processing

`web/src/space/data.ts` exposes `SpaceDataStore` and re-exports `layoutMaps`.
The store owns SSE connections, host snapshots, activity retention, and display
state. It schedules updates and keeps the same stateful path for live events and
recording replay.

- `snapshot-merge.ts` exposes `mergeSnapshot(previous, update)`. It applies full
  and delta updates, checks sequence baselines and process-map identity, and
  preserves the existing omitted-field behavior.
- `resource-layout.ts` exposes `networkGroups`, `networkLayout`, and `fileLayout`.
  These functions group destinations and place markers without owning the store.
  Prior positions, ordering, and host-layer heights are preserved. File and
  Netlink placement mirrors the marker lattice below the owning process.

Transformation tests live beside the extracted functions. Store regression,
activity retention, and file-history/placement integration tests remain with
`SpaceDataStore`. Docker `ordinary` runs these and the host/RDMA tests; `browser`
checks live display and static recording replay.
