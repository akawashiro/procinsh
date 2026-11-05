// Build a closed route near several processes and sample it at constant world speed.

import * as T from "three";
import type { TourSubject } from "./types.js";

export const FLYTHROUGH_SECONDS = 15;
export const FLYTHROUGH_SPEED = 6;
export interface TourFlight {
  path: T.CatmullRomCurve3;
  length: number;
  offset: T.Vector3;
}

export function createTourFlight(
  subjects: readonly TourSubject[],
  angle: number,
  random = Math.random,
): TourFlight {
  const seed = subjects[Math.floor(random() * subjects.length)];
  const center = seed
    ? new T.Vector3(seed.position.x, seed.position.y, seed.position.z)
    : new T.Vector3(0, 0, 4);
  const nearby = subjects
    .map(({ position }) => new T.Vector3(position.x, position.y, position.z))
    .sort((a, b) => a.distanceToSquared(center) - b.distanceToSquared(center));
  const selected: T.Vector3[] = [];
  for (const point of nearby) {
    if (selected.every((p) => p.distanceToSquared(point) > 0.01))
      selected.push(point);
    if (selected.length === 6) break;
  }
  const points: T.Vector3[] = [];
  if (selected.length < 2) {
    for (let i = 0; i < 4; i++) {
      const a = (i * Math.PI) / 2;
      points.push(
        center.clone().add(new T.Vector3(Math.cos(a) * 5, Math.sin(a) * 5, 0)),
      );
    }
  } else {
    // Walk the cluster's convex boundary to avoid tight reversals between
    // nearby interior subjects. Collinear clusters reduce to a rounded oval.
    const sorted = [...selected].sort((a, b) => a.x - b.x || a.y - b.y);
    const cross = (a: T.Vector3, b: T.Vector3, c: T.Vector3) =>
      (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    function boundary(input: T.Vector3[]) {
      const chain: T.Vector3[] = [];
      for (const point of input) {
        while (
          chain.length > 1 &&
          cross(chain.at(-2)!, chain.at(-1)!, point) <= 0
        )
          chain.pop();
        chain.push(point);
      }
      chain.pop();
      return chain;
    }
    const ordered = [...boundary(sorted), ...boundary([...sorted].reverse())];
    for (let i = 0; i < ordered.length; i++) {
      const from = ordered[i],
        to = ordered[(i + 1) % ordered.length];
      const direction = to.clone().sub(from);
      const side = new T.Vector3(direction.y, -direction.x, 0);
      if (side.lengthSq() < 1e-6) side.set(1, 0, 0);
      side.normalize().multiplyScalar(Math.min(4, direction.length() * 0.35));
      points.push(from, from.clone().lerp(to, 0.5).add(side));
    }
  }
  const path = new T.CatmullRomCurve3(points, true, "centripetal");
  path.arcLengthDivisions = 4096;
  return {
    path,
    length: path.getLength(),
    offset: new T.Vector3(Math.cos(angle) * 14, Math.sin(angle) * 14, 6),
  };
}

export function flythroughTarget(
  flight: TourFlight,
  seconds: number,
  target: T.Vector3,
) {
  const distance = Math.max(0, seconds) * FLYTHROUGH_SPEED;
  return flight.path.getPointAt(
    (distance % flight.length) / flight.length,
    target,
  );
}
