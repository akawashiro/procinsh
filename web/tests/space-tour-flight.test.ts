import { expect, test } from "vitest";

import * as T from "three";
import {
  createTourFlight,
  flythroughTarget,
  FLYTHROUGH_SPEED,
} from "../src/space/tour-flight.js";

const subjects = (count: number) =>
  Array.from({ length: count }, (_, i) => ({
    id: String(i),
    position: { x: i * 6, y: (i % 3) * 4, z: 4 },
    activity: 0,
    connections: 0,
  }));

test.each([0, 1, 2, 8])(
  "%i subjects produce continuous constant-speed flight, including route wraparound",
  (count) => {
    const flight = createTourFlight(subjects(count), 0.7, () => 0.5);
    const previous = flythroughTarget(flight, 0, new T.Vector3());
    for (let i = 1; i <= 3600; i++) {
      const point = flythroughTarget(flight, i / 120, new T.Vector3());
      const speed = point.distanceTo(previous) * 120;
      expect(speed).toBeGreaterThan(FLYTHROUGH_SPEED * 0.97);
      expect(speed).toBeLessThan(FLYTHROUGH_SPEED * 1.03);
      expect(point.z).toBeCloseTo(4, 10);
      previous.copy(point);
    }
  },
);

test("nearby subjects form the route and duplicate positions cannot create zero-length legs", () => {
  const input = subjects(12);
  input.push({ ...input[0], id: "duplicate" });
  const flight = createTourFlight(input, 0, () => 0);
  const visits = flight.path.points.filter((_, i) => i % 2 === 0);
  expect(visits.length).toBeGreaterThanOrEqual(2);
  expect(visits.length).toBeLessThanOrEqual(6);
  expect(visits[0]).toEqual(new T.Vector3(0, 0, 4));
  expect(new Set(visits.map((p) => p.toArray().join(","))).size).toBe(
    visits.length,
  );
  expect(
    visits.every((p) =>
      input.some(
        (s) =>
          p.distanceTo(
            new T.Vector3(s.position.x, s.position.y, s.position.z),
          ) < 1e-9,
      ),
    ),
  ).toBe(true);
  expect(flight.length).toBeGreaterThan(0);
});
