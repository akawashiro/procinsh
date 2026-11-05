// Plan scene-aware shots and move a Z-up camera using frame time, independent of replay.

import * as T from "three";
import type { OrbitControls } from "three/addons/controls/OrbitControls.js";
import type { Position, TourScene, TourShot } from "./types.js";
import {
  createTourFlight,
  flythroughTarget,
  FLYTHROUGH_SECONDS,
  type TourFlight,
} from "./tour-flight.js";

interface Pose {
  target: T.Vector3;
  offset: T.Vector3;
}
interface Shot {
  kind: TourShot;
  arrival: Pose;
  departure: Pose;
  travel: number;
  drift: number;
  hold: number;
  orbitAngle: number;
  flight?: TourFlight;
}
const TOUR_SPEED = 1.4;
const ease = (t: number) => t * t * t * (t * (t * 6 - 15) + 10);
const vector = (p: Position) => new T.Vector3(p.x, p.y, p.z);

/** OrbitControls stays enabled so its start event can interrupt before it moves. */
export class TourCamera {
  active = false;
  private shot?: Shot;
  private origin?: Pose;
  private elapsed = 0;
  private lastTime?: number;
  private shotIndex = 0;
  private previousSubject?: string;
  private untilRearrange = 0;
  private damping = true;
  private readonly interrupt = () => this.stop();

  constructor(
    private readonly camera: T.PerspectiveCamera,
    private readonly controls: OrbitControls,
    private readonly read: () => TourScene,
    private readonly changed: (active: boolean) => void,
    private readonly rearrange: () => void,
    private readonly random = Math.random,
  ) {
    controls.addEventListener("start", this.interrupt);
  }

  start(reducedMotion = false) {
    if (reducedMotion || this.active) return;
    // Drain residual manual damping while preserving the current composition.
    const position = this.camera.position.clone();
    const target = this.controls.target.clone();
    this.damping = this.controls.enableDamping;
    this.controls.enableDamping = false;
    this.controls.update();
    this.camera.position.copy(position);
    this.controls.target.copy(target);
    this.controls.update();
    this.active = true;
    this.shotIndex = 0;
    this.previousSubject = undefined;
    this.untilRearrange = 100 + this.random() * 50;
    this.lastTime = undefined;
    this.nextShot();
    this.changed(true);
  }

  stop() {
    if (!this.active) return;
    this.active = false;
    this.controls.enableDamping = this.damping;
    this.lastTime = undefined;
    this.changed(false);
  }

  /** Reset on visibility changes so suspended time never advances a shot. */
  suspend() {
    this.lastTime = undefined;
  }

  update(wallTime: number) {
    if (!this.active || !this.shot || !Number.isFinite(wallTime)) return;
    const gap =
      this.lastTime === undefined ? 0 : (wallTime - this.lastTime) / 1000;
    const delta = gap >= 0 && gap < 0.25 ? gap : 0;
    this.lastTime = wallTime;
    this.elapsed += delta * TOUR_SPEED;
    this.untilRearrange -= delta;
    const shot = this.shot;
    if (this.elapsed < shot.travel) {
      this.move(this.origin!, shot.arrival, ease(this.elapsed / shot.travel));
    } else if (shot.flight) {
      const seconds = Math.min(
        FLYTHROUGH_SECONDS,
        (this.elapsed - shot.travel) / TOUR_SPEED,
      );
      flythroughTarget(shot.flight, seconds, this.controls.target);
      this.camera.position.copy(this.controls.target).add(shot.flight.offset);
      this.controls.update();
      if (this.elapsed >= shot.travel + shot.drift) this.nextShot();
    } else if (this.elapsed < shot.travel + shot.drift) {
      this.move(
        shot.arrival,
        shot.departure,
        ease((this.elapsed - shot.travel) / shot.drift),
        shot.orbitAngle,
      );
    } else {
      this.move(shot.arrival, shot.departure, 1, shot.orbitAngle);
      if (this.elapsed >= shot.travel + shot.drift + shot.hold) {
        // Rearrange from a wide composition, without invoking manual Fit all.
        if (shot.kind === "overview" && this.untilRearrange <= 0) {
          this.rearrange();
          this.untilRearrange = 100 + this.random() * 50;
        }
        this.nextShot();
      }
    }
  }

  get shotKind() {
    return this.shot?.kind;
  }

  dispose() {
    this.stop();
    this.controls.removeEventListener("start", this.interrupt);
  }

  private pose(): Pose {
    return {
      target: this.controls.target.clone(),
      offset: this.camera.position.clone().sub(this.controls.target),
    };
  }

