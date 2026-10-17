#!/usr/bin/env node
// Run inside the isolated experiment container as root; browsers run as UID 1000.
import {spawn, execFileSync} from 'node:child_process';
import {readFile, writeFile, mkdir, open, mkdtemp, chown} from 'node:fs/promises';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';

const root = process.argv[2] || '/experiment';
const warmupSeconds = Number(process.env.WARMUP_SECONDS || 30);
const sampleSeconds = Number(process.env.SAMPLE_SECONDS || 60);
const cases = {
  normal: {active:true, maps:true},
  no_maps: {active:true, maps:false},
};
const order = (process.env.ORDER || 'normal,no_maps,no_maps,normal').split(',');
const output = join(root, `run-${new Date().toISOString().replace(/[:.]/g, '-')}`);
await mkdir(output);
const base = 'http://127.0.0.1:9099';
const children = [];
const sockets = [];
const records = [];
const assert = (ok, message) => { if (!ok) throw new Error(message); };
const log = s => console.log(`${new Date().toISOString()} ${s}`);
async function launch(command, args, filename, browser = false, overrides = {}) {
  const file = await open(join(output, filename), 'w');
  const child = spawn(command, args, {stdio:['ignore','pipe','pipe'],
    ...(browser ? {uid:1000, gid:1000} : {}),
    env:{...process.env, HOME:'/tmp/benchmark-user', DISPLAY:'', WAYLAND_DISPLAY:'',
      TAB_COUNT:'20', INTERVAL_SECONDS:'10', PERF_BUILDID_DIR:'/tmp/perf-cache', ...overrides}});
  child.text = '';
  child.done = new Promise(resolve => {
    child.once('error', error => resolve({error:String(error)}));
    child.once('exit', (code, signal) => resolve({code, signal}));
  });
  for (const stream of [child.stdout,child.stderr]) stream.on('data', bytes => {
    child.text = (child.text + bytes.toString()).slice(-300000);
    file.write(bytes).catch(() => {});
  });
  child.done.then(() => file.close());
  children.push(child);
  return child;
}
async function stop(child) {
  if (!child || child.exitCode !== null || child.signalCode !== null) return;
  child.kill('SIGTERM');
  await Promise.race([child.done, delay(5000)]);
  if (child.exitCode === null && child.signalCode === null) {
    child.kill('SIGKILL'); await child.done;
  }
}
async function until(read, label, seconds = 60) {
  const deadline = Date.now() + seconds*1000;
  while (Date.now() < deadline) {
    const value = await read();
    if (value) return value;
    await delay(200);
  }
  throw new Error(`Timed out: ${label}`);
}
async function connect(url) {
  const ws = new WebSocket(url);
  sockets.push(ws);
  await new Promise((resolve,reject) => { ws.onopen=resolve;ws.onerror=reject; });
  let seq=0;
  const pending=new Map();
  ws.onmessage=event=>{
    const msg=JSON.parse(event.data);if (!msg.id) return;
    const p=pending.get(msg.id);if (!p) return;
    pending.delete(msg.id);clearTimeout(p.timer);
    msg.error?p.reject(new Error(JSON.stringify(msg.error))):p.resolve(msg.result);
  };
  return (method,params={})=>new Promise((resolve,reject)=>{
    const id=++seq;
    const timer=setTimeout(()=>{pending.delete(id);reject(new Error(method+' timed out'));},30000);
    pending.set(id,{resolve,reject,timer});ws.send(JSON.stringify({id,method,params}));
  });
}
const capture=`(() => {
  window.benchmark = {counts:{snapshot:0,activity:0,gap:0},errors:[],status:null,snapshot:null,snapshotTimes:[],activityTimes:[]};
  let knownMaps=new Map();
  const Original=window.EventSource;
  window.EventSource=class extends Original {
    constructor(url, options) {
      super(url, options);
      for (const event of ['snapshot','activity','gap']) this.addEventListener(event, message=>{
        const b=window.benchmark; b.counts[event]++;
        if (event === 'snapshot') b.snapshotTimes.push(performance.now());
        if (event === 'activity') b.activityTimes.push(performance.now());
        const v=JSON.parse(message.data);
        if (event==='activity') b.status=v.status;
        if (event==='snapshot') {
          const next=new Map();
          for (const p of v.processes) {
            const id=p.identity.pid+':'+p.identity.start_time_ticks;
            const maps=p.maps ? new Map(p.maps.map(m=>[m.start,m])) : (knownMaps.get(id)||new Map());
            if (p.maps_delta) {
              for (const addr of p.maps_delta.remove) maps.delete(addr);
              for (const m of p.maps_delta.upsert) maps.set(m.start,m);
            }
            next.set(id,maps);
          }
          knownMaps=next;
          b.snapshot={processes:v.processes.length, inspectedFds:v.inspected_fds, inspectedProcesses:v.inspected_processes,
            maps:[...knownMaps.values()].reduce((s,m)=>s+m.size,0),
            mapErrors:v.processes.filter(p=>p.maps_error).map(p=>p.maps_error)};
        }
      });
      this.addEventListener('error',()=>window.benchmark.errors.push('SSE error: '+url));
    }
  };
  addEventListener('error',e=>window.benchmark.errors.push(e.message));
})()`;
let cdp;
async function evaluate(expression) {
  const r=await cdp('Runtime.evaluate',{expression,returnByValue:true,awaitPromise:true});
  if (r.exceptionDetails) throw new Error(JSON.stringify(r.exceptionDetails));
  return r.result.value;
}
async function navigate(url) {
  await cdp('Page.navigate',{url});
  await until(()=>evaluate(`document.readyState==='complete' && location.href===${JSON.stringify(url)}`),'navigation');
}
async function workloadState(workload) {
  const pid=Number(workload.text.match(/Chrome PID=(\d+)/)?.[1]);
  const args=(await readFile(`/proc/${pid}/cmdline`,'utf8')).split('\0');
  const dir=args.find(a=>a.startsWith('--user-data-dir=')).split('=').slice(1).join('=');
  const port=(await readFile(join(dir,'DevToolsActivePort'),'utf8')).split('\n')[0];
  const pages=(await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()).filter(p=>p.type==='page');
  return {tabs:pages.length,titled:pages.filter(p=>p.title).length,
    chromeErrorPages:pages.filter(p=>p.url.startsWith('chrome-error:')).length,
    challengePages:pages.filter(p=>/just a moment|access denied|attention required/i.test(p.title)).length,
    navigationErrors:workload.text.split('\n').filter(line=>line.includes('navigation error'))};
}
try {
  await mkdir('/tmp/benchmark-user',{recursive:true});await chown('/tmp/benchmark-user',1000,1000);
  const workload=await launch('/usr/bin/node',[join(root,'source/scripts/chrome_load.mjs')],'chrome-workload.log',true);
  await until(()=>/ready: \d+ page tabs/.test(workload.text),'20-tab workload readiness',120);
  const dir=await mkdtemp('/tmp/space-observer-');await chown(dir,1000,1000);
  const observer=await launch('/usr/bin/google-chrome',['--headless=new','--disable-dev-shm-usage',
    '--no-first-run','--no-default-browser-check','--remote-debugging-port=0',`--user-data-dir=${dir}`,
    '--use-gl=angle','--use-angle=swiftshader','--enable-unsafe-swiftshader','--window-size=1280,900',
    'about:blank'],'observer.log',true);
  const endpoint=await until(()=>observer.text.match(/ws:\/\/127\.0\.0\.1:\d+\/devtools\/browser\/[^\s]+/)?.[0],'observer Chrome');
  const pages=await (await fetch(`http://127.0.0.1:${new URL(endpoint).port}/json/list`)).json();
  cdp=await connect(pages.find(p=>p.type==='page').webSocketDebuggerUrl);
  await cdp('Page.enable');await cdp('Runtime.enable');
  await cdp('Page.addScriptToEvaluateOnNewDocument',{source:capture});
  for (let i=0;i<order.length;i++) {
    const variant=order[i], settings=cases[variant];
    assert(settings, 'Unknown case: '+variant);
    const name=`${i+1}-${variant}`;
    const backend=await launch(join(root,'runtime','experiment-release','scripts/dev_run.sh'),
      ['--listen','127.0.0.1:9099'],`${name}-backend.log`,false,{
        PROCINSH_EXPERIMENT_MAPS_LOG:join(output,`${name}-maps.jsonl`),
        ...(settings.maps ? {} : {PROCINSH_EXPERIMENT_NO_MAPS:'1'}),
      });
    await until(async()=>{
      if (backend.exitCode !== null) throw new Error(backend.text);
      return fetch(base,{signal:AbortSignal.timeout(1000)}).then(r=>r.ok).catch(()=>false);
    },'backend ready');
    if (settings.active) {
      await navigate(base+'/space');
      await until(()=>evaluate('window.benchmark.counts.activity>=2 && window.benchmark.counts.snapshot>=1'),'SPACE SSE ready');
    } else { await navigate('about:blank'); }
    log(`WARMUP ${name} (${warmupSeconds}s), backend PID ${backend.pid}`);
    await delay(warmupSeconds*1000);
    const before=settings.active ? await evaluate('window.benchmark') : null;
    const tabsBefore=await workloadState(workload);
    assert(tabsBefore.tabs===20,'Workload must have exactly 20 tabs');
    log(`MEASURE ${name} (${sampleSeconds}s)`);
    const file=join(output,`${name}-cpu.json`);
    const reader=await launch('/usr/bin/python3',[join(root,'cpu.py'),String(backend.pid),String(sampleSeconds),file],`${name}-cpu.log`);
    const exit=await reader.done;assert(exit.code===0,'CPU reader failed: '+reader.text);
    const cpu=JSON.parse(await readFile(file,'utf8'));
    const after=settings.active ? await evaluate('window.benchmark') : null;
    const tabsAfter=await workloadState(workload);
    if (settings.active) {
      assert(after.counts.activity>before.counts.activity+5,'SPACE activity stream stalled');
      assert(after.counts.snapshot>before.counts.snapshot,'SPACE snapshots stalled');
      assert(after.errors.length===0,'Observer errors: '+after.errors.join('; '));
      if (settings.maps) {
        assert(before.snapshot?.maps>0 && after.snapshot?.maps>0,'SPACE has no memory maps');
      } else {
        assert(before.snapshot?.maps===0 && after.snapshot?.maps===0,'maps ablation failed');
      }
      assert(before.snapshot.processes>0 && after.snapshot.processes>0,'No processes');
      assert(after.snapshot.inspectedFds>0,'FD collection stopped');
      assert(after.snapshot.mapErrors.length===0,'Map errors');
      const sensors=['cpu','ipc','files','signals'].map(s=>after.status?.[s]?.state);
      assert(sensors.every(s=>s==='observing'),'Sensors are not observing: '+sensors);
    }
    assert(tabsAfter.tabs===20,'Workload lost tabs');
    assert(workload.exitCode===null && observer.exitCode===null,'A Chrome process exited');
    const record={variant,settings,iteration:i+1,backendPid:backend.pid,
      ...Object.fromEntries(Object.entries(cpu).filter(([k])=>!['samples','threads'].includes(k))),
      topThreads:cpu.threads.slice(0,8),before,after,workloadBefore:tabsBefore,workloadAfter:tabsAfter};
    records.push(record);
    const result={baseCommit:'7bb1b8a13cafbb0d6ca31b549dd507fdd8ed9036',
      experimentPatch:'experiment.patch',
      warmupSeconds,sampleSeconds,order,kernel:execFileSync('uname',['-r'],{encoding:'utf8'}).trim(),
      chrome:execFileSync('/usr/bin/google-chrome',['--version'],{encoding:'utf8'}).trim(),records};
    await writeFile(join(output,'results.json'),JSON.stringify(result,null,2)+'\n');
    const screenshot=await cdp('Page.captureScreenshot',{format:'png'});
    await writeFile(join(output,`${name}.png`),Buffer.from(screenshot.data,'base64'));
    log(`RESULT ${name}: ${cpu.cpu_percent.toFixed(2)}% (user ${cpu.user_seconds}s, system ${cpu.system_seconds}s)`);
    if (variant==='normal' && i===order.length-1) {
      log(`PERF ${name}: 30s, outside CPU interval`);
      const perf=await launch('/usr/bin/perf',['record','-e','cpu-clock','-F','199',
        '--clockid','mono','-p',String(backend.pid),'-o',join(output,'normal-perf.data'),
        '--','/usr/bin/sleep','30'],'perf.log');
      const exit=await perf.done; assert(exit.code===0,'perf failed: '+perf.text);
    }
    await navigate('about:blank');await stop(backend);
    if (settings.maps) {
      const lines=(await readFile(join(output,`${name}-maps.jsonl`),'utf8')).trim().split('\n').map(JSON.parse);
      const selected=lines.filter(s=>s.start_ns>=cpu.monotonic_start*1e9 && s.end_ns<=cpu.monotonic_end*1e9);
      const boundary=lines.filter(s=>s.start_ns<cpu.monotonic_end*1e9 && s.end_ns>cpu.monotonic_start*1e9 && !selected.includes(s));
      assert(selected.length>0,'No scoped CPU measurements');
      record.mapsTiming={
        scope:'Complete read_maps calls entirely within the whole-process CPU interval; logging is excluded.',
        calls:selected.length, boundaryCalls:boundary.length,
        readCpuSeconds:selected.reduce((n,s)=>n+s.read_cpu_ns,0)/1e9,
        parseCpuSeconds:selected.reduce((n,s)=>n+s.parse_cpu_ns,0)/1e9,
        rows:selected.reduce((n,s)=>n+s.rows,0), bytes:selected.reduce((n,s)=>n+s.bytes,0),
        failedCalls:selected.filter(s=>!s.success).length,
      };
      log(`MAPS ${name}: read ${record.mapsTiming.readCpuSeconds.toFixed(4)}s, parse ${record.mapsTiming.parseCpuSeconds.toFixed(4)}s`);
      await writeFile(join(output,'results.json'),JSON.stringify(result,null,2)+'\n');
    }
    await delay(1500);
  }
  log(`COMPLETED ${output}`);
} finally {
  for (const ws of sockets) ws.close();
  for (const child of children.reverse()) await stop(child);
}
