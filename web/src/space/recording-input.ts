// Bounded UTF-8 acquisition for both local files and HTTP response bodies.

import { MAX_RECORDING_BYTES } from "./recording.js";

/** Inspect bytes rather than extensions or headers: fetch may already have
 * decoded Content-Encoding. Enforce the limit on input and expanded output. */
export async function readRecording(
  source: ReadableStream<Uint8Array<ArrayBuffer>>,
  limit = MAX_RECORDING_BYTES,
): Promise<string> {
  function bounded() {
    let bytes = 0;
    return new TransformStream<
      Uint8Array<ArrayBuffer>,
      Uint8Array<ArrayBuffer>
    >({
      transform(chunk, controller) {
        bytes += chunk.byteLength;
        if (bytes > limit)
          throw new Error("Recording exceeds the 128 MiB replay limit.");
        controller.enqueue(chunk);
      },
    });
  }
  const reader = source.pipeThrough(bounded()).getReader();
  const prefix: Uint8Array<ArrayBuffer>[] = [];
  let length = 0;
  try {
    while (length < 2) {
      const { value, done } = await reader.read();
      if (done) break;
      prefix.push(value);
      length += value.byteLength;
    }
  } catch (error) {
    await reader.cancel().catch(() => {});
    throw error;
  }
  const magic = prefix.flatMap((chunk) => Array.from(chunk.subarray(0, 2)));
  const stream = new ReadableStream<Uint8Array<ArrayBuffer>>({
    async pull(controller) {
      if (prefix.length) {
        controller.enqueue(prefix.shift()!);
        return;
      }
      const { value, done } = await reader.read();
      if (done) controller.close();
      else controller.enqueue(value);
    },
    cancel(reason) {
      return reader.cancel(reason);
    },
  });
  const decoded =
    magic[0] === 0x1f && magic[1] === 0x8b
      ? stream.pipeThrough(new DecompressionStream("gzip"))
      : stream;
  return new Response(decoded.pipeThrough(bounded())).text();
}