  private move(from: Pose, to: Pose, t: number, orbitAngle?: number) {
    // Interpolate offsets in cylindrical coordinates: opposite viewpoints cannot
    // collapse through the target, and the Z-up horizon remains level.
    const a = Math.atan2(from.offset.y, from.offset.x);
    const b = Math.atan2(to.offset.y, to.offset.x);
    // An explicit sweep preserves full revolutions; endpoint angles alone
    // would reduce a 360-degree orbit to a stationary pose.
    const sweep = orbitAngle ?? Math.atan2(Math.sin(b - a), Math.cos(b - a));
    const angle = a + sweep * t;
    const radius = T.MathUtils.lerp(
      Math.hypot(from.offset.x, from.offset.y),
      Math.hypot(to.offset.x, to.offset.y),
      t,
    );
    this.controls.target.lerpVectors(from.target, to.target, t);
    this.camera.position
      .copy(this.controls.target)
      .add(
        new T.Vector3(
          Math.cos(angle) * radius,
          Math.sin(angle) * radius,
          T.MathUtils.lerp(from.offset.z, to.offset.z, t),
        ),
      );
    this.controls.update();
  }

  private nextShot() {
    const scene = this.read();
    const bounds = new T.Box3();
    for (const subject of scene.subjects) {
      bounds.expandByPoint(
        vector(subject.position).add(new T.Vector3(-1, -1, -4)),
      );
      bounds.expandByPoint(
        vector(subject.position).add(new T.Vector3(1, 1, 4)),
      );
    }
    for (const curve of scene.connections)
      for (const point of curve) bounds.expandByPoint(vector(point));
    if (bounds.isEmpty())
      bounds.setFromCenterAndSize(
        new T.Vector3(0, 0, 4),
        new T.Vector3(12, 12, 8),
      );
    const center = bounds.getCenter(new T.Vector3());
    const extent = Math.max(16, bounds.getSize(new T.Vector3()).length());
    const kinds: TourShot[] = ["overview", "hero", "flythrough"];
    let kind = kinds[this.shotIndex++ % kinds.length];
    if (kind === "hero" && !scene.subjects.length) kind = "overview";
    let target = center.clone();
    let distance = extent * 0.85;
    let elevation = 0.45;
    const angle = this.shotIndex * 2.399963 + this.random() * 0.7;
    const ranked = scene.subjects
      .filter(
        (s) => scene.subjects.length === 1 || s.id !== this.previousSubject,
      )
      .map((s) => ({
        subject: s,
        score:
          Math.log1p(s.connections) * 2 +
          Math.min(1, s.activity) +
          this.random() * 2,
      }))
      .sort((a, b) => b.score - a.score);
    const subject = ranked[0]?.subject;
    if (kind === "hero" && subject) {
      this.previousSubject = subject.id;
      target = vector(subject.position);
      distance = Math.max(16, Math.min(34, extent * 0.24));
      elevation = 0.5;
    }
    // Fit wide shots to the narrower field of view (portrait windows included).
    if (kind === "overview") {
      const halfFov = T.MathUtils.degToRad(this.camera.fov / 2);
      const halfAngle = Math.min(
        halfFov,
        Math.atan(Math.tan(halfFov) * this.camera.aspect),
      );
      distance =
        (extent * 0.52) / (Math.sin(halfAngle) * Math.hypot(1, elevation));
    }
    const maxDistance = this.controls.maxDistance * 0.95;
    distance = Math.min(distance, maxDistance / Math.hypot(1, elevation));
    const flight =
      kind === "flythrough"
        ? createTourFlight(scene.subjects, angle, this.random)
        : undefined;
    const arrival: Pose = flight
      ? {
          target: flight.path.getPointAt(0),
          offset: flight.offset.clone(),
        }
      : {
          target,
          offset: new T.Vector3(
            Math.cos(angle) * distance,
            Math.sin(angle) * distance,
            distance * elevation,
          ),
        };
    const orbitAngle =
      kind === "overview"
        ? Math.PI * 2
        : kind === "hero"
          ? 0.12 + this.random() * 0.15
          : 0;
    const departure: Pose = {
      target: target.clone(),
      offset: arrival.offset
        .clone()
        .applyAxisAngle(new T.Vector3(0, 0, 1), orbitAngle),
    };
    if (flight) flythroughTarget(flight, FLYTHROUGH_SECONDS, departure.target);
    this.origin = this.pose();
    this.elapsed = 0;
    this.shot = {
      kind,
      orbitAngle,
      flight,
      arrival,
      departure,
      travel: 8 + this.random() * 4,
      drift: flight ? FLYTHROUGH_SECONDS * TOUR_SPEED : 12 + this.random() * 8,
      hold: flight ? 0 : this.random() < 0.5 ? 3 + this.random() * 4 : 0,
    };
  }
}
