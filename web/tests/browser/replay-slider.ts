// Check the painted native thumb, including padding that value-only checks miss.

import assert from "node:assert/strict";
import type { Cdp, Evaluate } from "./harness.js";

export async function checkReplaySliderTravel(cdp: Cdp, evaluate: Evaluate) {
  await evaluate(
    "document.querySelector('#timeline').style.accentColor='#3399ff'",
  );
  async function thumbAt(value: number) {
    await evaluate(
      `(()=>{const slider=document.querySelector('#timeline');slider.value=${value};slider.dispatchEvent(new Event('input'))})()`,
    );
    const rectangle = await evaluate<{
      x: number;
      y: number;
      width: number;
      height: number;
    }>(
      "(()=>{const r=document.querySelector('#timeline').getBoundingClientRect();return {x:r.x,y:r.y,width:r.width,height:r.height}})()",
    );
    const screenshot = await cdp("Page.captureScreenshot", {
      format: "png",
      clip: { ...rectangle, scale: 1 },
    });
    return evaluate<{
      center: number | null;
      width: number;
    }>(`new Promise((resolve,reject)=>{
      const image=new Image();
      image.onload=()=>{
        const canvas=document.createElement('canvas');canvas.width=image.width;canvas.height=image.height;
        const context=canvas.getContext('2d');context.drawImage(image,0,0);
        const pixels=context.getImageData(0,Math.floor(image.height/2)-6,image.width,1).data;
        const positions=[];
        for(let x=0;x<image.width;x++){
          const [red,green,blue]=pixels.slice(x*4,x*4+3);
          if(blue>green+25&&green>red+25)positions.push(x);
        }
        resolve({center:positions.length?(positions[0]+positions.at(-1))/2:null,width:image.width});
      };
      image.onerror=()=>reject(new Error('Unable to decode slider screenshot'));
      image.src=${JSON.stringify("data:image/png;base64," + screenshot.data)};
    })`);
  }
  try {
    const start = await thumbAt(0);
    const end = await thumbAt(1);
    assert.ok(
      start.center !== null && start.center < 12,
      `the thumb reaches the left end of the track: ${JSON.stringify(start)}`,
    );
    assert.ok(
      end.center !== null && end.center > end.width - 12,
      `the thumb reaches the right end of the track: ${JSON.stringify(end)}`,
    );
  } finally {
    await evaluate(
      "(()=>{const slider=document.querySelector('#timeline');slider.style.removeProperty('accent-color');slider.value=0;slider.dispatchEvent(new Event('input'))})()",
    );
  }
}
