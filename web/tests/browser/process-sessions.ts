import { connectPage, pages, type Cdp, type ProcessChecks } from "./harness.js";
import { until as poll } from "../support/runtime.js";
import assert from "node:assert/strict";

export async function checkProcessSessions({
  cdp,
  evaluate,
  choose,
  until,
  delay,
  url,
  debugPort,
  originalPid,
  otherPid,
}: Omit<ProcessChecks, "waitFor"> & {
  cdp: Cdp;
  until: typeof poll;
  url: string;
  debugPort: string;
}) {
  const pinnedUrl = await evaluate<string>("location.href");
  const { targetId } = await cdp("Target.createTarget", { url });
  let socket: WebSocket | undefined;
  try {
    const page = await until(async () => {
      return (await pages(debugPort)).find((page) => page.id === targetId);
    }, "second browser tab");
    const connection = await connectPage(page.webSocketDebuggerUrl);
    socket = connection.socket;
    const { cdp: call, evaluate: second } = connection;
    await until(
      () => second("document.querySelectorAll('#process-list tr').length>2"),
      "independent list",
    );
    assert.equal(
      await second("document.getElementById('inspector')"),
      null,
      "root always shows list",
    );
    await call("Page.navigate", { url: pinnedUrl });
    await until(
      () =>
        second(
          `document.getElementById('identity')?.textContent.includes('PID ${originalPid} /')`,
        ),
      "second tab process",
    );
    assert.equal(
      await second("location.href"),
      pinnedUrl,
      "new tab retains the observed process identity",
    );
    await evaluate("document.getElementById('back').click()");
    await choose(otherPid);
    await delay(300);
    assert.ok(
      await second(
        `document.getElementById('identity').textContent.includes('PID ${originalPid} /')`,
      ),
      "other tab keeps its own process",
    );
    await evaluate("document.getElementById('back').click()");
    const before = await second(
      "document.getElementById('metrics').textContent",
    );
    await until(
      async () =>
        (await second("document.getElementById('metrics').textContent")) !==
        before,
      "other tab keeps observing after back",
    );
    await until(
      () => evaluate("document.querySelectorAll('#process-list tr').length>2"),
      "main tab list",
    );
    assert.equal(await evaluate("document.getElementById('inspector')"), null);
    await choose(originalPid);
    console.log(
      "Process sessions passed: independent tabs, root list, independent selection and disconnect.",
    );
  } finally {
    socket?.close();
    await cdp("Target.closeTarget", { targetId });
  }
}
