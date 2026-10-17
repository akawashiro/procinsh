#!/usr/bin/env node
// Run inside the isolated experiment container as root; browsers run as UID 1000.
import {spawn, execFileSync} from 'node:child_process';
import {readFile, readdir, writeFile, mkdir, open, mkdtemp, chown} from 'node:fs/promises';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';

const root = process.argv[2] || '/experiment';
const warmupSeconds = Number(process.env.WARMUP_SECONDS || 30);
const sampleSeconds = Number(process.env.SAMPLE_SECONDS || 60);
const profiles = (process.env.PROFILES || 'debug,release').split(',');
const order = (process.env.ORDER || 'before,after,after,before').split(',');
const output = join(root, `run-${new Date().toISOString().replace(/[:.]/g, '-')}`);
await mkdir(output);
const base = 'http://127.0.0.1:9097';
const children = [];
const sockets = [];
const records = [];
let diagnosticCaptured = false;
const assert = (ok, message) => { if (!ok) throw new Error(message); };
const log = s => console.log(`${new Date().toISOString()} ${s}`);
async function launch(command, args, filename, browser = false) {
  const file = await open(join(output, filename), 'w');
  const child = spawn(command, args, {stdio:['ignore','pipe','pipe'],
    ...(browser ? {uid:1000, gid:1000} : {}),
    env:{...process.env, HOME:'/tmp/benchmark-user', DISPLAY:'', WAYLAND_DISPLAY:'',
      TAB_COUNT:'20', INTERVAL_SECONDS:'10', PERF_BUILDID_DIR:'/tmp/perf-cache'}});
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
  window.benchmark = {counts:{snapshot:0,activity:0,gap:0},errors:[],status:null,snapshot:null};
  let knownMaps=new Map();
  const Original=window.EventSource;
  window.EventSource=class extends Original {
    constructor(url, options) {
      super(url, options);
      for (const event of ['snapshot','activity','gap']) this.addEventListener(event, message=>{
        const b=window.benchmark; b.counts[event]++;
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
          b.snapshot={processes:v.processes.length,
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
  const workload=await launch('/usr/bin/node',[join(root,'before-source/scripts/chrome_load.mjs')],'chrome-workload.log',true);
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
  for (const profile of profiles) for (let i=0;i<order.length;i++) {
    const variant=order[i];const name=`${profile}-${i+1}-${variant}`;
    const backend=await launch(join(root,'runtime',`${variant}-${profile}`,'scripts/dev_run.sh'),
      ['--listen','127.0.0.1:9097'],`${name}-backend.log`);
    await until(async()=>{
      if (backend.exitCode !== null) throw new Error(backend.text);
      return fetch(base,{signal:AbortSignal.timeout(1000)}).then(r=>r.ok).catch(()=>false);
    },'backend ready');
    await navigate(base+'/space');
    await until(()=>evaluate('window.benchmark.counts.activity>=2 && window.benchmark.counts.snapshot>=1'),'SPACE SSE ready');
    log(`WARMUP ${name} (${warmupSeconds}s), backend PID ${backend.pid}`);
    await delay(warmupSeconds*1000);
    const before=await evaluate('window.benchmark');
    const tabsBefore=await workloadState(workload);
    assert(tabsBefore.tabs===20,'Workload must have exactly 20 tabs');
    if (process.env.CAPTURE_FIXTURE) {
      const texts=[];
      for (const pid of await readdir('/proc')) {
        if (!/^\d+$/.test(pid)) continue;
        try {texts.push(await readFile(`/proc/${pid}/maps`,'utf8'));} catch {}
      }
      await writeFile(join(output,`${name}-fixture.maps`),texts.join(''));
    }
    let perf;
    if (process.env.CAPTURE_PERF) {
      perf=await launch('/usr/bin/perf',['record','-e','cpu-clock','-F','199','--clockid','mono',
        '-p',String(backend.pid),'-o',join(output,`${name}-perf.data`),
        '--','/usr/bin/sleep',String(sampleSeconds)],`${name}-perf.log`);
    }
    log(`MEASURE ${name} (${sampleSeconds}s)`);
    const file=join(output,`${name}-cpu.json`);
    const reader=await launch('/usr/bin/python3',[join(root,'cpu.py'),String(backend.pid),String(sampleSeconds),file],`${name}-cpu.log`);
    const exit=await reader.done;assert(exit.code===0,'CPU reader failed: '+reader.text);
    if (perf) { const p=await perf.done; assert(p.code===0,'perf failed: '+perf.text); }
    const cpu=JSON.parse(await readFile(file,'utf8'));
    const after=await evaluate('window.benchmark');
    const tabsAfter=await workloadState(workload);
    assert(after.counts.activity>before.counts.activity+5,'SPACE activity stream stalled');
    assert(after.counts.snapshot>before.counts.snapshot,'SPACE snapshots stalled');
    assert(after.errors.length===0,'Observer errors: '+after.errors.join('; '));
    assert(before.snapshot?.maps>0 && after.snapshot?.maps>0,'SPACE has no memory maps');
    assert(workload.exitCode===null && observer.exitCode===null,'A Chrome process exited');
    const sensors=['cpu','ipc','files','signals'].map(s=>after.status?.[s]?.state);
    assert(sensors.every(s=>s==='observing'),'Sensors are not observing: '+sensors);
    const record={profile,variant,iteration:i+1,backendPid:backend.pid,
      ...Object.fromEntries(Object.entries(cpu).filter(([k])=>!['samples','threads'].includes(k))),
      topThreads:cpu.threads.slice(0,8),before,after,workloadBefore:tabsBefore,workloadAfter:tabsAfter};
    records.push(record);
    const result={baseCommit:'7bb1b8a13cafbb0d6ca31b549dd507fdd8ed9036',
      afterCommit:process.env.AFTER_COMMIT||null,
      warmupSeconds,sampleSeconds,order,kernel:execFileSync('uname',['-r'],{encoding:'utf8'}).trim(),
      chrome:execFileSync('/usr/bin/google-chrome',['--version'],{encoding:'utf8'}).trim(),records};
    await writeFile(join(output,'results.json'),JSON.stringify(result,null,2)+'\n');
    const screenshot=await cdp('Page.captureScreenshot',{format:'png'});
    await writeFile(join(output,`${name}.png`),Buffer.from(screenshot.data,'base64'));
    log(`RESULT ${name}: ${cpu.cpu_percent.toFixed(2)}% (user ${cpu.user_seconds}s, system ${cpu.system_seconds}s)`);
    if (!diagnosticCaptured && process.env.PROFILE_AFTER_VARIANT===variant) {
      diagnosticCaptured=true;
      log(`PROFILE ${name} after CPU interval (30s)`);
      const diagnostic=await launch('/usr/bin/perf',['record','-e','cpu-clock','-F','199','--clockid','mono',
        '-p',String(backend.pid),'-o',join(output,`${name}-diagnostic-perf.data`),
        '--','/usr/bin/sleep','30'],`${name}-diagnostic-perf.log`);
      const p=await diagnostic.done;assert(p.code===0,'Diagnostic perf failed: '+diagnostic.text);
    }
    await navigate('about:blank');await stop(backend);await delay(1500);
  }
  log(`COMPLETED ${output}`);
} finally {
  for (const ws of sockets) ws.close();
  for (const child of children.reverse()) await stop(child);
}
