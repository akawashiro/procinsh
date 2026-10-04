import assert from 'node:assert/strict';
import {layoutMaps,edgeDirection,ipcParticlePlan,cpuGlowLevel,treeLayout,stableLayout} from '../dist/web/space-model.js';
const {AdaptiveRenderScale}=await import('../dist/web/space-model.js');
{
  const resolution=new AdaptiveRenderScale(1);
  const sample=(fps,count)=>{for(let i=0;i<count;i++)resolution.sample(fps);};
  for(let i=0;i<20;i++) { sample(15,2); sample(60,1); }
  assert.equal(resolution.scale,1,'transient low FPS never accumulates into a permanent quality drop');
  sample(15,3);
  assert.equal(resolution.scale,.9,'sustained low FPS reduces resolution gently');
  sample(24,10);
  assert.equal(resolution.scale,.9,'24 FPS is outside the low-FPS range');
  sample(60,4);
  assert.equal(resolution.scale,.9,'recovery waits for sustained healthy FPS');
  sample(45,1);
  assert.equal(resolution.scale,1,'healthy FPS restores initial sharpness without reloading');
  sample(15,100);
  assert.equal(resolution.scale,.5,'sustained load respects the lower bound');
  sample(60,100);
  assert.equal(resolution.scale,1,'resolution recovers fully even after reaching the floor');
  sample(15,3);
  for(let i=0;i<10;i++) { sample(60,4); sample(35,1); }
  assert.equal(resolution.scale,.9,'middle FPS holds resolution and interrupts recovery');
  sample(15,2);
  resolution.resetSampling();
  sample(15,1);
  assert.equal(resolution.scale,.9,'hidden tabs discard pending slow windows');
  sample(60,4);
  resolution.resetSampling();
  sample(60,1);
  assert.equal(resolution.scale,.9,'hidden tabs discard pending healthy windows');
  for(const dpr of [.4,.75,1,2,3]) {
    const bounded=new AdaptiveRenderScale(dpr);
    for(let i=0;i<100;i++)bounded.sample(10);
    assert.equal(bounded.scale,Math.min(dpr,.5));
    for(let i=0;i<100;i++)bounded.sample(60);
    assert.equal(bounded.scale,Math.min(dpr,1.5),'recovery respects the initial DPR cap');
  }
}
console.log('Adaptive resolution checks passed: transient dips, sustained load, recovery, hysteresis, visibility reset, and DPR bounds.');
const regions=layoutMaps([{start:'0xffffffffff600000',end:'0xffffffffff601000'},{start:'0x1000',end:'0x3000'},{start:'0x7fff00000000',end:'0x7fff00001000'}]);
assert.equal(regions[0].start,'0x1000');
assert.ok(regions[2].z > regions[1].z);
assert.ok(regions.every(region => region.h > 0));
assert.deepEqual(layoutMaps([]),[]);
const a={process_id:{pid:1,start_time_ticks:2},resource:{kind:'pipe',device:{major:0,minor:1},inode:'2'}},b={process_id:{pid:2,start_time_ticks:3},resource:{kind:'pipe',device:{major:0,minor:1},inode:'2'}};
const edge={endpoint:a,peer:b,shared:false};
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
const netEdge=(id,remote='203.0.113.1:443',pid=1,protocol='TCP')=>({id,endpoint:{process_id:{pid,start_time_ticks:1},resource:{kind:'socket',device:{major:0,minor:0},inode:String(Number(id)||1)},fd:Number(id)||1},peer:null,shared:false,socket:{protocol:{kind:protocol.startsWith('UDP')?'udp':'tcp',family:protocol.endsWith('6')?'ipv6':'ipv4'},state:{kind:'established'},remote:(()=>{const i=remote.lastIndexOf(':');return {ip:remote.slice(0,i).replace(/^\[|\]$/g,''),port:Number(remote.slice(i+1))}})(),local:{ip:'127.0.0.1',port:5000},network_peer:true}});
const connections=[netEdge('1'),netEdge('2'),netEdge('3','[2001:db8::1]:443'),netEdge('4','203.0.113.1:443',2),netEdge('5','203.0.113.1:443',1,'UDP')];
const groups=networkGroups(connections);
assert.equal(groups.size,4);
assert.equal([...groups.values()][0].members.length,2);
assert.match([...groups.values()][1].label,/\[2001:db8::1\]:443/);
assert.equal(networkGroups([{...connections[0],shared:true},{...connections[0],peer:connections[1].endpoint},{...connections[0],socket:null},{...connections[0],socket:{...connections[0].socket,network_peer:false}}]).size,0);
const owners=new Map([['1:1',{x:0,y:0}],['2:1',{x:0,y:0}]]);
const netLayout=networkLayout(groups,owners);
assert.equal(new Set([...netLayout.values()].map(p=>JSON.stringify(p))).size,4);
const grownGroups=networkGroups([...connections,netEdge('6','203.0.113.2:443')]);
const netGrown=networkLayout(grownGroups,owners,netLayout);
for(const [id,p] of netLayout)assert.deepEqual(netGrown.get(id),p);
assert.deepEqual(networkLayout(new Map(),owners,netGrown),new Map());
assert.deepEqual(networkLayout(networkGroups([...connections].reverse()),owners),netLayout);
assert.equal(connectionState({...connections[0],socket:{protocol:{kind:'tcp',family:'ipv4'},state:{kind:'listen'},network_peer:false}}),'Listening');
assert.equal(connectionState({...connections[0],socket:{protocol:{kind:'udp',family:'ipv4'},state:{kind:'unconnected'},network_peer:false}}),'No destination set');
assert.equal(connectionState(connections[0]),'Network destination');
assert.equal(edgeDirection(connections[0],{...connections[0].endpoint,write:true}),1);
assert.equal(edgeDirection(connections[0],{...connections[0].endpoint,write:false}),-1);
console.log('Network model checks passed: grouping, IPv6, classification, stable placement, and direction.');

