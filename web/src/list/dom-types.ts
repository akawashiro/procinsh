// Element IDs in the process list template.
export interface ListElements {
  brand: HTMLAnchorElement;
  error: HTMLElement;
  explorer: HTMLElement;
  search: HTMLInputElement;
  sort: HTMLSelectElement;
  "process-count": HTMLElement;
  "process-list": HTMLElement;
}

export function listElement<K extends keyof ListElements>(
  id: K,
): ListElements[K] {
  const element = document.getElementById(id);
  if (!element) throw new Error(`Missing element: ${id}`);
  return element as ListElements[K];
}
