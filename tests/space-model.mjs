import assert from 'node:assert/strict';
import {layoutMaps,edgeDirection,ipcParticlePlan,cpuGlowLevel,treeLayout,stableLayout} from '../dist/web/space-model.js';
const regions=layoutMaps([{start:'0xffffffffff600000',end:'0xffffffffff601000'},{start:'0x1000',end:'0x3000'},{start:'0x7fff00000000',end:'0x7fff00001000'}]);
assert.equal(regions[0].start,'0x1000');
assert.ok(regions[2].z > regions[1].z);
assert.ok(regions.every(region => region.h > 0));
assert.deepEqual(layoutMaps([]),[]);
const a={process_id:{pid:1,start_time_ticks:2},resource:'pipe:1:2'},b={process_id:{pid:2,start_time_ticks:3},resource:'pipe:1:2'};
const edge={a,b,shared:false};
assert.equal(edgeDirection(edge,{...a,write:true}),1);
assert.equal(edgeDirection(edge,{...b,write:false}),1);
assert.equal(edgeDirection({...edge,shared:true},{...a,write:true}),null);
assert.equal(edgeDirection(edge,{...a,process_id:{pid:1,start_time_ticks:999},write:true}),null);
assert.deepEqual(ipcParticlePlan(1),{duration:2000,offsets:[0,100]});
assert.deepEqual(ipcParticlePlan(2).offsets,[0,100,200]);
assert.equal(ipcParticlePlan(4).offsets.length,4);
assert.equal(ipcParticlePlan(8).offsets.length,5);
assert.equal(ipcParticlePlan(16).offsets.length,6);
assert.equal(ipcParticlePlan(1_000_000).offsets.length,6);
assert.deepEqual(ipcParticlePlan(16,true),{duration:150,offsets:[0]});
const active={last:1000,window_ms:100,runtime_ns:20_000_000,running_threads:1};
assert.ok(cpuGlowLevel(active,1000)>.9,'running process is bright');
assert.ok(cpuGlowLevel({...active,running_threads:0},1000)>.5,'recent runtime is visible');
assert.ok(cpuGlowLevel(active,1250)<cpuGlowLevel(active,1000),'afterglow fades');
assert.equal(cpuGlowLevel(active,1500),0,'afterglow ends after 500ms');
assert.equal(cpuGlowLevel(active,999),0,'future timestamps are rejected');
assert.equal(cpuGlowLevel(null,1000),0);
const processNode=(pid,parent=null)=>({identity:{pid,start_time_ticks:pid*10},parent_id:parent&&{pid:parent,start_time_ticks:parent*10}});
const tree=treeLayout([processNode(1),processNode(2,1),processNode(3,1),processNode(4,2),processNode(8,99)]);
assert.equal(tree.get('1:10').y,0);
assert.ok(tree.get('2:20').y>tree.get('1:10').y);
assert.ok(tree.get('4:40').y>tree.get('2:20').y);
assert.equal(tree.get('1:10').x,(tree.get('2:20').x+tree.get('3:30').x)/2);
assert.equal(tree.get('8:80').y,0,'missing parent becomes a root');
const cyclic=treeLayout([
  {identity:{pid:10,start_time_ticks:1},parent_id:{pid:11,start_time_ticks:1}},
  {identity:{pid:11,start_time_ticks:1},parent_id:{pid:10,start_time_ticks:1}},
]);
assert.equal(cyclic.size,2);assert.ok([...cyclic.values()].some(v=>v.parent===null));
assert.deepEqual([...treeLayout([processNode(3,1),processNode(1),processNode(2,1)]).entries()],
  [...treeLayout([processNode(1),processNode(2,1),processNode(3,1)]).entries()]);
const fanout=treeLayout([processNode(1),...Array.from({length:400},(_,i)=>processNode(i+2,1))]);
const fanoutX=[...fanout.values()].map(v=>v.x),fanoutY=[...fanout.values()].map(v=>v.y);
assert.ok(Math.max(...fanoutX)-Math.min(...fanoutX)<400,'large sibling groups wrap instead of becoming one long row');
assert.ok(Math.max(...fanoutY)>20,'wrapped sibling groups use the plane');
console.log('Space model checks passed: compressed addresses, edges, IPC particles, PID reuse, CPU glow, and deterministic process trees.');

