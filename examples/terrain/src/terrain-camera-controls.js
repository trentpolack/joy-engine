
import { BROWSER_EVENT, KEY_CODE } from 'joy-engine/constants';
// Copyright (c) 2026 Trent Polack. Licensed under the MIT License.

const MOVEMENT_KEYS = Object.freeze({forward:KEY_CODE.KEY_W,backward:KEY_CODE.KEY_S,left:KEY_CODE.KEY_A,right:KEY_CODE.KEY_D,down:KEY_CODE.KEY_Q,up:KEY_CODE.KEY_E});
const PRIMARY_BUTTON = 0;
const SECONDARY_BUTTON = 2;

/**
 * @typedef {object} CameraControlsConfig
 * @property {number} flySpeed World units per second.
 * @property {number} lookSensitivity Radians per CSS pixel while flying.
 * @property {number} orbitYawSensitivity Radians per CSS pixel of horizontal dragging.
 * @property {number} orbitPitchSensitivity Radians per CSS pixel of vertical dragging.
 * @property {number} minOrbitPitch Minimum orbit elevation in radians.
 * @property {number} maxOrbitPitch Maximum orbit elevation in radians.
 * @property {number} zoomSensitivity Exponential distance change per scroll pixel.
 * @property {number} minDistance Minimum orbit distance in world units.
 * @property {number} maxDistance Maximum orbit distance in world units.
 * @property {number} maxPitch Absolute pitch limit in radians, short of vertical.
 * @property {number} wheelLinePixels CSS pixels per line for non-pixel wheel events.
 * @property {number} verticalFovRadians Must match the borrowed camera projection.
 */

/**
 * Own input listeners for this example; borrow the canvas, camera, and window.
 * Camera coordinates are Y-up world units; angles are radians and update uses
 * seconds. Keep this policy local until another example needs the same controls.
 */
export class TerrainCameraControls {
  /**
   * @param {HTMLCanvasElement} canvas
   * @param {import('joy-engine').OrbitCamera} camera Mutated in place.
   * @param {CameraControlsConfig} config Speeds use world units/second and radians/CSS pixel.
   * @param {Window} [host] Borrowed input surface, also supplies platform detection.
   */
  constructor(canvas, camera, config, host = window) {
    this.canvas = canvas;
    this.camera = camera;
    this.config = config;
    this.isMac = /Mac/i.test(host.navigator.platform);
    this.events = new AbortController();
    /** @type {Set<string>} */
    this.keys = new Set();
    this.flying = false;
    this.disposed = false;
    this.pointerId = /** @type {number|null} */(null);
    this.pointerX = 0;
    this.pointerY = 0;
    this.originalCursor = canvas.style.cursor;

    // Pointer capture keeps release events arriving when a drag leaves the canvas.
    // AbortSignal removes every listener together; blur clears held keys as well.
    const signal = this.events.signal;
    canvas.addEventListener(BROWSER_EVENT.POINTERDOWN,event => this.beginDrag(event),{signal});
    canvas.addEventListener(BROWSER_EVENT.POINTERMOVE,event => this.drag(event),{signal});
    canvas.addEventListener(BROWSER_EVENT.POINTERUP,event => {
      if(event.pointerId === this.pointerId) { this.stopFlight(); }
    },{signal});
    canvas.addEventListener(BROWSER_EVENT.POINTERCANCEL,() => this.stopFlight(),{signal});
    canvas.addEventListener(BROWSER_EVENT.LOSTPOINTERCAPTURE,() => this.stopFlight(),{signal});
    // Secondary-button flight replaces the native menu only on this viewport.
    canvas.addEventListener(BROWSER_EVENT.CONTEXTMENU,event => event.preventDefault(),{signal});
    canvas.addEventListener(BROWSER_EVENT.WHEEL,event => this.gesture(event),{signal,passive:false});
    host.addEventListener(BROWSER_EVENT.KEYDOWN,event => this.keyDown(event),{signal});
    host.addEventListener(BROWSER_EVENT.KEYUP,event => this.keys.delete(event.code),{signal});
    host.addEventListener(BROWSER_EVENT.BLUR,() => this.stopFlight(),{signal});
  }

  /**
   * Translate the eye and target together while the secondary button is held.
   * Forward follows pitch as well as yaw; Q/E are world down/up. Combined input
   * is normalized so diagonal flight has the same speed as a single direction.
   * @param {number} elapsedSeconds Frame duration in seconds.
   */
  update(elapsedSeconds) {
    if(!this.flying || this.disposed) { return; }
    const {backward,right} = this.camera.basis();
    const forward = Number(this.keys.has(MOVEMENT_KEYS.forward)) - Number(this.keys.has(MOVEMENT_KEYS.backward));
    const strafe = Number(this.keys.has(MOVEMENT_KEYS.right)) - Number(this.keys.has(MOVEMENT_KEYS.left));
    const vertical = Number(this.keys.has(MOVEMENT_KEYS.up)) - Number(this.keys.has(MOVEMENT_KEYS.down));
    const direction = right.map((value,i) => value*strafe - backward[i]*forward + (i === 1 ? vertical : 0));
    const length = Math.hypot(...direction);
    if(length === 0) { return; }
    const distance = this.config.flySpeed*elapsedSeconds/length;
    for(let i = 0; i < 3; i++) {
      this.camera.target[i]+= direction[i]*distance;
    }
  }

