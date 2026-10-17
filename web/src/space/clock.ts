// SPACE animation time can be supplied by replay while camera input stays live.
let readClock: (() => number) | undefined;
/** Live frames use the RAF timestamp; replay reads the virtual clock after ticks. */
export const spaceNow = (frameTime?: number) =>
  readClock ? readClock() : (frameTime ?? performance.now());
export function setSpaceClock(clock?: () => number) {
  readClock = clock;
}