const originalNodes=[processNode(1),processNode(2,1),processNode(3,1)];
const anchored=stableLayout(originalNodes);
assert.deepEqual(anchored,treeLayout(originalNodes));
const coordinates=layout=>[...layout].map(([id,p])=>[id,{x:p.x,y:p.y}]).sort();
assert.deepEqual(coordinates(stableLayout([...originalNodes].reverse(),anchored)),coordinates(anchored));
const grownNodes=[...originalNodes,processNode(4,2),processNode(5,1),processNode(6)];
const grown=stableLayout(grownNodes,anchored);
for(const [id,p] of anchored) assert.deepEqual(grown.get(id),p,'existing nodes stay anchored');
assert.deepEqual(coordinates(stableLayout([...grownNodes].reverse(),anchored)),coordinates(grown));
const changed=stableLayout([processNode(2,99),processNode(3,2),processNode(4,2)],grown);
for(const [id,p] of changed) assert.deepEqual({x:p.x,y:p.y},{x:grown.get(id).x,y:grown.get(id).y});
assert.equal(changed.has('1:10'),false,'removed identities are released');
const reused=stableLayout([processNode(1),processNode(2,1),{identity:{pid:3,start_time_ticks:999},parent_id:processNode(1).identity}],anchored);
assert.equal(reused.has('3:30'),false);
assert.ok(reused.has('3:999'),'PID reuse is a new identity');
const vacant=stableLayout(originalNodes.filter(n=>n.identity.pid!==5),grown);
const refilled=stableLayout([...originalNodes,processNode(7,1)],vacant);
assert.deepEqual({x:refilled.get('7:70').x,y:refilled.get('7:70').y},{x:grown.get('5:50').x,y:grown.get('5:50').y},'vacated child position is reusable');
const large=stableLayout([processNode(1),...Array.from({length:1000},(_,i)=>processNode(i+2,1))],stableLayout([processNode(1)]));
const places=[...large.values()];
for(let i=0;i<places.length;i++) for(let j=i+1;j<places.length;j++) {
  assert.ok(Math.abs(places[i].x-places[j].x)>=4.8-1e-9 || Math.abs(places[i].y-places[j].y)>=6.5-1e-9,'new placements do not overlap');
}
assert.deepEqual(stableLayout([],grown),new Map());
const stableCycle=stableLayout([processNode(1,2),processNode(2,1),processNode(9)],anchored);
assert.equal(stableCycle.size,3);
assert.ok([...stableCycle.values()].every(p=>Number.isFinite(p.x)&&Number.isFinite(p.y)));
console.log('Stable layout checks passed: additions, exits, reparenting, PID reuse, vacant slots, cycles, and 1000 newcomers.');

const {networkGroups,networkLayout,connectionState}=await import('../dist/web/space-model.js');
const netEdge=(id,remote='203.0.113.1:443',pid=1,protocol='TCP')=>({id,a:{process_id:{pid,start_time_ticks:1},resource:`socket:${id}`,fd:Number(id)||1},b:null,shared:false,socket:{protocol,state:'ESTABLISHED',remote,local:'127.0.0.1:5000',network_peer:true}});
const connections=[netEdge('1'),netEdge('2'),netEdge('3','[2001:db8::1]:443'),netEdge('4','203.0.113.1:443',2),netEdge('5','203.0.113.1:443',1,'UDP')];
const groups=networkGroups(connections);
assert.equal(groups.size,4);
assert.equal([...groups.values()][0].members.length,2);
assert.match([...groups.values()][1].label,/\[2001:db8::1\]:443/);
assert.equal(networkGroups([{...connections[0],shared:true},{...connections[0],b:connections[1].a},{...connections[0],socket:null},{...connections[0],socket:{...connections[0].socket,network_peer:false}}]).size,0);
const owners=new Map([['1:1',{x:0,y:0}],['2:1',{x:0,y:0}]]);
const netLayout=networkLayout(groups,owners);
assert.equal(new Set([...netLayout.values()].map(p=>JSON.stringify(p))).size,4);
const grownGroups=networkGroups([...connections,netEdge('6','203.0.113.2:443')]);
const netGrown=networkLayout(grownGroups,owners,netLayout);
for(const [id,p] of netLayout)assert.deepEqual(netGrown.get(id),p);
assert.deepEqual(networkLayout(new Map(),owners,netGrown),new Map());
assert.deepEqual(networkLayout(networkGroups([...connections].reverse()),owners),netLayout);
assert.equal(connectionState({...connections[0],socket:{protocol:'TCP',state:'LISTEN',network_peer:false}}),'Listening');
assert.equal(connectionState({...connections[0],socket:{protocol:'UDP',network_peer:false}}),'No destination set');
assert.equal(connectionState(connections[0]),'Network destination');
assert.equal(edgeDirection(connections[0],{...connections[0].a,write:true}),1);
assert.equal(edgeDirection(connections[0],{...connections[0].a,write:false}),-1);
console.log('Network model checks passed: grouping, IPv6, classification, stable placement, and direction.');

