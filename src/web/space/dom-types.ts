// Element IDs in the SPACE template.
export interface SpaceElements {
  world: HTMLCanvasElement;
  labels: HTMLCanvasElement;
  brand: HTMLAnchorElement;
  back: HTMLAnchorElement;
  fps: HTMLElement;
  search: HTMLInputElement;
  reset: HTMLButtonElement;
  rearrange: HTMLButtonElement;
  details: HTMLElement;
  close: HTMLButtonElement;
  "process-details": HTMLElement;
  name: HTMLElement;
  pid: HTMLElement;
  inspect: HTMLAnchorElement;
  "connection-details": HTMLElement;
  "connection-kind": HTMLElement;
  "connection-label": HTMLElement;
  "connection-state": HTMLElement;
  "connection-facts": HTMLElement;
  "connection-endpoints": HTMLElement;
  hover: HTMLElement;
  failure: HTMLElement;
}

export function spaceElement<K extends keyof SpaceElements>(
  id: K,
): SpaceElements[K] {
  const element = document.getElementById(id);
  if (!element) throw new Error(`Missing element: ${id}`);
  return element as SpaceElements[K];
}
