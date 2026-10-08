import { spawn, type SpawnOptions } from "node:child_process";
import { fileURLToPath } from "node:url";
import { resolve } from "node:path";

export const repositoryRoot = fileURLToPath(
  new URL("../../../", import.meta.url),
);
export const repositoryPath = (...parts: string[]) =>
  resolve(repositoryRoot, ...parts);
export const delay = (ms: number) =>
  new Promise<void>((resolve) => setTimeout(resolve, ms));

/** Capture child output while keeping fixture and backend paths relative to the repository. */
export function launch(
  program: string,
  args: string[],
  options: SpawnOptions = {},
) {
  const child = Object.assign(
    spawn(program, args, {
      cwd: repositoryRoot,
      ...options,
      stdio: ["ignore", "pipe", "pipe"],
    }),
    { output: "" },
  );
  child.stdout!.on(
    "data",
    (chunk: Buffer) => (child.output += chunk.toString()),
  );
  child.stderr!.on(
    "data",
    (chunk: Buffer) => (child.output += chunk.toString()),
  );
  child.on("error", (error: Error) => (child.output += error.message));
  return child;
}

export type TestProcess = ReturnType<typeof launch>;

export async function until<T>(
  read: () => T | Promise<T>,
  label: string,
  timeout = 15000,
): Promise<NonNullable<T>> {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    const value = await read();
    if (value) return value;
    await delay(80);
  }
  throw new Error(`Timed out: ${label}`);
}
