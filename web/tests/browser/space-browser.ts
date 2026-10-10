import type { SystemSnapshot } from "../../src/shared/api-types.js";
import type { createSpaceCamera } from "../../src/space/camera.js";
type CameraView = ReturnType<ReturnType<typeof createSpaceCamera>["view"]>;
// Browser integration via tsx and DevTools Protocol. Requires Chrome or Chromium.
import {
  launch as spawnCaptured,
  delay,
  until,
  repositoryPath,
  type TestProcess,
} from "../support/runtime.js";
import { connectPage, pages } from "./harness.js";
import { mkdtemp, rm, writeFile, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import assert from "node:assert/strict";
import { checkProcessDetails } from "./process-details.js";
import { checkDescriptors } from "./fds.js";
import { checkFileSpace } from "./space-files.js";
import { checkNetworkSpace } from "./space-network.js";
import { processEventRecording } from "./process-events.js";
import { checkBuildHeader } from "./build-header.js";

// The test build manifest locates the SPACE inspection module.
const manifest = JSON.parse(
  await readFile(
    new URL("../../dist/.vite/manifest.json", import.meta.url),
    "utf8",
  ),
);
const spaceModule = "/" + manifest["src/space/app.ts"].file;

const children: TestProcess[] = [],
  errors: string[] = [];
function launch(program: string, args: string[]) {
  const child = spawnCaptured(program, args, {
    env: {
      ...process.env,
      PROCINSH_TEST_ENV: "value=with\nline <b>literal</b>",
    },
  });
  children.push(child);
  return child;
}
const profile = await mkdtemp(join(tmpdir(), "procinsh-browser-"));
let socket: WebSocket | undefined;
try {
  const app = launch("./scripts/dev_run.sh", ["--listen", "127.0.0.1:0"]);
  const url = await until(
    () => app.output.match(/http:\/\/127\.0\.0\.1:\d+/)?.[0],
    "HTTP server",
  );
  const chrome = launch(
    process.env.CHROME || "/opt/google/chrome/google-chrome",
    [
      "--headless=new",
      "--no-sandbox",
      "--use-gl=angle",
      "--use-angle=swiftshader",
      "--enable-unsafe-swiftshader",
      "--disable-dev-shm-usage",
      "--no-first-run",
      "--remote-debugging-port=0",
      `--user-data-dir=${profile}`,
      "about:blank",
    ],
  );
  const debugUrl = await until(
    () =>
      chrome.output.match(
        /ws:\/\/127\.0\.0\.1:(\d+)\/devtools\/browser\/[\w-]+/,
      )?.[0],
    "Chrome DevTools",
  );
  const debugPort = new URL(debugUrl).port;
  const page = (await pages(debugPort)).find((page) => page.type === "page");
  assert.ok(page, "Chrome exposes a browser page");
  const connection = await connectPage(page.webSocketDebuggerUrl, errors);
  socket = connection.socket;
  const { cdp, evaluate, waitFor } = connection;
  await cdp("Page.addScriptToEvaluateOnNewDocument", {
    source: `window.spaceTestModule=${JSON.stringify(spaceModule)}`,
  });
  await cdp("Runtime.enable");
  await cdp("Log.enable");
  await cdp("Page.enable");
  await cdp("Page.addScriptToEvaluateOnNewDocument", {
    source: processEventRecording,
  });
  await cdp("Emulation.setDeviceMetricsOverride", {
    width: 1440,
    height: 1100,
    deviceScaleFactor: 1,
    mobile: false,
  });
  await cdp("Page.addScriptToEvaluateOnNewDocument", {
    source: `
    window.spaceTestSources=[];
    const NativeEventSource=window.EventSource;
    window.EventSource=class extends NativeEventSource {
      constructor(...args){super(...args);this.snapshots=[];window.spaceTestSources.push(this);this.addEventListener('snapshot',event=>{try{const snapshot=JSON.parse(event.data);this.snapshots.push(snapshot);window.spaceTestSystemSnapshot=snapshot;}catch{}});}
    };
  `,
  });
  await cdp("Page.navigate", { url: url + "/space" });
  const snapshot = await until(
    () =>
      evaluate<SystemSnapshot | null>(
        "window.spaceTestSystemSnapshot?.processes.length>2 ? window.spaceTestSystemSnapshot : null",
      ),
    "space snapshot",
  );
  const first = snapshot.processes[0].identity;
  await waitFor(
    `import(window.spaceTestModule).then(m=>Boolean(m.processPosition('${first.pid}:${first.start_time_ticks}')))`,
    "rendered space snapshot",
  );
  // Feed deterministic FPS samples through the live animation loop, then verify
  // that both the WebGL drawing buffer and the render indicator recover.
  const resolutionFrames = await evaluate<
    Array<{ scale: number; width: number; indicator: string }>
  >(`(async()=>{
    const {AdaptiveRenderScale}=await import(window.spaceTestModule);
    const original=AdaptiveRenderScale.prototype.sample;
    const captures=[];
    let index=0;
    try {
      return await new Promise((resolve,reject)=>{
        const timeout=setTimeout(()=>reject(new Error('Render-scale recovery timed out')),15000);
        AdaptiveRenderScale.prototype.sample=function(){
          if(index===0){this.scale=this.max;this.resetSampling();}
          const scale=original.call(this,index++<3?15:60);
          setTimeout(()=>{
            captures.push({scale,width:document.getElementById('world').width,
              indicator:document.getElementById('fps').textContent});
            if(captures.length===8){clearTimeout(timeout);resolve(captures);}
          },0);
          return scale;
        };
      });
    } finally { AdaptiveRenderScale.prototype.sample=original; }
  })()`);
  assert.equal(
    resolutionFrames[2].scale,
    0.9,
    "sustained low FPS lowers live render scale",
  );
  assert.ok(
    resolutionFrames[2].width < resolutionFrames[0].width,
    "WebGL drawing buffer shrinks",
  );
  assert.match(resolutionFrames[2].indicator, /90% render/);
  assert.equal(
    resolutionFrames[7].scale,
    1,
    "live animation restores initial render scale",
  );
  assert.equal(
    resolutionFrames[7].width,
    resolutionFrames[0].width,
    "WebGL drawing buffer recovers",
  );
  assert.match(resolutionFrames[7].indicator, /100% render/);
  assert.equal(await evaluate("document.documentElement.lang"), "en");
  assert.equal(await evaluate("document.title"), "procinsh");
  const config = await evaluate<{ version: string }>(
    "fetch('/api/config').then(response=>response.json())",
  );
  assert.equal(
    await evaluate("document.querySelector('header #brand').textContent"),
    `procinsh v${config.version}`,
  );
  const buildRevision = await checkBuildHeader(evaluate);
  assert.equal(
    await evaluate("document.querySelector('.counts')"),
    null,
    "process counts are removed",
  );
  assert.equal(
    await evaluate("document.querySelector('.telemetry')"),
    null,
    "sensor status is removed",
  );
  assert.equal(
    await evaluate("document.querySelector('header #back').textContent"),
    "Go to list view",
  );
  assert.equal(
    await evaluate(
      "document.querySelector('header #back').getAttribute('href')",
    ),
    "/list",
  );
  assert.equal(
    await evaluate("document.querySelector('footer')"),
    null,
    "footer content is moved into the header",
  );
  assert.equal(
    await evaluate("document.querySelector('header .legend')"),
    null,
    "header legend is removed",
  );
  assert.match(
    await evaluate<string>("document.querySelector('header').textContent"),
    /DRAG · ORBIT/,
  );
  assert.ok(
    await evaluate(
      "document.querySelector('header').getBoundingClientRect().height<=42",
    ),
    "header is compact",
  );
  assert.equal(
    await evaluate(
      "getComputedStyle(document.getElementById('labels')).pointerEvents",
    ),
    "none",
  );
  assert.ok(
    await evaluate(
      "(()=>{const c=document.getElementById('labels'),d=c.getContext('2d').getImageData(0,0,c.width,c.height).data;for(let i=3;i<d.length;i+=4)if(d[i])return true;return false})()",
    ),
    "process names are rendered above the 3D scene",
  );
  assert.equal(
    await evaluate("document.getElementById('connection')"),
    null,
    "live status label is removed",
  );
  assert.equal(
    await evaluate("document.getElementById('shared')"),
    null,
    "shared FD connections are always enabled without a toggle",
  );
  assert.equal(
    await evaluate("document.getElementById('connected')"),
    null,
    "connected-only filter is removed",
  );
  assert.deepEqual(
    await evaluate(
      "Array.from(document.querySelector('.tools').children,e=>e.id)",
    ),
    ["search", "reset", "rearrange"],
  );
  await delay(1000);
  assert.ok(snapshot.processes.length > 2);
  const n = snapshot.processes.find((n) => n.identity.pid === app.pid);
  assert.ok(n, "backend process appears in snapshot");
  await evaluate(
    `document.getElementById('search').value='${app.pid}'; document.getElementById('search').dispatchEvent(new Event('input')); document.getElementById('search').dispatchEvent(new KeyboardEvent('keydown',{key:'Enter'}));`,
  );
  assert.match(
    await evaluate<string>("document.getElementById('pid').textContent"),
    new RegExp(String(app.pid)),
  );
  assert.equal(
    await evaluate("document.getElementById('inspect').getAttribute('href')"),
    `/process/${app.pid}?start_time_ticks=${n.identity.start_time_ticks}`,
  );
  await evaluate("document.getElementById('reset').click()");
  assert.equal(
    await evaluate("document.getElementById('search').value"),
    "",
    "Fit all clears search",
  );
  assert.equal(
    await evaluate("document.getElementById('regions')"),
    null,
    "address-space list is removed from process details",
  );
  assert.ok(
    await evaluate(
      "window.spaceTestSources.filter(s=>new URL(s.url).pathname==='/api/system/events').every(s=>!new URL(s.url).search)",
    ),
    "SSE needs no token",
  );
  await evaluate("window.extraViewer=new EventSource('/api/system/events')");
  await waitFor(
    "window.extraViewer.readyState===EventSource.OPEN",
    "second viewer",
  );
  await evaluate(
    "Object.defineProperty(document,'hidden',{configurable:true,value:true});document.dispatchEvent(new Event('visibilitychange'))",
  );
  await waitFor(
    "window.spaceTestSources.filter(s=>s!==window.extraViewer).every(s=>s.readyState===EventSource.CLOSED)",
    "hidden viewer closes",
  );
  assert.equal(
    await evaluate("window.extraViewer.readyState===EventSource.OPEN"),
    true,
    "second viewer remains connected",
  );
  await evaluate("window.extraViewer.close()");
  await waitFor(
    "window.spaceTestSources.every(s=>s.readyState===EventSource.CLOSED)",
    "all viewers close",
  );
  await evaluate(
    "Object.defineProperty(document,'hidden',{configurable:true,value:false});document.dispatchEvent(new Event('visibilitychange'))",
  );
  await waitFor(
    "window.spaceTestSources.at(-1).readyState===EventSource.OPEN",
    "visible reconnect",
  );
  await evaluate(
    "window.failedSource=window.spaceTestSources.at(-1);window.failedSource.dispatchEvent(new Event('error'))",
  );
  await waitFor(
    "window.spaceTestSources.at(-1)!==window.failedSource && window.spaceTestSources.at(-1).readyState===EventSource.OPEN",
    "error reconnect",
  );
  await evaluate(
    "window.failedSource.dispatchEvent(new MessageEvent('snapshot',{data:'invalid stale data'}))",
  );
  assert.ok(
    await evaluate("document.getElementById('failure').hidden"),
    "reconnection clears error",
  );
  await evaluate(
    "window.mismatchedSource=window.spaceTestSources.at(-1);window.mismatchedSource.dispatchEvent(new MessageEvent('snapshot',{data:JSON.stringify({kind:'delta',sequence:999,base_sequence:-1,processes:[],fd_relations:[]})}))",
  );
  await waitFor(
    "window.spaceTestSources.at(-1)!==window.mismatchedSource && window.spaceTestSources.at(-1).readyState===EventSource.OPEN",
    "baseline mismatch reconnect",
  );
  await waitFor(
    "window.spaceTestSources.at(-1).snapshots[0]?.kind==='full' && window.spaceTestSources.at(-1).snapshots[0]?.sequence===1",
    "baseline mismatch full reset",
  );
  await evaluate(`(async()=>{
    window.spaceTestSources.forEach(source=>source.close());
    const m=await import(window.spaceTestModule), id={pid:424242,start_time_ticks:7}, peerId={pid:434343,start_time_ticks:8};
    const node={identity:id,name:'cpu-glow-test',uid:1000,username:'test',euid:0,effective_username:'root',maps_epoch:1,maps:[{start:'0x1000',end:'0x2000',readable:true,private:true,writable:true,executable:false,pathname:'[heap]'}]};
    const peer={...node,identity:peerId,parent_id:id,name:'connection-peer',username:'peer'};
    const a={process_id:id,fd:4,fd_count:1,resource:{kind:'socket',device:{major:0,minor:1},inode:'10'},kind:'socket',access:'read_write'};
    const b={process_id:peerId,fd:9,fd_count:2,resource:{kind:'socket',device:{major:0,minor:1},inode:'11'},kind:'socket',access:'read_write'};
    const edges=[
      {id:'unix-exact',endpoint:a,peer:b,label:'UNIX STREAM',shared:false,candidate:false},
      {id:'tcp-candidate',endpoint:a,peer:b,label:'TCP ESTABLISHED',shared:false,candidate:true},
      {id:'pipe-shared',endpoint:{...a,kind:'pipe',resource:{kind:'pipe',device:{major:0,minor:1},inode:'20'}},peer:{...b,kind:'pipe',resource:{kind:'pipe',device:{major:0,minor:1},inode:'20'}},label:'pipe',shared:true,candidate:false},
      {id:'external',endpoint:{...a,resource:{kind:'socket',device:{major:0,minor:1},inode:'99'}},peer:null,label:'UNIX STREAM external',shared:false,candidate:false},
    ];
    m.renderSystemSnapshot({processes:[node,peer],fd_relations:edges,captured_at:Date.now(),inspected_processes:2,inspected_fds:4,warnings:[]});
    m.selectConnection('unix-exact');
    m.renderActivity({window_ms:100,cpu:[{process_id:id,runtime_ns:40000000,switches:2,running_threads:1,cpus:[3]}],ipc:[{process_id:id,resource:{kind:'socket',device:{major:0,minor:1},inode:'10'},write:true,bytes:4096,count:16}],status:{cpu:{state:'observing'},ipc:{state:'observing'}}});
  })()`);
  await delay(50);
  assert.ok(
    await evaluate(
      "import(window.spaceTestModule).then(m=>m.processPosition('434343:8').y>m.processPosition('424242:7').y)",
    ),
    "child is placed in a deeper generation",
  );
  assert.ok(
    await evaluate(
      "import(window.spaceTestModule).then(m=>m.parentLineVisual('424242:7','434343:8').g<0.5)",
    ),
    "unselected parent line is muted",
  );
  await evaluate(
    "import(window.spaceTestModule).then(m=>m.selectProcess('434343:8'))",
  );
  assert.equal(
    await evaluate("document.getElementById('parent-details').hidden"),
    false,
  );
  assert.equal(
    await evaluate("document.getElementById('parent-pid').textContent"),
    "PID 424242 · Real: test (1000) Effective: root (0) ",
  );
  assert.equal(
    await evaluate(
      "document.getElementById('parent-inspect').getAttribute('href')",
    ),
    "/process/424242?start_time_ticks=7",
  );
  assert.equal(
    await evaluate("document.getElementById('parent-name').textContent"),
    "cpu-glow-test",
  );
  assert.ok(
    await evaluate(
      "import(window.spaceTestModule).then(m=>m.parentLineVisual('424242:7','434343:8').g>0.9)",
    ),
    "selected ancestry is highlighted",
  );
  await evaluate(
    "import(window.spaceTestModule).then(m=>m.selectProcess('424242:7'))",
  );
  assert.equal(
    await evaluate("document.getElementById('parent-details').hidden"),
    true,
    "a process without a parent hides the parent section",
  );
  assert.ok(
    await evaluate(
      "import(window.spaceTestModule).then(m=>m.parentLineVisual('424242:7','434343:8').g>0.9)",
    ),
    "selected direct child is highlighted",
  );
  await evaluate(
    "import(window.spaceTestModule).then(m=>m.selectProcess('424242:7',true))",
  );
  await delay(100);
  const hoverText = await evaluate<string>(`(()=>{
    const canvas=document.getElementById('world'),hover=document.getElementById('hover');
    for(let y=80;y<innerHeight;y+=12) for(let x=0;x<innerWidth;x+=12){
      canvas.dispatchEvent(new PointerEvent('pointermove',{clientX:x,clientY:y}));
      if(!hover.hidden&&hover.textContent.includes('cpu-glow-test'))return hover.textContent;
    }
    return null;
  })()`);
  assert.ok(hoverText, "process hover retains name and PID");
  assert.match(hoverText, /424242/);
  assert.doesNotMatch(
    hoverText,
    /Observed CPU|Off CPU|CPU 3|threads|RSS|MiB|%/,
  );
  await evaluate(
    "import(window.spaceTestModule).then(m=>m.selectConnection('unix-exact'))",
  );
  assert.equal(
    await evaluate("document.getElementById('connection-label').textContent"),
    "UNIX STREAM",
  );
  assert.match(
    await evaluate<string>(
      "document.getElementById('connection-facts').textContent",
    ),
    /4096 bytes \/ 16 operations/,
  );
  assert.match(
    await evaluate<string>(
      "document.getElementById('connection-endpoints').textContent",
    ),
    /cpu-glow-test[\s\S]*PID 424242[\s\S]*FD 4[\s\S]*connection-peer[\s\S]*PID 434343[\s\S]*FD 9/,
  );
  assert.deepEqual(
    await evaluate(
      "Array.from(document.querySelectorAll('#connection-endpoints a'),a=>a.getAttribute('href'))",
    ),
    [
      "/process/424242?start_time_ticks=7",
      "/process/434343?start_time_ticks=8",
    ],
  );
  await evaluate(
    "import(window.spaceTestModule).then(m=>m.selectConnection('tcp-candidate'))",
  );
  assert.equal(
    await evaluate("document.getElementById('connection-state').textContent"),
    "Candidate peer",
  );
  await evaluate(
    "import(window.spaceTestModule).then(m=>m.selectConnection('pipe-shared'))",
  );
  assert.equal(
    await evaluate("document.getElementById('connection-state').textContent"),
    "Shared FD",
  );
  await evaluate(
    "import(window.spaceTestModule).then(m=>m.selectConnection('external'))",
  );
  assert.match(
    await evaluate<string>(
      "document.getElementById('connection-endpoints').textContent",
    ),
    /External \/ unknown/,
  );
  await evaluate(
    "import(window.spaceTestModule).then(m=>m.renderSystemSnapshot({processes:[{identity:{pid:424242,start_time_ticks:7},name:'cpu-glow-test',uid:1000,username:'test',maps_epoch:1,maps:[]}],fd_relations:[],captured_at:Date.now(),inspected_processes:1,inspected_fds:0,warnings:[]}))",
  );
  assert.equal(
    await evaluate("document.getElementById('details').hidden"),
    true,
    "removed connection clears selection",
  );
  await evaluate(
    "import(window.spaceTestModule).then(m=>m.renderActivity({window_ms:100,cpu:[{process_id:{pid:424242,start_time_ticks:7},runtime_ns:40000000,switches:2,running_threads:1,cpus:[3]}],ipc:[],status:{cpu:{state:'observing'}}}))",
  );
  await until(
    () =>
      evaluate(
        "import(window.spaceTestModule).then(m=>m.cpuGlowVisual('424242:7').g)",
      ),
    "CPU activity lights the base",
    400,
  );
  await evaluate(
    "import(window.spaceTestModule).then(m=>m.selectProcess('424242:7'))",
  );
  assert.equal(await evaluate("document.getElementById('facts')"), null);
  assert.doesNotMatch(
    await evaluate<string>(
      "document.getElementById('process-details').textContent",
    ),
    /Observed CPU|Off CPU|CPU 3|threads|RSS|MiB|%/,
  );
  await evaluate(
    "import(window.spaceTestModule).then(m=>m.renderActivity({window_ms:100,cpu:[{process_id:{pid:424242,start_time_ticks:999},runtime_ns:100000000,switches:1,running_threads:1,cpus:[2]}],ipc:[],status:{cpu:{state:'observing'}}}))",
  );
  assert.equal(
    await evaluate(
      "import(window.spaceTestModule).then(m=>m.cpuGlowStates.has('424242:999'))",
    ),
    false,
    "stale identity is ignored",
  );
  await delay(550);
  assert.ok(
    (await evaluate<number>(
      "import(window.spaceTestModule).then(m=>m.cpuGlowVisual('424242:7').g)",
    )) < 0.01,
    "CPU afterglow ends",
  );
  const stableChecks = await evaluate<Record<string, boolean>>(`(async()=>{
    const m=await import(window.spaceTestModule), model=await import(window.spaceTestModule);
    const make=(pid,parent=null)=>({identity:{pid,start_time_ticks:1},parent_id:parent&&{pid:parent,start_time_ticks:1},name:'stable-'+pid,uid:1000,maps_epoch:1,maps:[]});
    const root=make(800001),child=make(800002,800001),sibling=make(800003,800001);
    const edge={id:'stable-edge',endpoint:{process_id:root.identity,fd:1,resource:{kind:'pipe',device:{major:0,minor:0},inode:'2176'}},peer:{process_id:child.identity,fd:2,resource:{kind:'pipe',device:{major:0,minor:0},inode:'2176'}},label:'stable pipe',shared:false,candidate:false};
    const render=nodes=>m.renderSystemSnapshot({processes:nodes,fd_relations:nodes.includes(root)&&nodes.includes(child)?[edge]:[]});
    const coords=nodes=>nodes.map(n=>m.processPosition(model.key(n.identity)));
    render([root,child,sibling]);
    const search=document.getElementById('search');
    search.value='  800002  ';
    search.dispatchEvent(new Event('input'));
    const pidFiltered=m.cpuGlowVisual('800002:1')!==null&&m.cpuGlowVisual('800001:1')===null&&m.parentLineVisual('800001:1','800002:1')===null;
    render([root,child,sibling]);
    const filterSurvivesUpdate=m.cpuGlowVisual('800002:1')!==null&&m.cpuGlowVisual('800003:1')===null;
    search.value='no-such-process';
    search.dispatchEvent(new Event('input'));
    search.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter'}));
    const noMatches=[root,child,sibling].every(n=>m.cpuGlowVisual(model.key(n.identity))===null);
    document.getElementById('reset').click();
    const allRestored=[root,child,sibling].every(n=>m.cpuGlowVisual(model.key(n.identity))!==null);
    m.selectProcess('800002:1',true);
    const before=coords([root,child,sibling]),view=m.cameraView();
    for(let i=0;i<4;i++) render([make(800010+i,800001),sibling,child,{...root,name:"updated-root"}]);
    const stable=JSON.stringify(before)===JSON.stringify(coords([root,child,sibling]));
    const cameraStable=JSON.stringify(view)===JSON.stringify(m.cameraView());
    m.selectProcess('800001:1');
    const freshDetails=document.getElementById('name').textContent==='updated-root';
    render([root,child,sibling,make(800020,800002)]);
    m.selectConnection('stable-edge');
    document.getElementById('search').value='STABLE';
    document.getElementById('search').dispatchEvent(new Event('input'));
    document.getElementById('rearrange').click();
    const searchKept=document.getElementById('search').value==='STABLE'&&m.cpuGlowVisual('800001:1')!==null;
    const expected=model.treeLayout([root,child,sibling,make(800020,800002)]);
    const arranged=[root,child,sibling].every(n=>{const actual=m.processPosition(model.key(n.identity)),p=expected.get(model.key(n.identity));return actual.x===p.x&&actual.y===p.y;});
    const selectionKept=!document.getElementById('details').hidden&&document.getElementById('connection-label').textContent==='stable pipe';
    const linesKept=Boolean(m.parentLineVisual('800001:1','800002:1'));
    const after=coords([root,child,sibling]);
    document.getElementById('reset').click();
    const resetStable=JSON.stringify(after)===JSON.stringify(coords([root,child,sibling]));
    const resetCleared=document.getElementById('search').value===''&&document.getElementById('details').hidden;
    m.selectProcess('800002:1');document.getElementById('rearrange').click();
    const processKept=!document.getElementById('details').hidden&&document.getElementById('name').textContent==='stable-800002';
    return {pidFiltered,filterSurvivesUpdate,noMatches,allRestored,stable,cameraStable,freshDetails,arranged,selectionKept,searchKept,linesKept,resetStable,resetCleared,processKept};
  })()`);
  for (const [check, passed] of Object.entries(stableChecks))
    assert.equal(passed, true, check);
  await evaluate(`(async()=>{
    const m=await import(window.spaceTestModule);
    document.getElementById('reset').click();
    const source={pid:810001,start_time_ticks:1},target={pid:810002,start_time_ticks:2};
    m.renderSystemSnapshot({processes:[{identity:source,name:'signal-source',uid:1000,maps:[],maps_epoch:1},{identity:target,parent_id:source,name:'signal-target',uid:1000,maps:[],maps_epoch:1}],fd_relations:[],warnings:[]});
    m.fitScene();
    window.signalFixture={timestamp_ns:123,src_pid:source.pid,dst_pid:target.pid,source_id:source,destination_id:target,signal:15};
    m.renderActivity({signals:[window.signalFixture]});
  })()`);
  const signalFlight = await evaluate<
    { label: string; source: string; destination: string }[]
  >("import(window.spaceTestModule).then(m=>m.signalVisuals())");
  assert.equal(signalFlight.length, 1);
  assert.equal(signalFlight[0].label, "SIGTERM");
  assert.equal(signalFlight[0].source, "810001:1");
  assert.equal(signalFlight[0].destination, "810002:2");
  await until(
    () =>
      evaluate(
        "import(window.spaceTestModule).then(m=>m.cpuGlowVisual('810002:2').r>0.5)",
      ),
    "signal arrival pulses destination",
    2500,
  );
  await delay(450);
  assert.equal(
    await evaluate(
      "import(window.spaceTestModule).then(m=>m.signalVisuals().length)",
    ),
    0,
  );
  assert.ok(
    (await evaluate<number>(
      "import(window.spaceTestModule).then(m=>m.cpuGlowVisual('810002:2').r)",
    )) < 0.01,
  );
  await evaluate(
    "import(window.spaceTestModule).then(m=>m.renderActivity({signals:Array(5000).fill(window.signalFixture)}))",
  );
  assert.equal(
    await evaluate(
      "import(window.spaceTestModule).then(m=>m.signalVisuals().length)",
    ),
    256,
  );
  await evaluate(
    "document.getElementById('search').value='signal-source';document.getElementById('search').dispatchEvent(new Event('input'))",
  );
  assert.equal(
    await evaluate(
      "import(window.spaceTestModule).then(m=>m.signalVisuals().length)",
    ),
    0,
  );
  await evaluate("document.getElementById('reset').click()");
  await checkFileSpace(evaluate, delay, cdp);
  await checkNetworkSpace(evaluate, delay, cdp);
  // A close-up right drag should travel as far in world space as a focused drag.
  await evaluate(
    "import(window.spaceTestModule).then(m=>m.selectProcess('900001:1',true))",
  );
  await delay(150);
  const panDistance = async () => {
    const before = await evaluate<CameraView>(
      "import(window.spaceTestModule).then(m=>m.cameraView())",
    );
    await cdp("Input.dispatchMouseEvent", {
      type: "mousePressed",
      x: 700,
      y: 600,
      button: "right",
      buttons: 2,
      clickCount: 1,
    });
    await cdp("Input.dispatchMouseEvent", {
      type: "mouseMoved",
      x: 800,
      y: 600,
      button: "right",
      buttons: 2,
    });
    await cdp("Input.dispatchMouseEvent", {
      type: "mouseReleased",
      x: 800,
      y: 600,
      button: "right",
      buttons: 0,
      clickCount: 1,
    });
    await delay(1200);
    const after = await evaluate<CameraView>(
      "import(window.spaceTestModule).then(m=>m.cameraView())",
    );
    return Math.hypot(...after.target.map((v, i) => v - before.target[i]));
  };
  const focusedPan = await panDistance();
  for (let i = 0; i < 12; i++)
    await cdp("Input.dispatchMouseEvent", {
      type: "mouseWheel",
      x: 700,
      y: 600,
      deltaX: 0,
      deltaY: -200,
    });
  await delay(300);
  const closeView = await evaluate<CameraView>(
    "import(window.spaceTestModule).then(m=>m.cameraView())",
  );
  assert.ok(
    Math.hypot(...closeView.position.map((v, i) => v - closeView.target[i])) <
      10,
    "wheel zoom reaches close-up",
  );
  const closePan = await panDistance();
  assert.ok(
    focusedPan > 1 &&
      closePan / focusedPan > 0.75 &&
      closePan / focusedPan < 1.25,
    "close-up panning preserves usable world-space speed",
  );

  await evaluate(
    `import(window.spaceTestModule).then(m=>m.renderSystemSnapshot(${JSON.stringify(snapshot)}))`,
  );
  await evaluate("document.getElementById('reset').click()");
  await delay(800);
  const png = await cdp("Page.captureScreenshot", { format: "png" });
  await writeFile(
    repositoryPath("target/browser-space.png"),
    Buffer.from(png.data, "base64"),
  );
  await evaluate(`(async()=>{
    const {renderSystemSnapshot,renderActivity,fitScene}=await import(window.spaceTestModule);
    const nodes=Array.from({length:1000},(_,i)=>({identity:{pid:100000+i,start_time_ticks:1},name:'load-'+i,uid:99999,username:'fixture',maps_epoch:1,maps:Array.from({length:16},(_,j)=>({start:'0x'+(4096+j*8192).toString(16),end:'0x'+(8192+j*8192).toString(16),readable:true,private:true,writable:true,executable:false,pathname:j===0?'[heap]':null}))}));
    for(let i=1;i<nodes.length;i++)nodes[i].parent_id=nodes[Math.floor((i-1)/4)].identity;
    const edges=Array.from({length:5000},(_,i)=>({id:'load-'+i,endpoint:{process_id:nodes[i%1000].identity,fd:i,resource:{kind:'pipe',device:{major:0,minor:0},inode:String(i)}},peer:{process_id:nodes[(i*7+1)%1000].identity,fd:i,resource:{kind:'pipe',device:{major:0,minor:0},inode:String(i)}},label:'PIPE',shared:false,candidate:false}));
    for(let i=0;i<1000;i++)edges.push({id:'network-load-'+i,endpoint:{process_id:nodes[i%100].identity,fd:6000+i,resource:{kind:'socket',device:{major:0,minor:0},inode:String(i)}},peer:null,label:'TCP network',shared:false,candidate:false,socket:{protocol:{kind:'tcp',family:'ipv4'},state:{kind:'established'},local:{ip:'127.0.0.1',port:5000},remote:{ip:'203.0.113.1',port:4000+i},network_peer:true}});
    renderSystemSnapshot({processes:nodes,fd_relations:edges,captured_at:Date.now(),inspected_processes:1000,inspected_fds:10000,warnings:[]});
    renderActivity({files:Array.from({length:512},(_,i)=>({process_id:nodes[i%100].identity,file:{device:{major:0,minor:0},inode:String(i),generation:0},path:'/tmp/load-'+i,write:i%2===0,bytes:4096,count:1}))});fitScene();
  })()`);
  assert.equal(
    await evaluate(
      "import(window.spaceTestModule).then(m=>m.networkVisuals().length)",
    ),
    1000,
    "large network snapshot renders all destination markers",
  );
  assert.equal(
    await evaluate(
      "import(window.spaceTestModule).then(m=>m.fileVisuals().length)",
    ),
    512,
    "file marker display limit renders alongside network snapshot",
  );
  await delay(1500);
  console.log(
    "1000 nodes / 6000 edges / 1000 destinations / 512 files:",
    await evaluate("document.getElementById('fps').textContent"),
  );
  await cdp("Emulation.setDeviceMetricsOverride", {
    width: 390,
    height: 844,
    deviceScaleFactor: 1,
    mobile: true,
  });
  assert.deepEqual(await checkBuildHeader(evaluate), buildRevision);
  assert.equal(
    await evaluate("document.documentElement.scrollWidth <= 390"),
    true,
  );
  assert.ok(
    await evaluate(
      "(()=>{const r=document.querySelector('.tools').getBoundingClientRect();return r.left>=0&&r.right<=390&&r.top>=0&&r.bottom<=844})()",
    ),
    "mobile tools remain in the viewport",
  );
  await evaluate("window.dispatchEvent(new Event('pagehide'))");
  await waitFor(
    "window.spaceTestSources.every(s=>s.readyState===EventSource.CLOSED)",
    "pagehide closes viewers",
  );
  // A live update between mouse down/up must not detach the detail link.
  await cdp("Emulation.setDeviceMetricsOverride", {
    width: 1440,
    height: 1100,
    deviceScaleFactor: 1,
    mobile: false,
  });
  await evaluate(`(async()=>{
    const m=await import(window.spaceTestModule);
    window.linkSnapshot={processes:[${JSON.stringify(n)}],fd_relations:[{
      id:'click-link',endpoint:{process_id:${JSON.stringify(n.identity)},fd:4,fd_count:1,resource:{kind:'pipe',device:{major:0,minor:0},inode:'1561'},kind:'pipe',access:'read_write'},
      peer:null,label:'click regression',shared:false,candidate:false
    }]};
    m.renderSystemSnapshot(window.linkSnapshot);
    m.selectConnection('click-link');
    window.detailLink=document.querySelector('#connection-endpoints a');
    window.detailLink.focus();
  })()`);
  const linkPoint = await evaluate<{ x: number; y: number }>(
    "(()=>{const r=window.detailLink.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2}})()",
  );
  await cdp("Input.dispatchMouseEvent", {
    type: "mousePressed",
    ...linkPoint,
    button: "left",
    buttons: 1,
    clickCount: 1,
  });
  await evaluate(
    "import(window.spaceTestModule).then(m=>{m.renderActivity({window_ms:100,cpu:[],ipc:[]});m.renderSystemSnapshot(window.linkSnapshot)})",
  );
  assert.ok(
    await evaluate(
      "window.detailLink.isConnected && document.activeElement===window.detailLink",
    ),
    "live activity and snapshots preserve the focused link",
  );
  await cdp("Input.dispatchMouseEvent", {
    type: "mouseReleased",
    ...linkPoint,
    button: "left",
    buttons: 0,
    clickCount: 1,
  });
  await waitFor(
    `location.pathname==='/process/${app.pid}' && document.getElementById('inspector')?.hidden===false`,
    "connection link opens process inspector",
  );
  await cdp("Page.navigate", { url });
  assert.deepEqual(
    errors.filter((e) => !/favicon.ico/.test(e)),
    [],
  );
  console.log(
    "Space browser checks passed: WebGL, snapshot, minimal tools, connection details/states/links, CPU base glow/fade, stale identity, process selection, independent graph selection, mobile, SSE connection lifecycle.",
  );
} finally {
  socket?.close();
  for (const child of children.reverse())
    if (child.exitCode === null && child.signalCode === null)
      child.kill("SIGTERM");
  await delay(300);
  for (const child of children)
    if (child.exitCode === null && child.signalCode === null)
      child.kill("SIGKILL");
  await rm(profile, { recursive: true, force: true }).catch(() => {});
}
