import type { api } from "../../src/shared/api.js";

export function deferred<T = unknown>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
// Like the real JSON client, the caller supplies the response type at this boundary.
export function mockApi(
  read: (path: string, options: RequestInit) => Promise<unknown>,
): typeof api {
  return async <T>(path: string, options: RequestInit = {}) =>
    (await read(path, options)) as T;
}

/** Controllable SSE transport; emit accepts unknown to exercise malformed wire data too. */
export class TestEventSource extends EventTarget {
  readyState = 0;
  onopen: ((event: Event) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  constructor(readonly url: string) {
    super();
  }
  close() {
    this.readyState = 2;
  }
  emit(type: string, data: unknown) {
    this.dispatchEvent(new MessageEvent(type, { data: JSON.stringify(data) }));
  }
  open() {
    this.readyState = 1;
    this.onopen?.(new Event("open"));
  }
  error() {
    this.onerror?.(new Event("error"));
  }
  // This transport substitutes only the EventSource surface used by the stores.
  asEventSource(): EventSource {
    return this as unknown as EventSource;
  }
}