  /**
   * Restore pointer state and release listeners; borrowed resources stay alive.
   */
  destroy() {
    if(this.disposed) { return; }
    this.disposed = true;
    this.events.abort();
    this.stopFlight();
  }

  /**
   * Cancel orbit/flight dragging and held input without changing the camera position or orientation.
   */
  stopFlight() {
    const pointerId = this.pointerId;
    this.pointerId = null;
    this.flying = false;
    this.keys.clear();
    this.canvas.style.cursor = this.originalCursor;
    if(pointerId !== null && this.canvas.hasPointerCapture(pointerId)) {
      this.canvas.releasePointerCapture(pointerId);
    }
  }

  /** @private @param {PointerEvent} event */
  beginDrag(event) {
    if((event.button !== PRIMARY_BUTTON && event.button !== SECONDARY_BUTTON) || this.pointerId !== null) { return; }
    event.preventDefault();
    // Remove focus from CONFIG inputs so keyboard movement reaches the viewport.
    this.canvas.focus();
    this.flying = event.button === SECONDARY_BUTTON;
    this.pointerId = event.pointerId;
    this.pointerX = event.clientX;
    this.pointerY = event.clientY;
    this.canvas.style.cursor = this.flying ? 'none' : this.originalCursor;
    this.canvas.setPointerCapture(event.pointerId);
  }

  /** @private @param {PointerEvent} event */
  drag(event) {
    if(this.pointerId === null || event.pointerId !== this.pointerId) { return; }
    const x = event.clientX - this.pointerX, y = event.clientY - this.pointerY;
    this.pointerX = event.clientX;
    this.pointerY = event.clientY;
    if(!this.flying) {
      // Restore the original left-drag orbit: keep focus fixed and rotate the eye
      // around it, with separate horizontal/vertical sensitivities and bounds.
      this.camera.yaw+= x*this.config.orbitYawSensitivity;
      this.camera.pitch = Math.max(this.config.minOrbitPitch,Math.min(this.config.maxOrbitPitch,
        this.camera.pitch + y*this.config.orbitPitchSensitivity));
      return;
    }
    // OrbitCamera derives eye from target. Rebuild target after rotation so mouse
    // look pivots around the eye instead of swinging the eye around an old target.
    const eye = this.camera.basis().eye;
    this.camera.yaw-= x*this.config.lookSensitivity;
    this.camera.pitch = this.clampPitch(this.camera.pitch + y*this.config.lookSensitivity);
    const backward = this.camera.basis().backward;
    this.camera.target = eye.map((value,i) => value - backward[i]*this.camera.distance);
  }

  /** @private @param {KeyboardEvent} event */
  keyDown(event) {
    if(!this.flying || !Object.values(MOVEMENT_KEYS).some(code => code === event.code)) { return; }
    if(this.canvas.ownerDocument.activeElement?.closest('aside')) { return; }
    event.preventDefault();
    this.keys.add(event.code);
  }

  /** @private @param {WheelEvent} event */
  gesture(event) {
    event.preventDefault();
    if(this.flying) { return; }
    // Trackpads report CSS pixels; conventional wheels can report lines/pages.
    const pixels = event.deltaMode === 1 ? this.config.wheelLinePixels : event.deltaMode === 2 ? this.canvas.clientHeight : 1;
    const x = event.deltaX*pixels, y = event.deltaY*pixels;
    // Ignore pinch-generated ctrl+wheel events. Ordinary scrolling now provides
    // zoom, so a trackpad pinch cannot unexpectedly alter this camera or the page.
    if(event.ctrlKey) { return; }
    if(this.isMac ? event.metaKey : event.altKey) {
      const {right,up} = this.camera.basis();
      // Match OrbitCamera's 45-degree vertical FOV. Scale world translation by
      // the visible height at the focus distance so panning follows the gesture.
      const worldPerPixel = 2*this.camera.distance*Math.tan(this.config.verticalFovRadians/2)/Math.max(1,this.canvas.clientHeight);
      for(let i = 0; i < 3; i++) {
        this.camera.target[i]+= (-right[i]*x + up[i]*y)*worldPerPixel;
      }
      return;
    }
    this.camera.distance = Math.max(this.config.minDistance,Math.min(this.config.maxDistance,
      this.camera.distance*Math.exp(y*this.config.zoomSensitivity)));
  }

  /** @private @param {number} pitch */
  clampPitch(pitch) {
    return Math.max(-this.config.maxPitch,Math.min(this.config.maxPitch,pitch));
  }
}
