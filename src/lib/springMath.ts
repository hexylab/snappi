/**
 * Critically damped spring (analytical solution) — TypeScript port of spring.rs.
 * Frame-rate independent, unconditionally stable.
 *
 * Parameterized by half-life: the time (seconds) for the spring
 * to cover 50% of the remaining distance to its target.
 */

import type { ZoomKeyframe } from "./types";

const LN_2 = 0.693147180559945;
const EPSILON = 1e-5;

class Spring {
  position: number;
  velocity: number;
  target: number;

  constructor(initial: number) {
    this.position = initial;
    this.velocity = 0;
    this.target = initial;
  }

  update(halfLife: number, dt: number) {
    if (dt <= 0) return;
    const y = (4 * LN_2) / Math.max(halfLife, EPSILON);
    const yHalf = y / 2;
    const j0 = this.position - this.target;
    const j1 = this.velocity + j0 * yHalf;
    const eydt = Math.exp(-yHalf * dt);
    this.position = eydt * (j0 + j1 * dt) + this.target;
    this.velocity = eydt * (this.velocity - j1 * yHalf * dt);
  }
}

export interface ViewportRect {
  x: number;
  y: number;
  width: number;
  height: number;
  zoom: number;
}

/**
 * Compute the animated viewport state at a given time.
 * Replays the Spring simulation through all keyframes up to timeMs.
 * O(K) where K = number of keyframes — fast enough for per-frame calls.
 */
export function computeViewportAt(
  keyframes: ZoomKeyframe[],
  timeMs: number,
  screenWidth: number,
  screenHeight: number,
): ViewportRect {
  const cx = new Spring(screenWidth / 2);
  const cy = new Spring(screenHeight / 2);
  const zoomSpring = new Spring(1.0);

  // Default half-lives (matches SpringHalfLife in spring.rs)
  let panHL = 0.22;
  let zoomHL = 0.25;
  let prevMs = 0;

  for (const kf of keyframes) {
    if (kf.time_ms > timeMs) {
      // Advance springs to the requested time and stop
      const dt = (timeMs - prevMs) / 1000;
      if (dt > 0) {
        cx.update(panHL, dt);
        cy.update(panHL, dt);
        zoomSpring.update(zoomHL, dt);
      }
      return makeViewport(cx, cy, zoomSpring, screenWidth, screenHeight);
    }

    // Advance springs to this keyframe's time
    const dt = (kf.time_ms - prevMs) / 1000;
    if (dt > 0) {
      cx.update(panHL, dt);
      cy.update(panHL, dt);
      zoomSpring.update(zoomHL, dt);
    }

    // Apply keyframe: set new targets and half-lives
    cx.target = kf.target_x;
    cy.target = kf.target_y;
    zoomSpring.target = kf.zoom_level;

    if (kf.spring_hint) {
      zoomHL = kf.spring_hint.zoom_half_life;
      panHL = kf.spring_hint.pan_half_life;
    } else {
      // Fallback: auto-detect zoom direction (mirrors AnimatedViewport::set_target)
      if (kf.zoom_level > zoomSpring.position) zoomHL = 0.25;
      else if (kf.zoom_level < zoomSpring.position) zoomHL = 0.35;
      panHL = 0.22;
    }

    prevMs = kf.time_ms;
  }

  // Past all keyframes — advance remaining time
  const dt = (timeMs - prevMs) / 1000;
  if (dt > 0) {
    cx.update(panHL, dt);
    cy.update(panHL, dt);
    zoomSpring.update(zoomHL, dt);
  }

  return makeViewport(cx, cy, zoomSpring, screenWidth, screenHeight);
}

/** Convert spring state to a clamped viewport rect (mirrors spring.rs current_viewport). */
function makeViewport(
  cx: Spring,
  cy: Spring,
  zoomSpring: Spring,
  screenWidth: number,
  screenHeight: number,
): ViewportRect {
  const zoom = Math.max(zoomSpring.position, 1.0);
  const vpW = screenWidth / zoom;
  const vpH = screenHeight / zoom;
  const x = Math.max(0, Math.min(screenWidth - vpW, cx.position - vpW / 2));
  const y = Math.max(0, Math.min(screenHeight - vpH, cy.position - vpH / 2));
  return { x, y, width: vpW, height: vpH, zoom };
}
