# Function time rankings

These rankings aggregate the baseline V8 CPU profiles by original function definition. Times are sampled estimates over each approximately 15-second capture. **Self** excludes callees; **total** includes callees. Percentages use non-idle sampled time, including native/program/GC samples, as the denominator. Parent and child total times overlap and must not be added. Recursive appearances of one function within a sampled stack count once for its total.

The inclusive ranking lists JavaScript/library frames; the self ranking also includes V8/native buckets. Native frames have no definition line and use zero in the CSV.

All rows, including functions below the cutoffs here, are available in [FUNCTION_TIMES.csv](FUNCTION_TIMES.csv). Definitions and collection conditions are in [REPORT.md](REPORT.md).

## idle-space

Sample window: 15.040 s; non-idle: 1205.6 ms; idle: 13834.8 ms.

### Largest inclusive times (JavaScript/library frames)

| Function | Definition | Self (ms) | Total (ms) | Self / active | Total / active |
| --- | --- | ---: | ---: | ---: | ---: |
| `animate` | [web/src/space/renderer.ts:473](../../../web/src/space/renderer.ts#L473) | 14.8 | 609.1 | 1.2% | 50.5% |
| `drawLabels` | [web/src/space/renderer.ts:220](../../../web/src/space/renderer.ts#L220) | 69.2 | 346.9 | 5.7% | 28.8% |
| `(anonymous)` | [web/src/space/data.ts:627](../../../web/src/space/data.ts#L627) | 34.9 | 221.1 | 2.9% | 18.3% |
| `render` | `three/build/three.module.js:16363` | 3.2 | 216.9 | 0.3% | 18.0% |
| `renderScene` | `three/build/three.module.js:16663` | 1.1 | 166.5 | 0.1% | 13.8% |
| `renderObjects` | `three/build/three.module.js:16814` | 1.1 | 165.4 | 0.1% | 13.7% |
| `renderObject` | `three/build/three.module.js:16843` | 1.1 | 164.3 | 0.1% | 13.6% |
| `renderBufferDirect` | `three/build/three.module.js:15935` | 6.3 | 163.3 | 0.5% | 13.5% |
| `updateSnapshot` | [web/src/space/app.ts:191](../../../web/src/space/app.ts#L191) | 0.0 | 142.8 | 0.0% | 11.8% |
| `rebuildScene` | [web/src/space/app.ts:152](../../../web/src/space/app.ts#L152) | 23.3 | 142.8 | 1.9% | 11.8% |
| `setProgram` | `three/build/three.module.js:17021` | 9.5 | 138.9 | 0.8% | 11.5% |
| `add` | [web/src/space/renderer.ts:248](../../../web/src/space/renderer.ts#L248) | 123.0 | 127.2 | 10.2% | 10.6% |
| `buildScene` | [web/src/space/scene.ts:226](../../../web/src/space/scene.ts#L226) | 24.1 | 117.4 | 2.0% | 9.7% |
| `onFirstUse` | `three/build/three.module.js:6491` | 2.1 | 72.3 | 0.2% | 6.0% |
| `addLine` | [web/src/space/scene.ts:345](../../../web/src/space/scene.ts#L345) | 25.4 | 44.4 | 2.1% | 3.7% |

### Largest self times

| Function | Definition | Self (ms) | Total (ms) | Self / active | Total / active |
| --- | --- | ---: | ---: | ---: | ---: |
| `(program)` | (V8/native) | 324.6 | 324.6 | 26.9% | 26.9% |
| `add` | [web/src/space/renderer.ts:248](../../../web/src/space/renderer.ts#L248) | 123.0 | 127.2 | 10.2% | 10.6% |
| `measureText` | (V8/native) | 79.3 | 79.3 | 6.6% | 6.6% |
| `drawLabels` | [web/src/space/renderer.ts:220](../../../web/src/space/renderer.ts#L220) | 69.2 | 346.9 | 5.7% | 28.8% |
| `getProgramInfoLog` | (V8/native) | 52.1 | 52.1 | 4.3% | 4.3% |
| `(garbage collector)` | (V8/native) | 35.1 | 35.1 | 2.9% | 2.9% |
| `(anonymous)` | [web/src/space/data.ts:627](../../../web/src/space/data.ts#L627) | 34.9 | 221.1 | 2.9% | 18.3% |
| `addLine` | [web/src/space/scene.ts:345](../../../web/src/space/scene.ts#L345) | 25.4 | 44.4 | 2.1% | 3.7% |
| `buildScene` | [web/src/space/scene.ts:226](../../../web/src/space/scene.ts#L226) | 24.1 | 117.4 | 2.0% | 9.7% |
| `rebuildScene` | [web/src/space/app.ts:152](../../../web/src/space/app.ts#L152) | 23.3 | 142.8 | 1.9% | 11.8% |
| `fillText` | (V8/native) | 21.2 | 21.2 | 1.8% | 1.8% |
| `strokeText` | (V8/native) | 18.2 | 18.2 | 1.5% | 1.5% |
| `animate` | [web/src/space/renderer.ts:473](../../../web/src/space/renderer.ts#L473) | 14.8 | 609.1 | 1.2% | 50.5% |
| `getShaderInfoLog` | (V8/native) | 13.8 | 13.8 | 1.1% | 1.1% |
| `(anonymous)` | [web/src/space/renderer.ts:321](../../../web/src/space/renderer.ts#L321) | 13.8 | 13.8 | 1.1% | 1.1% |

## interact-space

Sample window: 15.047 s; non-idle: 1672.1 ms; idle: 13374.9 ms.

### Largest inclusive times (JavaScript/library frames)

| Function | Definition | Self (ms) | Total (ms) | Self / active | Total / active |
| --- | --- | ---: | ---: | ---: | ---: |
| `animate` | [web/src/space/renderer.ts:473](../../../web/src/space/renderer.ts#L473) | 17.2 | 595.9 | 1.0% | 35.6% |
| `pointermove` | [web/src/space/selection.ts:174](../../../web/src/space/selection.ts#L174) | 6.3 | 438.8 | 0.4% | 26.2% |
| `intersect` | `three/build/three.core.js:54892` | 14.8 | 431.4 | 0.9% | 25.8% |
| `edgeHit` | [web/src/space/selection.ts:104](../../../web/src/space/selection.ts#L104) | 1.1 | 420.5 | 0.1% | 25.1% |
| `intersectObjects` | `three/build/three.core.js:54870` | 1.1 | 419.5 | 0.1% | 25.1% |
| `drawLabels` | [web/src/space/renderer.ts:220](../../../web/src/space/renderer.ts#L220) | 67.8 | 366.7 | 4.1% | 21.9% |
| `raycast` | `three/build/three.core.js:25835` | 51.0 | 244.7 | 3.1% | 14.6% |
| `render` | `three/build/three.module.js:16363` | 2.1 | 196.7 | 0.1% | 11.8% |
| `(anonymous)` | [web/src/space/data.ts:627](../../../web/src/space/data.ts#L627) | 35.9 | 194.5 | 2.1% | 11.6% |
| `raycast` | `three/build/three.core.js:28781` | 2.4 | 172.0 | 0.1% | 10.3% |
| `checkIntersection` | `three/build/three.core.js:28917` | 16.3 | 161.1 | 1.0% | 9.6% |
| `renderScene` | `three/build/three.module.js:16663` | 1.1 | 155.7 | 0.1% | 9.3% |
| `renderObjects` | `three/build/three.module.js:16814` | 1.1 | 154.6 | 0.1% | 9.2% |
| `renderObject` | `three/build/three.module.js:16843` | 0.0 | 153.6 | 0.0% | 9.2% |
| `renderBufferDirect` | `three/build/three.module.js:15935` | 3.2 | 153.6 | 0.2% | 9.2% |

### Largest self times

| Function | Definition | Self (ms) | Total (ms) | Self / active | Total / active |
| --- | --- | ---: | ---: | ---: | ---: |
| `(program)` | (V8/native) | 355.5 | 355.5 | 21.3% | 21.3% |
| `add` | [web/src/space/renderer.ts:248](../../../web/src/space/renderer.ts#L248) | 143.5 | 146.7 | 8.6% | 8.8% |
| `measureText` | (V8/native) | 102.4 | 102.4 | 6.1% | 6.1% |
| `drawLabels` | [web/src/space/renderer.ts:220](../../../web/src/space/renderer.ts#L220) | 67.8 | 366.7 | 4.1% | 21.9% |
| `multiplyMatrices` | `three/build/three.core.js:11843` | 54.3 | 54.3 | 3.2% | 3.2% |
| `raycast` | `three/build/three.core.js:25835` | 51.0 | 244.7 | 3.1% | 14.6% |
| `(garbage collector)` | (V8/native) | 48.8 | 48.8 | 2.9% | 2.9% |
| `getProgramInfoLog` | (V8/native) | 44.5 | 44.5 | 2.7% | 2.7% |
| `applyMatrix4` | `three/build/three.core.js:4777` | 37.1 | 37.1 | 2.2% | 2.2% |
| `distanceSqToSegment` | `three/build/three.core.js:10834` | 37.0 | 133.1 | 2.2% | 8.0% |
| `(anonymous)` | [web/src/space/data.ts:627](../../../web/src/space/data.ts#L627) | 35.9 | 194.5 | 2.1% | 11.6% |
| `addLine` | [web/src/space/scene.ts:345](../../../web/src/space/scene.ts#L345) | 34.9 | 45.4 | 2.1% | 2.7% |
| `copy` | `three/build/three.core.js:4539` | 28.6 | 28.6 | 1.7% | 1.7% |
| `dot` | `three/build/three.core.js:5072` | 26.6 | 26.6 | 1.6% | 1.6% |
| `fromArray` | `three/build/three.core.js:12544` | 26.2 | 26.2 | 1.6% | 1.6% |

## busy-space

Sample window: 15.059 s; non-idle: 2453.3 ms; idle: 12605.4 ms.

### Largest inclusive times (JavaScript/library frames)

| Function | Definition | Self (ms) | Total (ms) | Self / active | Total / active |
| --- | --- | ---: | ---: | ---: | ---: |
| `animate` | [web/src/space/renderer.ts:473](../../../web/src/space/renderer.ts#L473) | 454.3 | 1535.3 | 18.5% | 62.6% |
| `Color.constructor` | `three/build/three.core.js:15402` | 100.3 | 478.4 | 4.1% | 19.5% |
| `setHex` | `three/build/three.core.js:15505` | 367.5 | 367.5 | 15.0% | 15.0% |
| `drawLabels` | [web/src/space/renderer.ts:220](../../../web/src/space/renderer.ts#L220) | 50.8 | 312.5 | 2.1% | 12.7% |
| `(anonymous)` | [web/src/space/data.ts:640](../../../web/src/space/data.ts#L640) | 13.8 | 240.1 | 0.6% | 9.8% |
| `(anonymous)` | [web/src/space/data.ts:627](../../../web/src/space/data.ts#L627) | 33.8 | 233.9 | 1.4% | 9.5% |
| `ingestActivity` | [web/src/space/data.ts:499](../../../web/src/space/data.ts#L499) | 51.8 | 213.6 | 2.1% | 8.7% |
| `render` | `three/build/three.module.js:16363` | 1.1 | 197.2 | 0.0% | 8.0% |
| `renderScene` | `three/build/three.module.js:16663` | 0.0 | 159.0 | 0.0% | 6.5% |
| `renderObjects` | `three/build/three.module.js:16814` | 0.0 | 159.0 | 0.0% | 6.5% |
| `renderObject` | `three/build/three.module.js:16843` | 5.3 | 159.0 | 0.2% | 6.5% |
| `renderBufferDirect` | `three/build/three.module.js:15935` | 2.1 | 152.7 | 0.1% | 6.2% |
| `updateSnapshot` | [web/src/space/app.ts:191](../../../web/src/space/app.ts#L191) | 0.0 | 152.5 | 0.0% | 6.2% |
| `rebuildScene` | [web/src/space/app.ts:152](../../../web/src/space/app.ts#L152) | 18.0 | 138.8 | 0.7% | 5.7% |
| `setProgram` | `three/build/three.module.js:17021` | 5.3 | 127.0 | 0.2% | 5.2% |

### Largest self times

| Function | Definition | Self (ms) | Total (ms) | Self / active | Total / active |
| --- | --- | ---: | ---: | ---: | ---: |
| `animate` | [web/src/space/renderer.ts:473](../../../web/src/space/renderer.ts#L473) | 454.3 | 1535.3 | 18.5% | 62.6% |
| `(program)` | (V8/native) | 374.0 | 374.0 | 15.2% | 15.2% |
| `setHex` | `three/build/three.core.js:15505` | 367.5 | 367.5 | 15.0% | 15.0% |
| `add` | [web/src/space/renderer.ts:248](../../../web/src/space/renderer.ts#L248) | 110.2 | 116.5 | 4.5% | 4.8% |
| `Color.constructor` | `three/build/three.core.js:15402` | 100.3 | 478.4 | 4.1% | 19.5% |
| `measureText` | (V8/native) | 80.5 | 80.5 | 3.3% | 3.3% |
| `(garbage collector)` | (V8/native) | 70.0 | 70.0 | 2.9% | 2.9% |
| `getProgramInfoLog` | (V8/native) | 66.6 | 66.6 | 2.7% | 2.7% |
| `activeResources` | [web/src/space/renderer.ts:424](../../../web/src/space/renderer.ts#L424) | 58.1 | 58.1 | 2.4% | 2.4% |
| `key` | [web/src/space/model.ts:18](../../../web/src/space/model.ts#L18) | 57.1 | 57.1 | 2.3% | 2.3% |
| `ingestActivity` | [web/src/space/data.ts:499](../../../web/src/space/data.ts#L499) | 51.8 | 213.6 | 2.1% | 8.7% |
| `drawLabels` | [web/src/space/renderer.ts:220](../../../web/src/space/renderer.ts#L220) | 50.8 | 312.5 | 2.1% | 12.7% |
| `(anonymous)` | [web/src/space/data.ts:627](../../../web/src/space/data.ts#L627) | 33.8 | 233.9 | 1.4% | 9.5% |
| `buildScene` | [web/src/space/scene.ts:226](../../../web/src/space/scene.ts#L226) | 28.6 | 119.7 | 1.2% | 4.9% |
| `edgeDirection` | [web/src/space/data.ts:109](../../../web/src/space/data.ts#L109) | 23.3 | 81.5 | 0.9% | 3.3% |

## idle-list

Sample window: 15.012 s; non-idle: 558.9 ms; idle: 14452.8 ms.

### Largest inclusive times (JavaScript/library frames)

| Function | Definition | Self (ms) | Total (ms) | Self / active | Total / active |
| --- | --- | ---: | ---: | ---: | ---: |
| `refresh` | [web/src/list/data.ts:23](../../../web/src/list/data.ts#L23) | 0.0 | 118.1 | 0.0% | 21.1% |
| `render` | [web/src/list/app.ts:13](../../../web/src/list/app.ts#L13) | 0.0 | 114.9 | 0.0% | 20.6% |
| `renderProcesses` | [web/src/list/renderer.ts:8](../../../web/src/list/renderer.ts#L8) | 3.2 | 113.8 | 0.6% | 20.4% |
| `bytes` | [web/src/shared/display.ts:129](../../../web/src/shared/display.ts#L129) | 54.9 | 56.0 | 9.8% | 10.0% |
| `node` | [web/src/shared/dom.ts:4](../../../web/src/shared/dom.ts#L4) | 11.6 | 12.7 | 2.1% | 2.3% |
| `cell` | [web/src/shared/dom.ts:14](../../../web/src/shared/dom.ts#L14) | 0.0 | 8.5 | 0.0% | 1.5% |
| `processUrl` | [web/src/shared/navigation.ts:4](../../../web/src/shared/navigation.ts#L4) | 1.1 | 5.0 | 0.2% | 0.9% |
| `(anonymous)` | [web/src/list/data.ts:58](../../../web/src/list/data.ts#L58) | 0.0 | 3.2 | 0.0% | 0.6% |
| `api` | [web/src/shared/api.ts:5](../../../web/src/shared/api.ts#L5) | 1.1 | 3.2 | 0.2% | 0.6% |
| `rows` | [web/src/list/search.ts:32](../../../web/src/list/search.ts#L32) | 1.1 | 1.1 | 0.2% | 0.2% |
| `num` | [web/src/shared/display.ts:123](../../../web/src/shared/display.ts#L123) | 1.1 | 1.1 | 0.2% | 0.2% |

### Largest self times

| Function | Definition | Self (ms) | Total (ms) | Self / active | Total / active |
| --- | --- | ---: | ---: | ---: | ---: |
| `(program)` | (V8/native) | 427.0 | 427.0 | 76.4% | 76.4% |
| `bytes` | [web/src/shared/display.ts:129](../../../web/src/shared/display.ts#L129) | 54.9 | 56.0 | 9.8% | 10.0% |
| `replaceChildren` | (V8/native) | 32.8 | 32.8 | 5.9% | 5.9% |
| `(garbage collector)` | (V8/native) | 13.8 | 13.8 | 2.5% | 2.5% |
| `node` | [web/src/shared/dom.ts:4](../../../web/src/shared/dom.ts#L4) | 11.6 | 12.7 | 2.1% | 2.3% |
| `append` | (V8/native) | 4.2 | 4.2 | 0.8% | 0.8% |
| `renderProcesses` | [web/src/list/renderer.ts:8](../../../web/src/list/renderer.ts#L8) | 3.2 | 113.8 | 0.6% | 20.4% |
| `fetch` | (V8/native) | 2.1 | 2.1 | 0.4% | 0.4% |
| `URLSearchParams` | (V8/native) | 2.1 | 2.1 | 0.4% | 0.4% |
| `toString` | (V8/native) | 1.8 | 1.8 | 0.3% | 0.3% |
| `processUrl` | [web/src/shared/navigation.ts:4](../../../web/src/shared/navigation.ts#L4) | 1.1 | 5.0 | 0.2% | 0.9% |
| `api` | [web/src/shared/api.ts:5](../../../web/src/shared/api.ts#L5) | 1.1 | 3.2 | 0.2% | 0.6% |
| `rows` | [web/src/list/search.ts:32](../../../web/src/list/search.ts#L32) | 1.1 | 1.1 | 0.2% | 0.2% |
| `num` | [web/src/shared/display.ts:123](../../../web/src/shared/display.ts#L123) | 1.1 | 1.1 | 0.2% | 0.2% |
| `createElement` | (V8/native) | 1.1 | 1.1 | 0.2% | 0.2% |