const {remoteLabel}=await import('../dist/web/space-model.js');
assert.equal(remoteLabel({remote:{ip:'2001:db8::1',port:443},remote_hostname:'example.test'}),'example.test:443');
assert.equal(remoteLabel({remote:{ip:'192.0.2.1',port:80}}),'192.0.2.1:80');

const {processColors}=await import('../dist/web/space-model.js');
assert.equal(processColors({uid:1000,euid:1000}).real,processColors({uid:1000,euid:1000}).effective);
assert.notEqual(processColors({uid:1000,euid:0}).real,processColors({uid:1000,euid:0}).effective);
assert.equal(processColors({}).real,'#889299');

const {RecentFiles,fileKey,fileLayout}=await import('../dist/web/space-model.js');
{
  const files=new RecentFiles(),owner={pid:1,start_time_ticks:1},live=new Set(['1:1']);
  const event={process_id:owner,file:{device:{major:8,minor:1},inode:'42',generation:0},path:'/tmp/example',write:false,bytes:7,count:1};
  assert.equal(files.ingest([event,{...event,write:true,bytes:11}],100,live),true);
  const id=fileKey(event),file=files.entries.get(id);
  assert.equal(file.readBytes,7);assert.equal(file.writeBytes,11);assert.equal(file.label,'example');
  const positions=new Map([['1:1',{x:0,y:0}]]),before=fileLayout(files.entries,positions);
  files.ingest([{...event,path:'/tmp/renamed'}],200,live);
  assert.deepEqual(fileLayout(files.entries,positions,before),before,'rename preserves placement');
  assert.equal(file.label,'renamed');
  files.ingest([{...event,process_id:{pid:1,start_time_ticks:2}},{...event,bytes:0}],300,live);
  assert.equal(files.entries.size,1,'stale process and empty events ignored');
  for(let i=0;i<35;i++)files.ingest([{...event,file:{device:{major:8,minor:1},inode:String(i+1000),generation:0},path:null}],400+i,live);
  assert.equal(files.entries.size,32);assert.equal(files.evicted,4);
  const stable=fileLayout(files.entries,positions,before);
  assert.equal(new Set([...stable.values()].map(p=>JSON.stringify(p))).size,32,'markers never overlap');
  assert.ok([...stable.values()].every(p=>p.z<0),'files occupy separate space below the process');
  assert.equal(files.prune(30433,live),true);assert.equal(files.entries.size,1);
  assert.equal(files.prune(30434,live),true);assert.equal(files.entries.size,0);
  for(let p=1;p<=20;p++){
    live.add(`${p}:1`);
    for(let i=0;i<32;i++)files.ingest([{...event,process_id:{pid:p,start_time_ticks:1},file:{device:{major:8,minor:1},inode:String(i+1000),generation:0}}],40000,live);
  }
  assert.equal(files.entries.size,512,'global display limit enforced');
  files.prune(40001,new Set(['20:1']));assert.equal(files.entries.size,32,'removed processes lose file markers');
  files.clear();assert.equal(files.entries.size,0);assert.equal(files.evicted,0);
}
console.log('File model checks passed: aggregation, direction, stale identity, stable placement, TTL, per-process and global limits.');
// Identity equality is field based and large inode strings remain distinct.
{
  const resource={kind:'pipe',device:{major:8,minor:1},inode:'18446744073709551615'};
  const endpoint={process_id:{pid:1,start_time_ticks:2},resource};
  assert.equal(edgeDirection({endpoint,peer:null,shared:false},{...endpoint,resource:{inode:resource.inode,device:{minor:1,major:8},kind:'pipe'},write:true}),1);
  assert.equal(edgeDirection({endpoint,peer:null,shared:false},{...endpoint,resource:{...resource,inode:'18446744073709551614'},write:true}),null);
  const base={process_id:endpoint.process_id,file:{device:resource.device,inode:resource.inode,generation:0}};
  for(const file of [{...base.file,inode:'18446744073709551614'},{...base.file,generation:1},{...base.file,device:{major:9,minor:1}},{...base.file,device:{major:8,minor:2}}])assert.notEqual(fileKey({...base,file}),fileKey(base));
}

