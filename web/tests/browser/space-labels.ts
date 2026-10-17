import assert from "node:assert/strict";
import type { Cdp, Evaluate } from "./harness.js";
import { delay, until } from "../support/runtime.js";

/** Verify the retained overlay while the WebGL animation loop keeps running. */
export async function checkSpaceLabels(evaluate: Evaluate, cdp: Cdp) {
  await evaluate(`(async()=>{
    window.spaceTestSources.forEach(source=>source.close());
    const m=await import(window.spaceTestModule);
    m.renderSystemSnapshot({processes:[],fd_relations:[]});
    window.spaceTestSources.at(-1).dispatchEvent(new MessageEvent('gap',{data:'{}'}));
    window.labelSnapshot={processes:[
      {identity:{pid:920001,start_time_ticks:1},name:'label-first',maps:[]},
      {identity:{pid:920002,start_time_ticks:1},parent_id:{pid:920001,start_time_ticks:1},name:'label-second',maps:[]}
    ],fd_relations:[]};
    const context=document.getElementById('labels').getContext('2d');
    window.labelDraws=0;window.labelTexts=[];
    window.originalLabelClear=context.clearRect;
    window.originalLabelFill=context.fillText;
    context.clearRect=function(...args){window.labelDraws++;window.labelTexts=[];return window.originalLabelClear.apply(this,args);};
    context.fillText=function(text,...args){window.labelTexts.push({text,color:this.fillStyle});return window.originalLabelFill.call(this,text,...args);};
    m.renderSystemSnapshot(window.labelSnapshot,true);m.fitScene();
  })()`);
  const draws = () => evaluate<number>("window.labelDraws");
  const settle = async () => {
    let previous = -1,
      stable = 0;
    await until(async () => {
      await delay(150);
      const current = await draws();
      stable = current === previous ? stable + 1 : 0;
      previous = current;
      return stable >= 3;
    }, "label overlay settles after camera damping");
  };
  try {
    await settle();
    const idle = await draws();
    await delay(400);
    assert.equal(await draws(), idle, "stationary labels reuse the overlay");
    assert.ok(
      await evaluate<boolean>(
        "window.labelTexts.some(l=>l.text==='label-first')",
      ),
    );
    await evaluate(
      "import(window.spaceTestModule).then(m=>m.renderActivity({window_ms:100,cpu:[{process_id:{pid:920001,start_time_ticks:1},runtime_ns:20000000,running_threads:1,switches:1,cpus:[0]}]}))",
    );
    await delay(80);
    assert.ok(
      await evaluate<boolean>(
        "import(window.spaceTestModule).then(m=>m.cpuGlowVisual('920001:1')?.g>0)",
      ),
      "CPU glow animates with the retained overlay",
    );
    assert.equal(
      await draws(),
      idle,
      "CPU glow does not redraw stationary labels",
    );
    await evaluate(
      "import(window.spaceTestModule).then(m=>m.selectProcess('920001:1'))",
    );
    await until(
      () =>
        evaluate(
          "window.labelTexts.some(l=>l.text==='label-first'&&l.color==='#e0faff')",
        ),
      "selection updates label highlight",
    );
    const beforeZoom = await draws();
    await cdp("Input.dispatchMouseEvent", {
      type: "mouseWheel",
      x: 720,
      y: 600,
      deltaX: 0,
      deltaY: -100,
    });
    await until(
      async () => (await draws()) > beforeZoom,
      "zoom redraws labels",
    );
    const beforeOrbit = await draws();
    await cdp("Input.dispatchMouseEvent", {
      type: "mousePressed",
      x: 720,
      y: 600,
      button: "left",
      buttons: 1,
      clickCount: 1,
    });
    await cdp("Input.dispatchMouseEvent", {
      type: "mouseMoved",
      x: 800,
      y: 650,
      button: "left",
      buttons: 1,
    });
    await cdp("Input.dispatchMouseEvent", {
      type: "mouseReleased",
      x: 800,
      y: 650,
      button: "left",
      buttons: 0,
      clickCount: 1,
    });
    await until(
      async () => (await draws()) > beforeOrbit + 2,
      "camera motion and damping redraw labels",
    );
    await settle();
    const beforeResize = await draws();
    await cdp("Emulation.setDeviceMetricsOverride", {
      width: 1400,
      height: 1050,
      deviceScaleFactor: 1,
      mobile: false,
    });
    await until(
      async () => (await draws()) > beforeResize,
      "resize redraws the overlay",
    );
    await evaluate(
      "import(window.spaceTestModule).then(m=>{window.labelSnapshot.processes[0].name='label-renamed';m.renderSystemSnapshot(window.labelSnapshot)})",
    );
    await until(
      () =>
        evaluate(
          "window.labelTexts.some(l=>l.text==='label-renamed')&&!window.labelTexts.some(l=>l.text==='label-first')",
        ),
      "snapshot replaces label text",
    );
    await evaluate(`import(window.spaceTestModule).then(m=>m.renderActivity({signals:[{
      timestamp_ns:1,src_pid:920001,dst_pid:920002,signal:10,
      source_id:{pid:920001,start_time_ticks:1},destination_id:{pid:920002,start_time_ticks:1}
    }]}))`);
    await until(
      () => evaluate("window.labelTexts.some(l=>l.text.includes('USR1'))"),
      "moving signal label appears",
    );
    const signalDraws = await draws();
    await delay(150);
    assert.ok(
      (await draws()) > signalDraws + 1,
      "signal labels animate between data updates",
    );
    await until(
      () => evaluate("!window.labelTexts.some(l=>l.text.includes('USR1'))"),
      "expired signal label clears",
    );
    await settle();
    const expired = await draws();
    await delay(300);
    assert.equal(await draws(), expired, "overlay rests after signal expiry");
    await evaluate(
      "document.getElementById('search').value='label-second';document.getElementById('search').dispatchEvent(new Event('input'))",
    );
    await until(
      () => evaluate("!window.labelTexts.some(l=>l.text==='label-renamed')"),
      "search clears filtered labels",
    );
    await evaluate(
      "document.getElementById('search').value='';document.getElementById('search').dispatchEvent(new Event('input'))",
    );
    await until(
      () => evaluate("window.labelTexts.some(l=>l.text==='label-renamed')"),
      "search restores labels",
    );
  } finally {
    await evaluate(
      "(()=>{const context=document.getElementById('labels').getContext('2d');context.clearRect=window.originalLabelClear;context.fillText=window.originalLabelFill;})()",
    );
    await cdp("Emulation.setDeviceMetricsOverride", {
      width: 1440,
      height: 1100,
      deviceScaleFactor: 1,
      mobile: false,
    });
  }
}
