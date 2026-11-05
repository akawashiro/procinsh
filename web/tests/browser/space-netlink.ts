import type { Evaluate, Cdp } from "./harness.js";

import { delay as pause, repositoryPath, until } from "../support/runtime.js";
import assert from "node:assert/strict";
import { writeFile } from "node:fs/promises";

export async function checkNetlinkSpace(
  evaluate: Evaluate,
  delay: typeof pause,
  cdp: Cdp,
) {
  const initial = await evaluate<Record<string, boolean>>(`(async()=>{
    const m=await import(window.spaceTestModule);
    const node={identity:{pid:920001,start_time_ticks:1},name:'netlink-browser',uid:1000,euid:1000,maps:[]};
    const endpoint={process_id:node.identity,fd:3,fd_count:2,resource:{kind:'socket',device:{major:0,minor:1},inode:'14130'},kind:'socket',access:'read_write'};
    const socket={protocol:{kind:'other',family:16,socket_type:{kind:'dgram'},protocol:0},state:null,local:null,remote:null,path:null,network_peer:false,remote_hostname:null};
    const relation={id:'netlink-route',endpoint,peer:null,label:'NETLINK ROUTE',socket,shared:false,candidate:false};
    const usersock={...relation,id:'usersock',endpoint:{...endpoint,fd:4,resource:{...endpoint.resource,inode:'14131'}},socket:{...socket,protocol:{...socket.protocol,protocol:2}}};
    window.netlinkFixture={processes:[node],fd_relations:[relation,usersock]};
    window.netlinkEvent={process_id:node.identity,resource:endpoint.resource,bytes:100,count:1,write:true};
    m.renderSystemSnapshot(window.netlinkFixture);
    const quiet=m.netlinkVisuals().length===1&&m.netlinkParticles().length===0, suppressed=!m.connectionVisual('netlink-route'), usersockVisible=!!m.connectionVisual('usersock');
    m.selectNetlink(m.netlinkVisuals()[0].id);
    const zeroCounters=document.getElementById('connection-facts').textContent.includes('SEND: 0 bytes / 0 operations')&&document.getElementById('connection-facts').textContent.includes('RECEIVE: 0 bytes / 0 operations');
    m.pruneFiles(performance.now()+60000);
    const idle=m.netlinkVisuals().length===1;
    document.getElementById('close').click();
    const camera=m.cameraView(),process=m.processPosition('920001:1');
    m.renderActivity({ipc:[window.netlinkEvent,{...window.netlinkEvent,write:false,bytes:200}],files:[{process_id:node.identity,file:{device:{major:8,minor:1},inode:'42',generation:0},path:'/tmp/alongside.txt',bytes:1,count:1,write:true}]});
    window.netlinkDirections=m.netlinkParticles();window.netlinkProcess=process;
    const cameraStable=JSON.stringify(camera)===JSON.stringify(m.cameraView());
    const separate=JSON.stringify(m.netlinkVisuals()[0].position)!==JSON.stringify(m.fileVisuals()[0].position);
    m.fitScene();
    return {quiet,zeroCounters,idle,suppressed,usersockVisible,cameraStable,separate};
  })()`);
  for (const [name, value] of Object.entries(initial))
    assert.equal(value, true, name);
  assert.deepEqual(
    await evaluate(
      "[...new Set(window.netlinkDirections.map(p=>p.direction))].sort()",
    ),
    [-1, 1],
  );
  await delay(150);
  const click = async (field: "screen" | "pathScreen") => {
    const [x, y] = await evaluate<[number, number]>(
      `import(window.spaceTestModule).then(m=>m.netlinkVisuals()[0].${field})`,
    );
    const point = { x: (x * 0.5 + 0.5) * 1440, y: (-y * 0.5 + 0.5) * 1100 };
    await cdp("Input.dispatchMouseEvent", { type: "mouseMoved", ...point });
    await cdp("Input.dispatchMouseEvent", {
      type: "mousePressed",
      ...point,
      button: "left",
      clickCount: 1,
    });
    await cdp("Input.dispatchMouseEvent", {
      type: "mouseReleased",
      ...point,
      button: "left",
      clickCount: 1,
    });
  };
  await click("screen");
  assert.equal(
    await evaluate("document.getElementById('connection-label').textContent"),
    "NETLINK ROUTE · FD 3",
  );
  assert.match(
    await evaluate<string>(
      "document.getElementById('connection-facts').textContent",
    ),
    /SEND: 100 bytes \/ 1 operations.*RECEIVE: 200 bytes \/ 1 operations/,
  );
  assert.match(
    await evaluate<string>(
      "document.getElementById('connection-endpoints').textContent",
    ),
    /netlink-browser.*FD 3.*shared FDs/s,
  );
  assert.match(
    await evaluate<string>("document.getElementById('hover').textContent"),
    /NETLINK ROUTE.*SEND 100 bytes.*RECEIVE 200 bytes/s,
  );
  await evaluate("document.getElementById('close').click()");
  await click("pathScreen");
  assert.equal(
    await evaluate("document.getElementById('details').hidden"),
    false,
    "netlink path is selectable",
  );
  const screenshot = await cdp("Page.captureScreenshot", { format: "png" });
  await writeFile(
    repositoryPath("target/browser-space-netlink.png"),
    Buffer.from(screenshot.data, "base64"),
  );
  const retained = await evaluate<Record<string, boolean>>(`(async()=>{
    const m=await import(window.spaceTestModule),before=m.netlinkVisuals()[0],camera=m.cameraView();
    m.renderActivity({ipc:[window.netlinkEvent]});m.renderSystemSnapshot(window.netlinkFixture);
    const stable=JSON.stringify(before.position)===JSON.stringify(m.netlinkVisuals()[0].position);
    const cameraStable=JSON.stringify(camera)===JSON.stringify(m.cameraView());
    const processStable=JSON.stringify(window.netlinkProcess)===JSON.stringify(m.processPosition('920001:1'));
    document.getElementById('search').value='no-such-process';document.getElementById('search').dispatchEvent(new Event('input'));
    const filtered=m.netlinkVisuals().length===0&&m.netlinkParticles().length===0;
    document.getElementById('search').value='';document.getElementById('search').dispatchEvent(new Event('input'));
    const restored=m.netlinkVisuals().length===1;
    m.selectNetlink(m.netlinkVisuals()[0].id);
    m.renderSystemSnapshot({...window.netlinkFixture,fd_relations:[]});
    const closed=m.netlinkVisuals().length===0&&m.netlinkParticles().length===0&&document.getElementById('details').hidden;
    m.pruneFiles(performance.now()+30001);
    const expired=m.netlinkVisuals().length===0&&m.netlinkParticles().length===0&&document.getElementById('details').hidden;
    return {stable,cameraStable,processStable,filtered,restored,closed,expired};
  })()`);
  for (const [name, value] of Object.entries(retained))
    assert.equal(value, true, name);
  await evaluate(`import(window.spaceTestModule).then(m=>{
    m.renderSystemSnapshot(window.netlinkFixture);m.renderActivity({ipc:[window.netlinkEvent]});m.selectNetlink(m.netlinkVisuals()[0].id);
    m.renderSystemSnapshot({processes:[],fd_relations:[]});
  })`);
  assert.equal(
    await evaluate("document.getElementById('details').hidden"),
    false,
    "the exited owner retains plate selection during its animation",
  );
  assert.equal(
    await evaluate(
      "import(window.spaceTestModule).then(m=>!!m.processPosition('920001:1')&&m.netlinkParticles().length>0)",
    ),
    true,
    "Netlink traffic continues after its owner exits",
  );
  await until(
    () =>
      evaluate(
        "import(window.spaceTestModule).then(m=>m.netlinkVisuals().length===0&&!m.processPosition('920001:1')&&document.getElementById('details').hidden)",
      ),
    "Netlink completion releases its owner, plate and selection",
    3000,
  );
  console.log(
    "Netlink browser checks passed: plates, picking, directions, totals, USERSOCK, filtering, stable layout, idle visibility, removal and animation retention.",
  );
}
