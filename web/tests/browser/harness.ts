import type { Protocol } from "devtools-protocol";
import type { ProtocolMapping } from "devtools-protocol/types/protocol-mapping.js";
import { delay, until } from "../support/runtime.js";

export type Cdp = <M extends keyof ProtocolMapping.Commands>(
  method: M,
  ...params: ProtocolMapping.Commands[M]["paramsType"]
) => Promise<ProtocolMapping.Commands[M]["returnType"]>;
/** Browser expressions cross a JSON boundary; callers specify the expected result. */
export type Evaluate = <T = unknown>(expression: string) => Promise<T>;
export type WaitFor = (expression: string, label: string) => Promise<unknown>;

export interface ProcessChecks {
  evaluate: Evaluate;
  waitFor: WaitFor;
  delay: typeof delay;
  choose(pid: number): Promise<string>;
  originalPid: number;
  otherPid: number;
}

export interface DebugPage {
  id: string;
  type: string;
  webSocketDebuggerUrl: string;
}

export async function pages(debugPort: string): Promise<DebugPage[]> {
  return (await fetch(`http://127.0.0.1:${debugPort}/json/list`)).json();
}

/** Own the DevTools socket, typed command responses, and browser error recording. */
export async function connectPage(url: string, errors: string[] = []) {
  const socket = new WebSocket(url);
  await new Promise<void>((resolve, reject) => {
    socket.addEventListener("open", () => resolve(), { once: true });
    socket.addEventListener("error", reject, { once: true });
  });
  let sequence = 0;
  const pending = new Map<
    number,
    { resolve(value: unknown): void; reject(error: unknown): void }
  >();
  socket.addEventListener("message", (event: MessageEvent<string>) => {
    const message = JSON.parse(event.data) as {
      id?: number;
      result?: unknown;
      error?: { code: number; message: string };
      method?: string;
      params: unknown;
    };
    if (message.id) {
      const task = pending.get(message.id);
      if (task) {
        pending.delete(message.id);
        if (message.error) task.reject(new Error(message.error.message));
        else task.resolve(message.result);
      }
    }
    if (message.method === "Runtime.exceptionThrown") {
      const { exceptionDetails } =
        message.params as Protocol.Runtime.ExceptionThrownEvent;
      errors.push(
        exceptionDetails.text +
          " " +
          (exceptionDetails.exception?.description ?? ""),
      );
    }
    if (message.method === "Log.entryAdded") {
      const { entry } = message.params as Protocol.Log.EntryAddedEvent;
      if (
        entry.level === "error" &&
        !entry.url?.endsWith("/favicon.ico") &&
        !entry.text.includes("favicon.ico")
      )
        errors.push(entry.text);
    }
  });
  socket.addEventListener("close", () => {
    for (const task of pending.values())
      task.reject(new Error("DevTools socket closed"));
    pending.clear();
  });
  const cdp: Cdp = <M extends keyof ProtocolMapping.Commands>(
    method: M,
    ...params: ProtocolMapping.Commands[M]["paramsType"]
  ) =>
    new Promise<ProtocolMapping.Commands[M]["returnType"]>(
      (resolve, reject) => {
        const id = ++sequence;
        pending.set(id, {
          resolve: (value) =>
            resolve(value as ProtocolMapping.Commands[M]["returnType"]),
          reject,
        });
        socket.send(JSON.stringify({ id, method, params: params[0] ?? {} }));
      },
    );
  const evaluate: Evaluate = async <T>(expression: string): Promise<T> => {
    const response = await cdp("Runtime.evaluate", {
      expression,
      returnByValue: true,
      awaitPromise: true,
    });
    if (response.exceptionDetails)
      throw new Error(JSON.stringify(response.exceptionDetails));
    return response.result.value as T;
  };
  const waitFor: WaitFor = (expression, label) =>
    until(async () => {
      try {
        return await evaluate(expression);
      } catch (error) {
        if (
          error instanceof Error &&
          /Execution context was destroyed|Cannot find context|Inspected target navigated/.test(
            error.message,
          )
        )
          return false;
        throw error;
      }
    }, label);
  return { socket, cdp, evaluate, waitFor };
}
