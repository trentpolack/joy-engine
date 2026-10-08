// Copyright (c) 2026 Trent Polack. Licensed under the MIT License.
import assert from 'node:assert/strict';
import test from 'node:test';
import { OrbitCamera } from 'joy-engine';
import { TerrainCameraControls } from '../src/terrain-camera-controls.js';

// Browser surfaces are borrowed dependencies. Native EventTarget exercises real
// listener/AbortSignal lifetimes without requiring a GPU for camera state tests.
function fixture(platform = 'MacIntel') {
  const host = new EventTarget();
  host.navigator = {platform};
  const canvas = new EventTarget();
  canvas.style = {cursor:'crosshair'};
  canvas.clientHeight = 800;
  canvas.focus = () => {};
  canvas.ownerDocument = {activeElement:null};
  canvas.setPointerCapture = () => {};
  canvas.hasPointerCapture = () => false;
  canvas.releasePointerCapture = () => {};
  const camera = new OrbitCamera();
  camera.target = [0,0,0]; camera.distance = 10; camera.yaw = Math.PI/2; camera.pitch = 0;
  const controls = new TerrainCameraControls(canvas,camera,{
    flySpeed:10,lookSensitivity:0.01,orbitYawSensitivity:0.01,orbitPitchSensitivity:0.01,minOrbitPitch:0.01,maxOrbitPitch:1.4,
    zoomSensitivity:0.001,minDistance:1,maxDistance:100,maxPitch:1.5,wheelLinePixels:16,verticalFovRadians:Math.PI/4
  },host);
  return {host,canvas,camera,controls};
}
function emit(target,type,properties = {}) {
  const event = new Event(type,{cancelable:true});
  Object.assign(event,properties);
  target.dispatchEvent(event);
  return event;
}
function near(actual,expected) {
  actual.forEach((value,i) => assert.ok(Math.abs(value - expected[i]) < 1e-8,`${actual} should equal ${expected}`));
}

test('flight follows facing and strafes, with independent world-height movement', () => {
  const {host,canvas,camera,controls} = fixture();
  emit(host,'keydown',{code:'KeyW'}); controls.update(1);
  near(camera.basis().eye,[10,0,0]); // Movement requires a held secondary button.
  emit(canvas,'pointerdown',{button:2,pointerId:1,clientX:0,clientY:0});
  emit(host,'keydown',{code:'KeyW'}); controls.update(1);
  near(camera.basis().eye,[0,0,0]); // Facing -X, not the previous fixed -Z axis.
  emit(host,'keyup',{code:'KeyW'});
  emit(host,'keydown',{code:'KeyD'}); controls.update(1);
  near(camera.basis().eye,[0,0,-10]);
  emit(host,'keyup',{code:'KeyD'});
  emit(host,'keydown',{code:'KeyE'}); controls.update(1);
  near(camera.basis().eye,[0,10,-10]);
  emit(host,'keyup',{code:'KeyE'});
  emit(host,'keydown',{code:'KeyQ'}); controls.update(1);
  near(camera.basis().eye,[0,0,-10]);
  controls.destroy();
});

test('pitched flight follows the look direction and releasing cancels held movement', () => {
  const {host,canvas,camera,controls} = fixture();
  camera.yaw = 0; camera.pitch = Math.PI/6;
  emit(canvas,'pointerdown',{button:2,pointerId:1,clientX:0,clientY:0});
  emit(host,'keydown',{code:'KeyW'}); controls.update(1);
  near(camera.basis().eye,[0,0,0]);
  emit(host,'keyup',{code:'KeyW'});
  emit(host,'keydown',{code:'KeyS'}); controls.update(1);
  near(camera.basis().eye,[0,5,Math.sqrt(75)]);
  emit(host,'keyup',{code:'KeyS'});
  emit(host,'keydown',{code:'KeyA'}); controls.update(1);
  near(camera.basis().eye,[-10,5,Math.sqrt(75)]);
  emit(canvas,'pointerup',{pointerId:1});
  controls.update(1);
  near(camera.basis().eye,[-10,5,Math.sqrt(75)]);
  emit(canvas,'pointerdown',{button:2,pointerId:2,clientX:0,clientY:0});
  controls.update(1); // Re-entering flight must not retain the old left key.
  near(camera.basis().eye,[-10,5,Math.sqrt(75)]);
  controls.destroy();
});

test('flight look rotates around the eye and blur/disposal end held input', () => {
  const {host,canvas,camera,controls} = fixture();
  const eye = camera.basis().eye;
  emit(canvas,'pointerdown',{button:2,pointerId:1,clientX:0,clientY:0});
  assert.equal(canvas.style.cursor,'none');
  assert.equal(emit(canvas,'contextmenu').defaultPrevented,true);
  emit(canvas,'pointermove',{pointerId:1,clientX:20,clientY:10});
  near(camera.basis().eye,eye);
  assert.notEqual(camera.pitch,0);
  emit(host,'keydown',{code:'KeyW'});
  emit(host,'blur');
  assert.equal(canvas.style.cursor,'crosshair');
  controls.update(1); near(camera.basis().eye,eye);
  emit(canvas,'pointerdown',{button:2,pointerId:1,clientX:20,clientY:10});
  controls.destroy(); controls.destroy();
  const target = [...camera.target];
  emit(canvas,'wheel',{deltaX:30,deltaY:20,deltaMode:0,metaKey:false,altKey:false,ctrlKey:false});
  near(camera.target,target);
  assert.equal(canvas.style.cursor,'crosshair');
  assert.equal(emit(canvas,'contextmenu').defaultPrevented,false);
});

test('left-drag orbits, scrolling zooms, and platform modifier pans without rotating', () => {
  for(const platform of ['MacIntel','Win32']) {
    const {canvas,camera,controls} = fixture(platform);
    const target = [...camera.target];
    const eye = camera.basis().eye;
    emit(canvas,'pointerdown',{button:0,pointerId:1,clientX:0,clientY:0});
    assert.equal(controls.flying,false);
    assert.equal(canvas.style.cursor,'crosshair');
    emit(canvas,'pointermove',{pointerId:1,clientX:20,clientY:10});
    emit(canvas,'pointerup',{pointerId:1});
    near(camera.target,target);
    assert.notDeepEqual(camera.basis().eye,eye);
    const originalDistance = camera.distance;
    emit(canvas,'wheel',{deltaX:20,deltaY:10,deltaMode:0,metaKey:false,altKey:false,ctrlKey:false});
    assert.notEqual(camera.distance,originalDistance);
    const yaw = camera.yaw, pitch = camera.pitch;
    assert.notEqual(yaw,Math.PI/2);
    emit(canvas,'wheel',{deltaX:20,deltaY:10,deltaMode:0,metaKey:platform === 'MacIntel',altKey:platform === 'Win32',ctrlKey:false});
    assert.equal(camera.yaw,yaw); assert.equal(camera.pitch,pitch);
    assert.notDeepEqual(camera.target,target);
    const distance = camera.distance;
    emit(canvas,'wheel',{deltaX:0,deltaY:10,deltaMode:0,ctrlKey:true});
    assert.equal(camera.distance,distance); // Pinch no longer drives camera zoom.
    controls.destroy();
  }
});
