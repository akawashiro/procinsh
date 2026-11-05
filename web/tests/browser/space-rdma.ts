import type { Evaluate, Cdp } from "./harness.js";

import assert from "node:assert/strict";

type RdmaVisuals = {
  groups: {
    id: string;
    label: string;
    anchor: string;
    screen: number[];
    start: number[];
  }[];
  links: { id: string; screen: number[] }[];
  devices: { id: string; label: string; screen: number[] }[];
};

export async function checkRdmaSpace(evaluate: Evaluate, cdp: Cdp) {
  await evaluate(`(async()=>{
    const m=await import(window.spaceTestModule);
    // Finish the preceding fixture's activity before replacing its snapshot.
    document.getElementById('reset').click();
    const node={identity:{pid:900101,start_time_ticks:1},name:'rdma-browser',uid:1000,euid:1000,maps:[]};
    const endpoint=(qpn,owner)=>({owner,device:'ib0',port:1,qpn,qp_type:{kind:'rc'},state:{kind:'rts'}});
    const user={kind:'process',process_id:node.identity};
    const relation=(qpn,owner,extra)=>({id:'rdma:ib0/1:'+qpn,endpoint:endpoint(qpn,owner),peer:null,remote_qpn:qpn+100,cm:null,remote_gid:null,remote_hostname:null,label:'RDMA RC QP '+qpn,...extra});
    window.rdmaFixture={processes:[node],fd_relations:[],
      rdma_ports:[{device:'ib0',port:1,link_layer:'InfiniBand',state:'ACTIVE',lid:2,gid:'fe80:0000:0000:0000:0002:c903:0028:45c7',rate:'40 Gb/sec (4X QDR)'}],
      rdma_relations:[
        relation(524,{kind:'kernel',name:'rdma_cm'},{remote_gid:'fe80:0000:0000:0000:f452:1403:0080:de51',cm:{id:0,owner:{kind:'kernel',name:'rpcrdma'},state:{kind:'connect'},port_space:{kind:'tcp'},source:{kind:'inet',ip:'10.10.10.2',port:58985},destination:{kind:'inet',ip:'10.10.10.1',port:20049}}}),
        relation(1,{kind:'kernel',name:'ib_core'},{endpoint:{...endpoint(1,{kind:'kernel',name:'ib_core'}),qp_type:{kind:'gsi'}},remote_qpn:null}),
        relation(30,user),
        {...relation(10,user),id:'rdma:ib0/1:10/rdma:ib0/1:20',peer:endpoint(20,{kind:'kernel',name:'ib_isert'}),remote_qpn:20,label:'RDMA RC QP 10 ↔ QP 20'},
      ]};
    m.renderSystemSnapshot(window.rdmaFixture,true);m.fitScene();
  })()`);
  // Fitting changes the camera before the renderer updates its world matrix.
  // Wait for rendered frames before projecting coordinates used for picking.
  await evaluate(
    "new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))",
  );
  assert.deepEqual(
    await evaluate(
      "import(window.spaceTestModule).then(m=>m.networkVisuals())",
    ),
    [],
    "the RDMA fixture has no retained network destinations",
  );
  const visuals = await evaluate<RdmaVisuals>(
    "import(window.spaceTestModule).then(m=>m.rdmaVisuals())",
  );
  assert.deepEqual(
    visuals.groups.map((g) => g.label).sort(),
    ["RDMA RC ×1", "rpcrdma · RDMA RC 10.10.10.1:20049 ×1"].sort(),
    "management QPs are hidden; kernel QPs show their CM consumer",
  );
  assert.deepEqual(
    visuals.devices.map((d) => d.label),
    ["ib0"],
  );
  assert.deepEqual(
    visuals.links.map((l) => l.id),
    ["rdma:ib0/1:10/rdma:ib0/1:20"],
  );
  const process = visuals.groups.find((g) => g.anchor === "900101:1")!;
  assert.equal(process.start[2], 8, "process QPs leave from the hull roof");
  const click = async (projection: string) => {
    const [x, y] = await evaluate<[number, number]>(
      `import(window.spaceTestModule).then(m=>${projection})`,
    );
    for (const type of ["mousePressed", "mouseReleased"] as const)
      await cdp("Input.dispatchMouseEvent", {
        type,
        x: (x * 0.5 + 0.5) * 1440,
        y: (-y * 0.5 + 0.5) * 1100,
        button: "left",
        clickCount: 1,
      });
  };
  const text = (id: string) =>
    evaluate<string>(`document.getElementById('${id}').textContent`);
  await click("m.rdmaVisuals().groups.find(g=>g.anchor!=='900101:1').screen");
  assert.equal(
    await text("connection-state"),
    "RDMA queue pairs to a remote destination",
  );
  assert.match(
    await text("connection-endpoints"),
    /Remote QP 624 · GID fe80:0000:0000:0000:f452:1403:0080:de51/,
  );
  await click("m.rdmaVisuals().devices[0].screen");
  assert.equal(await text("connection-state"), "RDMA device");
  assert.match(await text("connection-endpoints"), /Port 1 · ACTIVE/);
  await click("m.rdmaVisuals().links[0].screen");
  assert.equal(
    await text("connection-state"),
    "Connected queue pairs on this host",
  );
  assert.match(await text("connection-endpoints"), /Kernel · ib_isert/);
  await evaluate("document.getElementById('close').click()");
  assert.equal(
    await evaluate(`import(window.spaceTestModule).then(m=>{
      m.renderSystemSnapshot({processes:[],fd_relations:[]},true);
      const v=m.rdmaVisuals();
      return v.groups.length+v.links.length+v.devices.length;
    })`),
    0,
    "snapshots without RDMA fields clear the RDMA view",
  );
}
