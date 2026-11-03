// Exercise tour ownership through real browser input in live and static replay SPACE.

import assert from "node:assert/strict";
import type { Cdp, Evaluate, WaitFor } from "./harness.js";
import { delay, until } from "../support/runtime.js";

export async function checkSpaceTour(cdp: Cdp, evaluate: Evaluate) {
  const original = await evaluate<string>("location.href");
  const active = async () => {
    const state = await evaluate<string>(
      "document.querySelector('#tour').getAttribute('aria-pressed')",
    );
    const url = new URL(await evaluate<string>("location.href"));
    assert.equal(url.searchParams.get("tour"), state === "true" ? "1" : null);
    url.searchParams.delete("tour");
    assert.equal(
      url.href,
      original,
      "tour updates preserve other parameters and fragments",
    );
    return state;
  };
  const start = () => evaluate("document.querySelector('#tour').click()");
  assert.equal(await active(), "false");
  await start();
  assert.equal(await active(), "true");
  assert.equal(
    await evaluate("document.querySelector('#tour').textContent"),
    "Exit Tour",
  );
  await delay(200);
  await start();
  assert.equal(await active(), "false");
  await start();
  await cdp("Input.dispatchKeyEvent", {
    type: "keyDown",
    key: "Escape",
    code: "Escape",
  });
  await cdp("Input.dispatchKeyEvent", {
    type: "keyUp",
    key: "Escape",
    code: "Escape",
  });
  assert.equal(await active(), "false");
  for (const type of ["mouseWheel", "mousePressed"] as const) {
    await start();
    if (type === "mouseWheel") {
      await cdp("Input.dispatchMouseEvent", {
        type,
        x: 700,
        y: 500,
        deltaX: 0,
        deltaY: 60,
      });
    } else {
      await cdp("Input.dispatchMouseEvent", {
        type,
        x: 700,
        y: 500,
        button: "left",
        clickCount: 1,
      });
      await cdp("Input.dispatchMouseEvent", {
        type: "mouseMoved",
        x: 710,
        y: 505,
        button: "left",
        buttons: 1,
      });
      await cdp("Input.dispatchMouseEvent", {
        type: "mouseReleased",
        x: 710,
        y: 505,
        button: "left",
        clickCount: 1,
      });
    }
    assert.equal(await active(), "false", `${type} returns camera ownership`);
  }
  for (const action of [
    "document.querySelector('#reset').click()",
    "document.querySelector('#rearrange').click()",
    "document.querySelector('#search').dispatchEvent(new Event('input'))",
  ]) {
    await start();
    await evaluate(action);
    assert.equal(await active(), "false", action);
  }
  await start();
  await cdp("Emulation.setEmulatedMedia", {
    features: [{ name: "prefers-reduced-motion", value: "reduce" }],
  });
  await until(
    () => evaluate("document.querySelector('#tour').disabled"),
    "reduced-motion change stops the tour",
  );
  assert.equal(await active(), "false");
  assert.equal(
    await evaluate("document.querySelector('#tour').disabled"),
    true,
  );
  await start();
  assert.equal(await active(), "false");
  await cdp("Emulation.setEmulatedMedia", {
    features: [{ name: "prefers-reduced-motion", value: "no-preference" }],
  });
  await until(
    () => evaluate("!document.querySelector('#tour').disabled"),
    "tour is available after motion preference changes",
  );
  assert.equal(
    await evaluate("document.querySelector('#tour').disabled"),
    false,
  );
}

export async function checkSpaceTourUrl(
  cdp: Cdp,
  evaluate: Evaluate,
  waitFor: WaitFor,
  address: string,
) {
  const url = new URL(address);
  url.searchParams.set("tour", "1");
  url.searchParams.set("view-test", "keep");
  url.hash = "tour-test";
  const active =
    "document.querySelector('#tour')?.getAttribute('aria-pressed')==='true'";
  await cdp("Page.navigate", { url: url.href });
  await waitFor(
    `location.href===${JSON.stringify(url.href)} && ${active}`,
    "tour URL starts after scene loading",
  );
  const origin = await evaluate<number>("performance.timeOrigin");
  await cdp("Page.reload");
  await waitFor(
    `performance.timeOrigin!==${origin} && ${active}`,
    "refresh keeps tour mode",
  );
  await evaluate(
    "history.replaceState({tourTest:true}, '', location.href);document.querySelector('#tour').click()",
  );
  const manual = new URL(url);
  manual.searchParams.delete("tour");
  assert.equal(await evaluate("location.href"), manual.href);
  assert.deepEqual(await evaluate("history.state"), { tourTest: true });

  // Back and forward restore mode without accumulating entries for button toggles.
  await evaluate(
    `history.pushState(null, '', ${JSON.stringify(url.href)});dispatchEvent(new PopStateEvent('popstate'))`,
  );
  await waitFor(active, "URL navigation enters tour");
  await evaluate("history.back()");
  await waitFor(
    `!(${active}) && location.href===${JSON.stringify(manual.href)}`,
    "back restores manual camera mode",
  );
  await evaluate("history.forward()");
  await waitFor(active, "forward restores tour");

  await cdp("Emulation.setEmulatedMedia", {
    features: [{ name: "prefers-reduced-motion", value: "reduce" }],
  });
  await waitFor(
    "document.querySelector('#tour').disabled",
    "reduced motion stops URL tour",
  );
  const reducedOrigin = await evaluate<number>("performance.timeOrigin");
  await cdp("Page.navigate", { url: url.href });
  await waitFor(
    `performance.timeOrigin!==${reducedOrigin} && document.querySelector('#tour')?.disabled && new URL(location.href).searchParams.get('tour')===null`,
    "reduced motion blocks automatic tour on load",
  );
  assert.equal(await evaluate(active), false);
  assert.equal(await evaluate("location.href"), manual.href);
  await cdp("Emulation.setEmulatedMedia", {
    features: [{ name: "prefers-reduced-motion", value: "no-preference" }],
  });
  await waitFor(
    "!document.querySelector('#tour').disabled",
    "motion preference restored",
  );
  assert.equal(
    await evaluate(active),
    false,
    "changing preference does not auto-start touring",
  );
}
