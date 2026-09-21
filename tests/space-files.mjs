import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';
export async function checkFileSpace(evaluate,delay,cdp){
  await evaluate(`(async()=>{
    const m=await import('/space.js');
    const node={identity:{pid:910001,start_time_ticks:1},name:'file-browser',uid:1000,euid:1000,rss_bytes:4096,maps:[]};
    window.fileFixture={nodes:[node],edges:[]};m.renderTopology(window.fileFixture);
    const e={process_id:node.identity,resource:'file:8:1:42:0',path:'/tmp/example.txt',bytes:100,count:1};
    window.fileEvent=e;window.fileCamera=m.cameraView();window.fileProcess=m.processPosition('910001:1');
    m.renderActivity({files:[{...e,write:true},{...e,write:false,bytes:200}],status:{files:'observing',files_lost:3}});
    window.fileCreationStable=JSON.stringify(window.fileCamera)===JSON.stringify(m.cameraView());window.fileDirections=m.fileParticles();m.fitTopology();
  })()`);
  assert.equal(await evaluate('window.fileCreationStable'),true,'new file markers do not move the camera');
  assert.deepEqual([...new Set((await evaluate('window.fileDirections')).map(p=>p.direction))].sort(),[-1,1]);
  assert.ok((await evaluate('window.fileDirections')).every(p=>p.color===0xffffff));
  assert.equal(await evaluate("document.querySelector('.file-monitor')"),null);
  await delay(150);
  const click=async field=>{
    const [x,y]=await evaluate(`import('/space.js').then(m=>m.fileVisuals()[0].${field})`);
    await cdp('Input.dispatchMouseEvent',{type:'mousePressed',x:(x*.5+.5)*1440,y:(-y*.5+.5)*1100,button:'left',clickCount:1});
    await cdp('Input.dispatchMouseEvent',{type:'mouseReleased',x:(x*.5+.5)*1440,y:(-y*.5+.5)*1100,button:'left',clickCount:1});
  };
  await click('screen');
  assert.equal(await evaluate("document.getElementById('connection-state').textContent"),'Regular file');
  assert.match(await evaluate("document.getElementById('connection-facts').textContent"),/READ: 200 bytes \/ 1 operations.*WRITE: 100 bytes \/ 1 operations/);
  assert.match(await evaluate("document.getElementById('connection-endpoints').textContent"),/\/tmp\/example.txt.*file-browser.*PID 910001/s);
  await evaluate("document.getElementById('close').click()");await click('pathScreen');
  assert.equal(await evaluate("document.getElementById('details').hidden"),false,'file path is selectable');
  const checks=await evaluate(`(async()=>{
    const m=await import('/space.js');
    const before=m.fileVisuals()[0],camera=m.cameraView();
    m.renderActivity({files:[{...window.fileEvent,write:false,path:'/tmp/renamed.txt',bytes:50}]});
    m.renderTopology(window.fileFixture);
    const stable=JSON.stringify(before.position)===JSON.stringify(m.fileVisuals()[0].position);
    const cameraStable=JSON.stringify(camera)===JSON.stringify(m.cameraView());
    const processStable=JSON.stringify(window.fileProcess)===JSON.stringify(m.processPosition('910001:1'));
    const renamed=document.getElementById('connection-label').textContent==='renamed.txt';
    document.getElementById('search').value='no-match';document.getElementById('search').dispatchEvent(new Event('input'));
    const filtered=m.fileVisuals().length===0;
    document.getElementById('search').value='';document.getElementById('search').dispatchEvent(new Event('input'));
    m.renderActivity({files:[{...window.fileEvent,resource:'file:8:1:43:0',path:null}],status:{files:'unavailable: test'}});
    m.selectFile(m.fileVisuals().find(f=>f.label==='file:8:1:43:0').id);
    const fallback=document.getElementById('connection-endpoints').textContent.includes('Path unavailable');
    m.pruneFiles(performance.now()+30001);
    const expired=m.fileVisuals().length===0&&document.getElementById('details').hidden;
    return {stable,cameraStable,processStable,renamed,filtered,fallback,expired};
  })()`);
  for(const [name,value] of Object.entries(checks))assert.equal(value,true,name);
  await evaluate(`import('/space.js').then(m=>{m.renderActivity({files:[{...window.fileEvent,write:true}],status:{files:'observing',files_lost:0}});m.selectFile(m.fileVisuals()[0].id);m.fitTopology();})`);
  await delay(100);
  const png=await cdp('Page.captureScreenshot',{format:'png'});await writeFile('target/browser-space-files.png',Buffer.from(png.data,'base64'));
  await evaluate("import('/space.js').then(m=>m.renderTopology({nodes:[],edges:[]}))");
  assert.equal(await evaluate("import('/space.js').then(m=>m.fileVisuals().length)"),0);
  assert.equal(await evaluate("document.getElementById('details').hidden"),true);
  console.log('File browser checks passed: picking, directions, totals, paths, stable positions/camera, filtering, expiry.');
}
