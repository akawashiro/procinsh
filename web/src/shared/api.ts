// JSON requests shared by the list and process pages.
export function errorMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
export async function api<T = unknown>(
  path: string,
  options: RequestInit = {},
): Promise<T> {
  const response = await fetch(path, {
    cache: "no-store",
    ...options,
    headers: { "Content-Type": "application/json", ...options.headers },
  });
  const body: unknown = await response
    .json()
    .catch(() => ({ error: `HTTP ${response.status}` }));
  if (!response.ok)
    throw new Error(
      body && typeof body === "object" && "error" in body
        ? String(body.error)
        : `HTTP ${response.status}`,
    );
  // The server owns this JSON contract; this assertion is not runtime validation.
  return body as T;
}
