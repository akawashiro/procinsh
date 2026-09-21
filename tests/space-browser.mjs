// Browser integration without npm dependencies. Requires Node >=22 and Chrome.
import {spawn} from 'node:child_process';
import {mkdtemp, rm, writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import assert from 'node:assert/strict';
import {checkAutoSnapshot} from './auto-snapshot.mjs';
import {checkProcessDetails} from './process-details.mjs';
import {checkDescriptors} from './fds.mjs';
import {checkFileSpace} from './space-files.mjs';
import {checkNetworkSpace} from './space-network.mjs';

const children = [], errors = [];
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
function launch(program, args) {
  const p = spawn(program, args, {stdio: ['ignore', 'pipe', 'pipe'], env: {...process.env, PROCINSH_TEST_ENV: 'value=with\nline <b>literal</b>'}});
  p.output = ''; p.stdout.on('data', b => p.output += b); p.stderr.on('data', b => p.output += b);
  p.on('error', e => { p.output += e.message; });
  children.push(p); return p;
}
async function until(fn, label, timeout = 15000) {
  const end = Date.now() + timeout;
  while (Date.now() < end) { const value = await fn(); if (value) return value; await delay(80); }
  throw new Error(`Timed out: ${label}`);
}
const profile = await mkdtemp(join(tmpdir(), 'procinsh-browser-'));
let socket;
try {
  const app = launch('target/debug/procinsh', ['--listen', '127.0.0.1:0', '--interval', '100ms']);
  const url = await until(() => app.output.match(/http:\/\/127\.0\.0\.1:\d+/)?.[0], 'HTTP server');
  const chrome = launch(process.env.CHROME || '/opt/google/chrome/google-chrome', ['--headless=new', '--no-sandbox', '--use-gl=angle', '--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--disable-dev-shm-usage', '--no-first-run', '--remote-debugging-port=0', `--user-data-dir=${profile}`, 'about:blank']);
  const debugUrl = await until(() => chrome.output.match(/ws:\/\/127\.0\.0\.1:(\d+)\/devtools\/browser\/[\w-]+/)?.[0], 'Chrome DevTools');
  const debugPort = new URL(debugUrl).port;
  const pages = await (await fetch(`http://127.0.0.1:${debugPort}/json/list`)).json();
  socket = new WebSocket(pages.find(p => p.type === 'page').webSocketDebuggerUrl);
  await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
  let sequence = 0; const pending = new Map();
  socket.onmessage = event => {
    const data = JSON.parse(event.data);
    if (data.id) { const task = pending.get(data.id); if (task) { pending.delete(data.id); data.error ? task.reject(data.error) : task.resolve(data.result); } }
    if (data.method === 'Runtime.exceptionThrown') errors.push(data.params.exceptionDetails.text + ' ' + (data.params.exceptionDetails.exception?.description || ''));
    if (data.method === 'Log.entryAdded' && data.params.entry.level === 'error' && !data.params.entry.url?.endsWith('/favicon.ico')) errors.push(data.params.entry.text);
  };
  const cdp = (method, params = {}) => new Promise((resolve, reject) => { const id = ++sequence; pending.set(id, {resolve,reject}); socket.send(JSON.stringify({id,method,params})); });
  const evaluate = async expression => {
    const result = await cdp('Runtime.evaluate', {expression, returnByValue: true, awaitPromise: true});
    if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails));
    return result.result.value;
  };
  const waitFor = (expression, label) => until(() => evaluate(expression), label);
  await cdp('Runtime.enable'); await cdp('Log.enable'); await cdp('Page.enable');
  await cdp('Emulation.setDeviceMetricsOverride', {width: 1440, height: 1100, deviceScaleFactor: 1, mobile: false});
  await cdp('Page.addScriptToEvaluateOnNewDocument', {source: `
    window.spaceTestSources=[];window.spaceTestLeases=[];
    const nativeFetch=window.fetch;
    window.fetch=(url,options)=>{if(url==='/api/space/leases'&&options?.method==='POST')window.spaceTestLeases.push(JSON.parse(options.body));return nativeFetch(url,options);};
    const NativeEventSource=window.EventSource;
    window.EventSource=class extends NativeEventSource {
      constructor(...args){super(...args);window.spaceTestSources.push(this);}
    };
  `});
  await cdp('Page.navigate', {url: url+'/space'});
  const snapshot=await until(async()=>{const data=await (await fetch(url+'/api/space/snapshot')).json();return data.nodes.length>2?data:null;},'space topology');
  const first=snapshot.nodes[0].identity;
  await waitFor(`import('/space.js').then(m=>Boolean(m.processPosition('${first.pid}:${first.start_time_ticks}')))`, 'rendered space topology');
  assert.equal(await evaluate("document.documentElement.lang"),'en');
  assert.equal(await evaluate("document.querySelector('header #brand').textContent"),'procinsh');
  assert.equal(await evaluate("document.querySelector('.counts')"),null,'process counts are removed');
  assert.equal(await evaluate("document.querySelector('.telemetry')"),null,'sensor status is removed');
  assert.equal(await evaluate("document.querySelector('header #back').textContent"),'Back to process list');
  assert.equal(await evaluate("document.querySelector('header #back').getAttribute('href')"),'/');
  assert.equal(await evaluate("document.querySelector('footer')"),null,'footer content is moved into the header');
  assert.match(await evaluate("document.querySelector('header').textContent"),/CODE[\s\S]*HEAP[\s\S]*CPU[\s\S]*WRITE[\s\S]*DRAG · ORBIT/);
  assert.ok(await evaluate("document.querySelector('header').getBoundingClientRect().height<=42"),'header is compact');
  assert.equal(await evaluate("getComputedStyle(document.getElementById('labels')).pointerEvents"),'none');
  assert.ok(await evaluate("(()=>{const c=document.getElementById('labels'),d=c.getContext('2d').getImageData(0,0,c.width,c.height).data;for(let i=3;i<d.length;i+=4)if(d[i])return true;return false})()"),'process names are rendered above the 3D scene');
  assert.equal(await evaluate("document.getElementById('connection')"),null,'live status label is removed');
  assert.equal(await evaluate("document.getElementById('shared')"),null,'shared FD connections are always enabled without a toggle');
  assert.equal(await evaluate("document.getElementById('connected')"),null,'connected-only filter is removed');
  assert.deepEqual(await evaluate("Array.from(document.querySelector('.tools').children,e=>e.id)"),['search','density','reset','rearrange']);
  await delay(1000);
  assert.ok(snapshot.nodes.length>2);
  assert.equal(await (await fetch(url+'/api/target')).json(),null,'space must not select inspector target');
  const n=snapshot.nodes.find(n=>n.identity.pid===app.pid);
  await evaluate(`document.getElementById('search').value='${app.pid}'; document.getElementById('search').dispatchEvent(new Event('input')); document.getElementById('search').dispatchEvent(new KeyboardEvent('keydown',{key:'Enter'}));`);
  assert.match(await evaluate("document.getElementById('pid').textContent"),new RegExp(String(app.pid)));
  assert.equal(await evaluate("document.getElementById('inspect').getAttribute('href')"), `/process/${app.pid}`);
  await waitFor(`window.spaceTestLeases.at(-1)?.selected_process?.pid===${app.pid}`, 'selected process lease');
  await evaluate("document.getElementById('close').click()");
  await waitFor("window.spaceTestLeases.at(-1)?.selected_process===null", 'selection released');
  assert.equal(await evaluate("document.getElementById('regions')"),null,'address-space list is removed from process details');
  await evaluate(`(async()=>{
    window.spaceTestSources.forEach(source=>source.close());
    const m=await import('/space.js'), id={pid:424242,start_time_ticks:7}, peerId={pid:434343,start_time_ticks:8};
    const node={identity:id,name:'cpu-glow-test',uid:1000,username:'test',rss_bytes:4096,cpu_percent:0,maps_epoch:1,maps:[{start:'0x1000',end:'0x2000',permissions:'rw-p',writable:true,executable:false,pathname:'[heap]'}]};
    const peer={...node,identity:peerId,parent_id:id,name:'connection-peer',username:'peer'};
    const a={process_id:id,fd:4,fd_count:1,resource:'socket:1:10',kind:'socket',access:2};
    const b={process_id:peerId,fd:9,fd_count:2,resource:'socket:1:11',kind:'socket',access:2};
    const edges=[
      {id:'unix-exact',a,b,label:'UNIX STREAM',shared:false,candidate:false},
      {id:'tcp-candidate',a,b,label:'TCP ESTABLISHED',shared:false,candidate:true},
      {id:'pipe-shared',a:{...a,kind:'pipe',resource:'pipe:1:20'},b:{...b,kind:'pipe',resource:'pipe:1:20'},label:'pipe',shared:true,candidate:false},
      {id:'external',a:{...a,resource:'socket:1:99'},b:null,label:'UNIX STREAM external',shared:false,candidate:false},
    ];
    document.getElementById('search').value='';
    m.renderTopology({nodes:[node,peer],edges,captured_at:Date.now(),inspected_processes:2,inspected_fds:4,warnings:[]});
    m.selectConnection('unix-exact');
    m.renderActivity({window_ms:100,cpu:[{process_id:id,runtime_ns:40000000,switches:2,running_threads:1,cpus:[3]}],memory:[],ipc:[{process_id:id,resource:'socket:1:10',write:true,bytes:4096,count:16}],status:{cpu:'observing',ipc:'observing'}});
  })()`);
  await delay(50);
  assert.ok(await evaluate("import('/space.js').then(m=>m.processPosition('434343:8').y>m.processPosition('424242:7').y)"),'child is placed in a deeper generation');
  assert.ok(await evaluate("import('/space.js').then(m=>m.parentLineVisual('424242:7','434343:8').g<0.5)"),'unselected parent line is muted');
  await evaluate("import('/space.js').then(m=>m.selectProcess('434343:8'))");
  assert.ok(await evaluate("import('/space.js').then(m=>m.parentLineVisual('424242:7','434343:8').g>0.9)"),'selected ancestry is highlighted');
  await evaluate("import('/space.js').then(m=>m.selectProcess('424242:7'))");
  assert.ok(await evaluate("import('/space.js').then(m=>m.parentLineVisual('424242:7','434343:8').g>0.9)"),'selected direct child is highlighted');
  await evaluate("import('/space.js').then(m=>m.selectConnection('unix-exact'))");
  assert.equal(await evaluate("document.getElementById('connection-label').textContent"),'UNIX STREAM');
  assert.match(await evaluate("document.getElementById('connection-facts').textContent"),/4096 bytes \/ 16 operations/);
  assert.match(await evaluate("document.getElementById('connection-endpoints').textContent"),/cpu-glow-test[\s\S]*PID 424242[\s\S]*FD 4[\s\S]*connection-peer[\s\S]*PID 434343[\s\S]*FD 9/);
  assert.deepEqual(await evaluate("Array.from(document.querySelectorAll('#connection-endpoints a'),a=>a.getAttribute('href'))"),['/process/424242','/process/434343']);
  await evaluate("import('/space.js').then(m=>m.selectConnection('tcp-candidate'))");
  assert.equal(await evaluate("document.getElementById('connection-state').textContent"),'Candidate peer');
  await evaluate("import('/space.js').then(m=>m.selectConnection('pipe-shared'))");
  assert.equal(await evaluate("document.getElementById('connection-state').textContent"),'Shared FD');
  await evaluate("import('/space.js').then(m=>m.selectConnection('external'))");
  assert.match(await evaluate("document.getElementById('connection-endpoints').textContent"),/External \/ unknown/);
  await evaluate("import('/space.js').then(m=>m.renderTopology({nodes:[{identity:{pid:424242,start_time_ticks:7},name:'cpu-glow-test',uid:1000,username:'test',rss_bytes:4096,cpu_percent:0,maps_epoch:1,maps:[]}],edges:[],captured_at:Date.now(),inspected_processes:1,inspected_fds:0,warnings:[]}))");
  assert.equal(await evaluate("document.getElementById('details').hidden"),true,'removed connection clears selection');
  await evaluate("import('/space.js').then(m=>m.renderActivity({window_ms:100,cpu:[{process_id:{pid:424242,start_time_ticks:7},runtime_ns:40000000,switches:2,running_threads:1,cpus:[3]}],memory:[],ipc:[],status:{cpu:'observing'}}))");
  await until(()=>evaluate("import('/space.js').then(m=>m.cpuGlowVisual('424242:7').g)"),'CPU activity lights the base',400);
  await evaluate("import('/space.js').then(m=>m.renderActivity({window_ms:100,cpu:[{process_id:{pid:424242,start_time_ticks:999},runtime_ns:100000000,switches:1,running_threads:1,cpus:[2]}],memory:[],ipc:[],status:{cpu:'observing'}}))");
  assert.equal(await evaluate("import('/space.js').then(m=>m.cpuGlowStates.has('424242:999'))"),false,'stale identity is ignored');
  await delay(550);
  assert.ok(await evaluate("import('/space.js').then(m=>m.cpuGlowVisual('424242:7').g)")<0.01,'CPU afterglow ends');
  const stableChecks=await evaluate(`(async()=>{
    const m=await import('/space.js'), model=await import('/space-model.js');
    const make=(pid,parent=null)=>({identity:{pid,start_time_ticks:1},parent_id:parent&&{pid:parent,start_time_ticks:1},name:'stable-'+pid,uid:1000,rss_bytes:4096,cpu_percent:0,maps_epoch:1,maps:[]});
    const root=make(800001),child=make(800002,800001),sibling=make(800003,800001);
    const edge={id:'stable-edge',a:{process_id:root.identity,fd:1,resource:'pipe:stable'},b:{process_id:child.identity,fd:2,resource:'pipe:stable'},label:'stable pipe',shared:false,candidate:false};
    const render=nodes=>m.renderTopology({nodes,edges:nodes.includes(root)&&nodes.includes(child)?[edge]:[]});
    const coords=nodes=>nodes.map(n=>m.processPosition(model.key(n.identity)));
    render([root,child,sibling]);
    m.selectProcess('800002:1',true);
    const before=coords([root,child,sibling]),view=m.cameraView();
    for(let i=0;i<4;i++) render([make(800010+i,800001),sibling,child,{...root,rss_bytes:2097152}]);
    const stable=JSON.stringify(before)===JSON.stringify(coords([root,child,sibling]));
    const cameraStable=JSON.stringify(view)===JSON.stringify(m.cameraView());
    m.selectProcess('800001:1');
    const freshDetails=document.getElementById('facts').textContent.includes('RSS 2.0 MiB');
    render([root,child,sibling,make(800020,800002)]);
    m.selectConnection('stable-edge');
    document.getElementById('search').value='stable';
    document.getElementById('search').dispatchEvent(new Event('input'));
    document.getElementById('rearrange').click();
    const expected=model.treeLayout([root,child,sibling,make(800020,800002)]);
    const arranged=[root,child,sibling].every(n=>{const actual=m.processPosition(model.key(n.identity)),p=expected.get(model.key(n.identity));return actual.x===p.x&&actual.y===p.y;});
    const selectionKept=!document.getElementById('details').hidden&&document.getElementById('connection-label').textContent==='stable pipe';
    const searchKept=document.getElementById('search').value==='stable';
    const linesKept=Boolean(m.parentLineVisual('800001:1','800002:1'));
    const after=coords([root,child,sibling]);
    document.getElementById('reset').click();
    const resetStable=JSON.stringify(after)===JSON.stringify(coords([root,child,sibling]));
    const resetCleared=document.getElementById('search').value===''&&document.getElementById('details').hidden;
    m.selectProcess('800002:1');document.getElementById('rearrange').click();
    const processKept=!document.getElementById('details').hidden&&document.getElementById('name').textContent==='stable-800002';
    return {stable,cameraStable,freshDetails,arranged,selectionKept,searchKept,linesKept,resetStable,resetCleared,processKept};
  })()`);
  for(const [check,passed] of Object.entries(stableChecks)) assert.equal(passed,true,check);
  await checkFileSpace(evaluate,delay,cdp);
  await checkNetworkSpace(evaluate,delay,cdp);
  // A close-up right drag should travel as far in world space as a focused drag.
  await evaluate("import('/space.js').then(m=>m.selectProcess('900001:1',true))");
  await delay(150);
  const panDistance=async()=>{
    const before=await evaluate("import('/space.js').then(m=>m.cameraView())");
    await cdp('Input.dispatchMouseEvent',{type:'mousePressed',x:700,y:600,button:'right',buttons:2,clickCount:1});
    await cdp('Input.dispatchMouseEvent',{type:'mouseMoved',x:800,y:600,button:'right',buttons:2});
    await cdp('Input.dispatchMouseEvent',{type:'mouseReleased',x:800,y:600,button:'right',buttons:0,clickCount:1});
    await delay(1200);
    const after=await evaluate("import('/space.js').then(m=>m.cameraView())");
    return Math.hypot(...after.target.map((v,i)=>v-before.target[i]));
  };
  const focusedPan=await panDistance();
  for(let i=0;i<12;i++)await cdp('Input.dispatchMouseEvent',{type:'mouseWheel',x:700,y:600,deltaX:0,deltaY:-200});
  await delay(300);
  const closeView=await evaluate("import('/space.js').then(m=>m.cameraView())");
  assert.ok(Math.hypot(...closeView.position.map((v,i)=>v-closeView.target[i]))<10,'wheel zoom reaches close-up');
  const closePan=await panDistance();
  assert.ok(focusedPan>1&&closePan/focusedPan>.75&&closePan/focusedPan<1.25,'close-up panning preserves usable world-space speed');

  await evaluate(`import('/space.js').then(m=>m.renderTopology(${JSON.stringify(snapshot)}))`);
  await evaluate("document.getElementById('density').value='1'; document.getElementById('density').dispatchEvent(new Event('change'))");
  await until(async()=> (await (await fetch(url+'/api/space/status')).json()).density===1,'density change');
  await evaluate("document.getElementById('reset').click()");
  await delay(800);
  const png=await cdp('Page.captureScreenshot',{format:'png'});await writeFile('target/browser-space.png',Buffer.from(png.data,'base64'));
  await evaluate(`(async()=>{
    const {renderTopology,renderActivity,fitTopology}=await import('/space.js');
    const nodes=Array.from({length:1000},(_,i)=>({identity:{pid:100000+i,start_time_ticks:1},name:'load-'+i,uid:99999,username:'fixture',rss_bytes:1048576,cpu_percent:0,maps_epoch:1,maps:Array.from({length:16},(_,j)=>({start:'0x'+(4096+j*8192).toString(16),end:'0x'+(8192+j*8192).toString(16),permissions:'rw-p',writable:true,executable:false,pathname:j===0?'[heap]':null}))}));
    for(let i=1;i<nodes.length;i++)nodes[i].parent_id=nodes[Math.floor((i-1)/4)].identity;
    const edges=Array.from({length:5000},(_,i)=>({id:'load-'+i,a:{process_id:nodes[i%1000].identity,fd:i,resource:'pipe:0:'+i},b:{process_id:nodes[(i*7+1)%1000].identity,fd:i,resource:'pipe:0:'+i},label:'PIPE',shared:false,candidate:false}));
    for(let i=0;i<1000;i++)edges.push({id:'network-load-'+i,a:{process_id:nodes[i%100].identity,fd:6000+i,resource:'socket:load:'+i},b:null,label:'TCP network',shared:false,candidate:false,socket:{protocol:'TCP',state:'ESTABLISHED',local:'127.0.0.1:5000',remote:'203.0.113.1:'+ (4000+i),network_peer:true}});
    renderTopology({nodes,edges,captured_at:Date.now(),inspected_processes:1000,inspected_fds:10000,warnings:[]});
    renderActivity({files:Array.from({length:512},(_,i)=>({process_id:nodes[i%100].identity,resource:'file:load:'+i,path:'/tmp/load-'+i,write:i%2===0,bytes:4096,count:1}))});fitTopology();
  })()`);
  assert.equal(await evaluate("import('/space.js').then(m=>m.networkVisuals().length)"),1000,'large network topology renders all destination markers');
  assert.equal(await evaluate("import('/space.js').then(m=>m.fileVisuals().length)"),512,'file marker display limit renders alongside network topology');
  await delay(1500); console.log('1000 nodes / 6000 edges / 1000 destinations / 512 files:',await evaluate("document.getElementById('fps').textContent"));
  await cdp('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true});
  assert.equal(await evaluate('document.documentElement.scrollWidth <= 390'),true);
  assert.ok(await evaluate("(()=>{const r=document.querySelector('.tools').getBoundingClientRect();return r.left>=0&&r.right<=390&&r.top>=0&&r.bottom<=844})()"),'mobile tools remain in the viewport');
  await cdp('Page.navigate',{url});
  await until(async()=> (await (await fetch(url+'/api/space/status')).json()).active===false,'observer stop');
  assert.deepEqual(errors.filter(e=>!/favicon.ico/.test(e)),[]);
  console.log('Space browser checks passed: WebGL, topology, minimal tools, connection details/states/links, CPU base glow/fade, stale identity, selection, density, no target mutation, mobile, observation shutdown.');

} finally {
  socket?.close();
  for (const child of children.reverse()) if (child.exitCode === null && child.signalCode === null) child.kill('SIGTERM');
  await delay(300);
  for (const child of children) if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL');
  await rm(profile, {recursive: true, force: true}).catch(() => {});
}
