import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';

export async function checkNetworkSpace(evaluate, delay, cdp) {
  await evaluate(`(async()=>{
    const m=await import('/space.js');
    const node={identity:{pid:900001,start_time_ticks:1},name:'network-browser',uid:1000,euid:0,rss_bytes:4096,maps:[]};
    const edge=(id,remote,fd)=>({id,a:{process_id:node.identity,fd,fd_count:1,resource:'socket:'+id,access:2},b:null,label:'TCP '+remote,shared:false,candidate:false,socket:{protocol:'TCP',state:'ESTABLISHED',local:'127.0.0.1:'+fd,remote,remote_hostname:remote.startsWith('203.')?'example.test':null,network_peer:true}});
    const edges=[edge('net-a','203.0.113.10:443',40),edge('net-b','203.0.113.10:443',41),edge('net-v6','[2001:db8::1]:443',42)];
    edges.push({...edge('listen','0.0.0.0:0',43),socket:{protocol:'TCP',state:'LISTEN',local:'0.0.0.0:8080',remote:'0.0.0.0:0',network_peer:false}});
    window.networkFixture={nodes:[node],edges};m.renderTopology(window.networkFixture,true);m.fitTopology();
  })()`);
  await delay(100);
  const groups=await evaluate("import('/space.js').then(m=>m.networkVisuals())");
  assert.equal(groups.length,2,'only connected network destinations have markers');
  assert.ok(groups.some(g=>g.label.includes('example.test:443')),'resolved name is used in the label');
  assert.ok(groups.some(g=>g.label.includes('×2')),'same destination is aggregated');
  assert.deepEqual(await evaluate("import('/space.js').then(m=>m.networkParticles())"),[],'idle connections have no particles');
  const clickProjected=async field=>{
    const [x,y]=await evaluate(`import('/space.js').then(m=>m.networkVisuals().find(g=>g.members.includes('net-a')).${field})`);
    await cdp('Input.dispatchMouseEvent',{type:'mousePressed',x:(x*.5+.5)*1440,y:(-y*.5+.5)*1100,button:'left',clickCount:1});
    await cdp('Input.dispatchMouseEvent',{type:'mouseReleased',x:(x*.5+.5)*1440,y:(-y*.5+.5)*1100,button:'left',clickCount:1});
  };
  await clickProjected('screen');
  assert.equal(await evaluate("document.getElementById('connection-state').textContent"),'Network destination');
  assert.match(await evaluate("document.getElementById('connection-endpoints').textContent"),/FD 40[\s\S]*FD 41/);
  await evaluate("document.querySelector('#connection-endpoints button').click()");
  assert.match(await evaluate("document.getElementById('connection-endpoints').textContent"),/FD 40/);
  await evaluate("Array.from(document.querySelectorAll('#connection-endpoints button')).find(b=>b.textContent==='Show all connections to this destination').click()");
  await evaluate("document.getElementById('close').click()");
  await clickProjected('pathScreen');
  assert.equal(await evaluate("document.getElementById('details').hidden"),false,'path selects group');
  const animated=await evaluate(`(async()=>{
    const m=await import('/space.js'),f=window.networkFixture;
    m.renderActivity({window_ms:100,ipc:[{process_id:f.nodes[0].identity,resource:'socket:net-a',write:true,bytes:100,count:1},{process_id:f.nodes[0].identity,resource:'socket:net-b',write:false,bytes:200,count:1}]});
    return {particles:m.networkParticles(),facts:document.getElementById('connection-facts').textContent};
  })()`);
  assert.deepEqual([...new Set(animated.particles.map(p=>p.direction))].sort(),[-1,1]);
  assert.match(animated.facts,/300 bytes \/ 2 operations/);
  const checks=await evaluate(`(async()=>{
    const m=await import('/space.js'),f=window.networkFixture;
    const before=m.networkVisuals().map(g=>[g.id,g.position]);
    for(const e of f.edges)if(e.socket.remote_hostname)e.socket.remote_hostname='renamed.example.test';
    m.renderTopology({...f,edges:[...f.edges].reverse()});
    const stable=before.every(([id,p])=>JSON.stringify(m.networkVisuals().find(g=>g.id===id).position)===JSON.stringify(p));
    document.getElementById('search').value='no-match';document.getElementById('search').dispatchEvent(new Event('input'));
    const filtered=m.networkVisuals().length===0;
    document.getElementById('search').value='network-browser';document.getElementById('search').dispatchEvent(new Event('input'));
    document.getElementById('rearrange').click();
    const rearranged=m.networkVisuals().length===2&&!document.getElementById('details').hidden&&m.networkParticles().length===0;
    m.renderTopology({...f,edges:f.edges.filter(e=>e.id!=='net-a')});
    const kept=!document.getElementById('details').hidden&&document.getElementById('connection-label').textContent.includes('×1');
    m.renderTopology({...f,edges:[]});
    const removed=document.getElementById('details').hidden&&m.networkVisuals().length===0;
    m.renderTopology(f);m.selectConnection('listen');
    const listening=document.getElementById('connection-state').textContent==='Listening';
    document.getElementById('reset').click();
    return {stable,filtered,rearranged,kept,removed,listening};
  })()`);
  for(const [name,passed] of Object.entries(checks))assert.equal(passed,true,name);
  await evaluate("import('/space.js').then(m=>{m.selectNetwork(m.networkVisuals()[0].id);m.fitTopology();})");
  await delay(100);
  const screenshot=await cdp('Page.captureScreenshot',{format:'png'});
  await writeFile('target/browser-space-network.png',Buffer.from(screenshot.data,'base64'));
  await cdp('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true});
  await delay(100);
  assert.equal(await evaluate('document.documentElement.scrollWidth<=390'),true,'network details fit mobile');
  assert.equal(await evaluate("document.getElementById('connection-endpoints').scrollWidth<=document.getElementById('connection-endpoints').clientWidth"),true,'addresses wrap in mobile details');
  await cdp('Emulation.setDeviceMetricsOverride',{width:1440,height:1100,deviceScaleFactor:1,mobile:false});
  await evaluate("document.getElementById('reset').click()");
  console.log('Network browser checks passed: marker/path picking, grouping, FD details, directions, stats, stable positions, search, rearrange, and removal.');
}