assert.equal(Display.state({kind:'unknown_inet',code:255}),'FF');
assert.equal(Display.protocol({kind:'unix',socket_type:{kind:'seqpacket'}}),'UNIX SEQPACKET');
assert.equal(Display.access('unknown'),'N/A');

assert.equal(Display.affinity([{start:0,end:3},{start:8,end:8}]),'0-3,8');
assert.equal(Display.affinity(null),'N/A');
assert.equal(Display.scheduler({kind:'unknown',code:99}),'UNKNOWN (99)');

assert.equal(Display.mapping({pathname:'/tmp/a [b]',readable:true,writable:true,executable:false,private:true}),'/tmp/a [b] [rw-p]');
assert.equal(Display.mapping({pathname:null,readable:true,writable:false,executable:false,private:false}),'[anonymous] [r--s]');

const {mergeSnapshot}=await import('../dist/web/space-model.js');
const original={identity:{pid:1,start_time_ticks:10},maps:[{start:'0x1000',end:'0x2000'}],maps_epoch:10,maps_error:null};
const base={processes:[original],fd_relations:[],sequence:1};
const delta=extra=>({kind:"delta",sequence:2,base_sequence:1,fd_relations:[],...extra});
const omitted={identity:original.identity,maps_epoch:20,maps_error:'read failed'};
const retained=mergeSnapshot(base,delta({processes:[omitted]}));
assert.deepEqual(retained.processes[0].maps,original.maps);
assert.equal(retained.processes[0].maps_epoch,10);
assert.equal(retained.processes[0].maps_error,'read failed');
assert.deepEqual(mergeSnapshot(base,delta({processes:[{...omitted,maps:[]}]})).processes[0].maps,[]);
assert.deepEqual(mergeSnapshot(base,delta({processes:[]})).processes,[]);
assert.deepEqual(mergeSnapshot(base,delta({processes:[{...omitted,identity:{pid:1,start_time_ticks:11}}]})).processes[0].maps,[]);
assert.equal(mergeSnapshot(retained,{...base,kind:"full"}).processes[0].maps_epoch,10);

const changedMap={start:'0x1000',end:'0x1800',writable:true};
const added={start:'0x1800',end:'0x2000'};
const split=mergeSnapshot(base,delta({processes:[{...omitted,maps_delta:{upsert:[added,changedMap],remove:[]}}]}));
assert.deepEqual(split.processes[0].maps,[changedMap,added]);
assert.equal(split.processes[0].maps_epoch,20);
const joined=mergeSnapshot(split,{kind:'delta',sequence:3,base_sequence:2,processes:[{...omitted,maps_delta:{upsert:[original.maps[0]],remove:['0x1800']}}],fd_relations_delta:{upsert:[{id:'new',label:'pipe'}],remove:[]}});
assert.deepEqual(joined.processes[0].maps,original.maps);
assert.deepEqual(joined.fd_relations,[{id:'new',label:'pipe'}]);
const removed=mergeSnapshot(joined,{kind:'delta',sequence:4,base_sequence:3,processes:[],fd_relations_delta:{upsert:[],remove:['new']}});
assert.deepEqual(removed.fd_relations,[]);
assert.throws(()=>mergeSnapshot(base,{kind:'delta',sequence:9,base_sequence:8,processes:[]}),/baseline mismatch/);
assert.throws(()=>mergeSnapshot(base,delta({processes:[{...omitted,identity:{pid:1,start_time_ticks:99},maps_delta:{upsert:[],remove:[]}}]})),/Missing process maps baseline/);
assert.deepEqual(mergeSnapshot(joined,{kind:'full',sequence:10,processes:[],fd_relations:[]}).fd_relations,[]);
console.log('Snapshot delta checks passed: split/merge, permissions, FD additions/removal, epoch, baseline mismatch, full reset.');
