# SPACE recording and replay

On the live `/space` page, select **Record**, continue exploring the machine, and
select **Stop & Download**. The browser downloads exactly one UTF-8 `.json`
file. Recording runs in memory, requires no upload or server storage, and starts
with the current complete snapshot, including maps retained from earlier deltas.

Recordings can contain process arguments, user names, paths and IP addresses.
Review them before sharing. The bundled demo uses synthetic process, pipe, CPU,
file and signal observations; it contains no captured machine data.

A stream gap, reconnect, hidden tab or page exit ends the recording. The retained
recording stays available as **Download recording** when the page is visible
again. The UI reports the reason; it does not resume across missing observations.
Leaving or reloading with an undownloaded recording prompts the browser's
unsaved-work confirmation. Download before closing the tab: memory is not durable
storage. At 128 MiB the recorder stops and retains the completed prefix; local
replay uses the same size limit. The live view continues independently.

**Open recording** opens `/replay/space/`. Select or drop the original downloaded
JSON. Files are read with the browser File API and never uploaded. Choose another
file on the same page. Play/pause, timeline seeking, event stepping in either
direction, and 0.1×/0.25×/0.5×/1×/2× speeds use a virtual clock shared with SPACE
activity animation. Orbit, pan and zoom remain available while paused. Hiding the
replay tab pauses playback. Enable **Loop** to repeat from the beginning at the
selected speed; disabling it stops playback at the end. Process inspection links
are disabled in replay.

## JSON contract

Version 1 is one JSON object containing:

- `format_version: 1`, `view_type: "space"` and ISO `started_at`.
- `duration_ms` and `end_reason`.
- `initial_snapshot`: complete `SystemSnapshot` (processes with complete maps and
  FD relations); no earlier SSE messages are required.
- `events`: ordered `{ at_ms, type, data }` objects. Time is milliseconds relative
  to recording start; equal timestamps retain array order. `type` is `snapshot`
  (a complete merged `SystemSnapshot`) or `activity` (`SpaceActivity`).

Payload definitions are in `web/src/shared/api-types.ts`; the shared runtime
parser is in `web/src/space/recording.ts`. Unknown versions, other view types,
invalid nested values and out-of-order/out-of-duration timestamps are rejected
before rendering. Local files and public recordings use this parser and the
same replay engine. Playback, seeking and looping end at the last event timestamp
(or zero for snapshot-only recordings). The timeline accepts fractional
milliseconds so its right edge reaches that exact timestamp. `duration_ms` retains
the elapsed recording time, which can include an interval between the last event
and stopping the recording. Seeking reconstructs state from the initial snapshot; this
bounds format complexity but can take longer for large recordings.

## Static site and Cloudflare Pages Free

The replay site builds with Node.js >=22.12 and npm alone:

```sh
npm --prefix web ci
npm --prefix web run build:replay
```

Publish **`web/dist-replay/`** directly. The build includes
`replay/space/index.html`, shared SPACE assets and `recordings/demo.json`.
It contains no LIST/PROCESS/LIVE page. Direct navigation and refresh work through
the directory's `index.html`; `/replay/` is reserved for a future selector.
The viewer never opens SSE or calls live APIs.

To configure a Cloudflare Pages Git integration, choose the Free plan, connect
this repository, leave the root directory at the repository root, select no
framework preset, set the build command to
`npm --prefix web ci && npm --prefix web run build:replay`, and set the output
directory to `web/dist-replay`. Use Node.js 22.12 or newer. The root
`wrangler.jsonc` also declares the Pages output directory for direct upload.
No Worker, Pages Function, Rust toolchain, collector, privileged access, backend,
or paid service is involved. The documentation publishing workflow is independent.
Creating the Cloudflare account/project and its initial deployment requires the
owner's Cloudflare account; a local build does not provision those resources.

Open `/replay/space/?recording=demo` on the deployed site. `recording` is an
allowlisted ID, never a user-supplied fetch URL. A single synthetic sanitized JSON
is shared by the normal embedded build and the static build. The build rejects
any static asset larger than 25 MiB, including public demos, according to
[Cloudflare Pages Free limits](https://developers.cloudflare.com/pages/platform/limits/).
No chunk conversion or companion files are needed.

For a deployment under a path prefix, build with
`PROCINSH_REPLAY_BASE=/prefix/ npm --prefix web run build:replay` and serve the
output under that prefix. Asset URLs and the demo fetch respect that base.

The normal `npm --prefix web run build` still outputs `web/dist/` for embedding
in Rust, with live `/space` and `/replay/space/` as separate page routes.

## Validation

Run `bash scripts/docker_test.sh ordinary`, `checks` and `browser`. Ordinary tests
include the standalone build, schema validation, midstream recording with merged
maps, relative timestamps, event ordering, playback/seek/step and asset size
boundaries. Browser tests cover recording download, unchanged local JSON picker
and drop loading, and a replay-only static server under `/demo/`, including
refresh, slow playback, unknown recording IDs and zero live API requests.