const {remoteLabel}=await import('../dist/web/space-model.js');
assert.equal(remoteLabel({remote:'[2001:db8::1]:443',remote_hostname:'example.test'}),'example.test:443');
assert.equal(remoteLabel({remote:'192.0.2.1:80'}),'192.0.2.1:80');

const {processColors}=await import('../dist/web/space-model.js');
assert.equal(processColors({uid:1000,euid:1000}).real,processColors({uid:1000,euid:1000}).effective);
assert.notEqual(processColors({uid:1000,euid:0}).real,processColors({uid:1000,euid:0}).effective);
assert.equal(processColors({}).real,'#889299');

const {RecentFiles,fileKey,fileLayout}=await import('../dist/web/space-model.js');
{
  const files=new RecentFiles(),owner={pid:1,start_time_ticks:1},live=new Set(['1:1']);
  const event={process_id:owner,resource:'file:8:1:42:0',path:'/tmp/example',write:false,bytes:7,count:1};
  assert.equal(files.ingest([event,{...event,write:true,bytes:11}],100,live),true);
  const id=fileKey(event),file=files.entries.get(id);
  assert.equal(file.readBytes,7);assert.equal(file.writeBytes,11);assert.equal(file.label,'example');
  const positions=new Map([['1:1',{x:0,y:0}]]),before=fileLayout(files.entries,positions);
  files.ingest([{...event,path:'/tmp/renamed'}],200,live);
  assert.deepEqual(fileLayout(files.entries,positions,before),before,'rename preserves placement');
  assert.equal(file.label,'renamed');
  files.ingest([{...event,process_id:{pid:1,start_time_ticks:2}},{...event,bytes:0}],300,live);
  assert.equal(files.entries.size,1,'stale process and empty events ignored');
  for(let i=0;i<35;i++)files.ingest([{...event,resource:'file:'+i,path:null}],400+i,live);
  assert.equal(files.entries.size,32);assert.equal(files.evicted,4);
  const stable=fileLayout(files.entries,positions,before);
  assert.equal(new Set([...stable.values()].map(p=>JSON.stringify(p))).size,32,'markers never overlap');
  assert.ok([...stable.values()].every(p=>p.z<0),'files occupy separate space below the process');
  assert.equal(files.prune(30433,live),true);assert.equal(files.entries.size,1);
  assert.equal(files.prune(30434,live),true);assert.equal(files.entries.size,0);
  for(let p=1;p<=20;p++){
    live.add(`${p}:1`);
    for(let i=0;i<32;i++)files.ingest([{...event,process_id:{pid:p,start_time_ticks:1},resource:'file:'+i}],40000,live);
  }
  assert.equal(files.entries.size,512,'global display limit enforced');
  files.prune(40001,new Set(['20:1']));assert.equal(files.entries.size,32,'removed processes lose file markers');
  files.clear();assert.equal(files.entries.size,0);assert.equal(files.evicted,0);
}
console.log('File model checks passed: aggregation, direction, stale identity, stable placement, TTL, per-process and global limits.');
