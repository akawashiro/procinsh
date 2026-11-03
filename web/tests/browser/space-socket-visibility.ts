import assert from "node:assert/strict";

import type { Evaluate } from "./harness.js";

export async function checkSocketVisibility(evaluate: Evaluate) {
  const initial = await evaluate<{
    raw: number;
    shown: string[];
    particles: { connectionId?: string; socketOwnerKey?: string }[];
  }>(`(async()=>{
    const m=await import(window.spaceTestModule);
    const node={identity:{pid:900010,start_time_ticks:1},name:'socket-visibility',uid:1000,maps:[]};
    const peer={...node,identity:{pid:900011,start_time_ticks:1}};
    const edge=(id,status)=>({id,endpoint:{process_id:node.identity,fd:10,fd_count:1,resource:{kind:'socket',device:{major:0,minor:10},inode:id},kind:'socket',access:'read_write',connection_status:status},peer:null,label:id,shared:false,candidate:false,socket:null});
    const listen=edge('listen','listening'),unconnected=edge('unconnected','unconnected'),unknown=edge('unknown','unknown'),known=edge('known','peer_present');
    known.socket={protocol:{kind:'tcp',family:'ipv4'},state:{kind:'established'},local:{ip:'127.0.0.1',port:12345},remote:{ip:'203.0.113.10',port:443},path:null,network_peer:true,remote_hostname:null};
    const shared={...listen,id:'shared',shared:true,peer:{...listen.endpoint,process_id:peer.identity}};
    const pipe={...edge('pipe',null),endpoint:{...edge('pipe',null).endpoint,kind:'pipe',resource:{kind:'pipe',device:{major:0,minor:10},inode:'pipe'}}};
    window.socketVisibilityFixture={processes:[node,peer],fd_relations:[listen,unconnected,shared,unknown,{...unknown,id:'unknown-duplicate'},known,pipe]};
    m.renderSystemSnapshot(window.socketVisibilityFixture,true);
    m.renderActivity({window_ms:100,status:{},ipc:[listen,unconnected,unknown,known].map(e=>({process_id:e.endpoint.process_id,resource:e.endpoint.resource,write:true,bytes:128,count:1}))});
    return m.socketVisibilityState();
  })()`);
  assert.equal(initial.raw, 7, "raw socket observations are retained");
  assert.deepEqual(
    [...initial.shown].sort(),
    ["known", "pipe", "unknown", "unknown-duplicate"],
    "listeners, unconnected sockets and shared listener lines are excluded",
  );
  assert.ok(initial.particles.some((p) => p.connectionId === "known"));
  assert.ok(
    initial.particles.some(
      (p) => !p.connectionId && p.socketOwnerKey?.includes("unknown"),
    ),
    "unknown ambiguous traffic still has a process port animation",
  );
  assert.ok(
    initial.particles.every(
      (p) =>
        !p.socketOwnerKey?.includes("unconnected") &&
        !p.socketOwnerKey?.includes("listen"),
    ),
  );

  const transitioned = await evaluate<{
    selected: string | null;
    shown: string[];
    particles: unknown[];
    networkCount: number;
    detailsHidden: boolean;
  }>(`(async()=>{
    const m=await import(window.spaceTestModule),f=window.socketVisibilityFixture;
    m.selectConnection('known');
    for(const e of f.fd_relations)if(['known','unknown','unknown-duplicate'].includes(e.id))e.endpoint.connection_status='unconnected';
    m.renderSystemSnapshot(f);
    m.renderActivity({window_ms:100,status:{},ipc:f.fd_relations.filter(e=>e.id!=='pipe').map(e=>({process_id:e.endpoint.process_id,resource:e.endpoint.resource,write:true,bytes:128,count:1}))});
    return {...m.socketVisibilityState(),networkCount:m.networkVisuals().length,detailsHidden:document.getElementById('details').hidden};
  })()`);
  assert.equal(transitioned.selected, null, "hidden sockets lose selection");
  assert.deepEqual(transitioned.shown, ["pipe"]);
  assert.deepEqual(
    transitioned.particles,
    [],
    "connection and port particles disappear immediately",
  );
  assert.equal(
    transitioned.networkCount,
    0,
    "stale remote addresses do not retain a marker",
  );
  assert.equal(transitioned.detailsHidden, true);

  const restored = await evaluate<boolean>(`(async()=>{
    const m=await import(window.spaceTestModule),f=window.socketVisibilityFixture;
    f.fd_relations.find(e=>e.id==='known').endpoint.connection_status='peer_present';
    m.renderSystemSnapshot(f);
    return !!m.connectionVisual('known') && m.networkVisuals().length===1;
  })()`);
  assert.equal(restored, true, "a peer-present socket returns to the scene");
  console.log(
    "Socket visibility browser checks passed: hidden/shared lines, raw observations, IPC/port particles, selection, state transitions and restoration.",
  );
}
