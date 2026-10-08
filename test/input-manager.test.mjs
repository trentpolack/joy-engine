import assert from 'node:assert/strict';
import test from 'node:test';
import { InputManager } from '../src/browser/input/input-manager.ts';

function fixture() {
  const target = new EventTarget();
  const documentTarget = new EventTarget();
  let pads = [];
  const presses = [];
  const manager = new InputManager({
    target, documentTarget, pointerTarget: target, getGamepads: () => pads,
    actions: {
      fire: {bindings: [{key: 'Space'}, {pointer: 0}, {button: 0}, {virtual: 'fire'}]},
      right: {bindings: [{key: 'KeyD'}, {virtual: 'right'}]},
      left: {bindings: [{key: 'KeyA'}]},
      menu: {contexts: ['menu'], bindings: [{key: 'Enter'}]}
    },
    axes: {move: {positive: 'right', negative: 'left', gamepadAxis: 0}},
    onPress: name => presses.push(name)
  });
  function send(type, properties = {}, source = target) {
    const event = new Event(type, {cancelable: true});
    for(const [key, value] of Object.entries(properties)) {
      Object.defineProperty(event, key, {value});
    }
    source.dispatchEvent(event);
    return event;
  }
  return {manager, send, presses, target, documentTarget, pads: value => { pads = value; }};
}

test('devices aggregate without false release and taps preserve both frame edges', () => {
  const f = fixture();
  f.send('keydown', {code: 'Space'});
  f.manager.setVirtual('fire', 1);
  f.send('keyup', {code: 'Space'});
  assert.deepEqual(f.manager.update().actions.fire, {down: true, pressed: true, released: false, value: 1});
  f.manager.setVirtual('fire', 0);
  assert.equal(f.manager.update().actions.fire.released, true);
  f.send('keydown', {code: 'Space'});
  f.send('keyup', {code: 'Space'});
  assert.deepEqual(f.manager.update().actions.fire, {down: false, pressed: true, released: true, value: 0});
  assert.equal(f.presses.length, 2);
  assert.equal(f.manager.update().actions.fire.pressed, false);
  f.send('keydown', {code: 'KeyD'});
  f.pads([{connected: true, axes: [-0.8], buttons: [{value: 0}]}]);
  assert.equal(f.manager.update().axes.move, 1);
  f.manager.destroy();
});

test('contexts, rebind, focus and disconnect release actions without stale holds', () => {
  const f = fixture();
  f.send('keydown', {code: 'Space'});
  f.manager.update();
  f.manager.setContexts(['menu']);
  assert.equal(f.manager.update().actions.fire.released, true);
  f.send('keydown', {code: 'Space'});
  assert.equal(f.manager.update().actions.fire.down, false);
  f.manager.setContexts(['gameplay']);
  f.send('keyup', {code: 'Space'});
  f.send('keydown', {code: 'Space'});
  f.manager.rebind('fire', [{key: 'KeyF'}, {button: 0}]);
  assert.equal(f.manager.update().actions.fire.down, false);
  f.pads([{connected: true, axes: [0], buttons: [{value: 1}]}]);
  assert.equal(f.manager.update().actions.fire.down, true);
  f.pads([]);
  assert.equal(f.manager.update().actions.fire.released, true);
  f.send('keydown', {code: 'KeyF'});
  f.send('blur');
  assert.equal(f.manager.update().actions.fire.down, false);
  f.send('keydown', {code: 'KeyF'});
  f.send('focusin', {target: undefined});
  f.manager.destroy();
  f.send('keydown', {code: 'KeyD'});
  assert.equal(f.manager.update().axes.move, 0);
});

test('typing descendants, gamepad focus suspension and virtual pointer cleanup stay neutral', () => {
  const f = fixture();
  const editable = {closest: selector => selector.includes('select') ? {} : null};
  f.send('keydown', {code: 'Space', target: editable});
  assert.deepEqual(f.presses, []);
  f.send('keydown', {code: 'Space'});
  f.send('focusin', {target: editable}, f.documentTarget);
  f.pads([{connected: true, axes: [0.8], buttons: [{value: 1}]}]);
  assert.equal(f.manager.update().actions.fire.down, false);
  assert.equal(f.manager.update().axes.move, 0);
  f.send('focusin', {}, f.documentTarget);
  assert.equal(f.manager.update().actions.fire.down, false);
  f.pads([{connected: true, axes: [0], buttons: [{value: 0}]}]);
  f.manager.update();
  f.pads([{connected: true, axes: [0], buttons: [{value: 1}]}]);
  assert.equal(f.manager.update().actions.fire.down, true);
  const button = new EventTarget();
  const unbind = f.manager.bindVirtualButton(button, 'right');
  f.send('pointerdown', {pointerId: 1}, button);
  f.send('pointerdown', {pointerId: 2}, button);
  f.send('pointerup', {pointerId: 1}, button);
  assert.equal(f.manager.update().axes.move, 1);
  f.send('lostpointercapture', {pointerId: 2}, button);
  assert.equal(f.manager.update().axes.move, 0);
  f.send('pointerdown', {pointerId: 3}, button);
  unbind();
  assert.equal(f.manager.update().axes.move, 0);
  f.manager.destroy();
});

test('clear cannot resurrect touch holds or keyboard repeat and pointer cancellation releases', () => {
  const f = fixture();
  const button = new EventTarget();
  f.manager.bindVirtualButton(button, 'right');
  f.send('pointerdown', {pointerId: 1}, button);
  f.send('pointerdown', {pointerId: 2}, button);
  f.manager.clear();
  f.send('pointerup', {pointerId: 1}, button);
  assert.equal(f.manager.update().axes.move, 0);
  f.send('keydown', {code: 'Space', repeat: true});
  assert.equal(f.manager.update().actions.fire.down, false);
  f.send('pointerdown', {pointerId: 3, button: 0, clientX: 20, clientY: 30});
  assert.equal(f.manager.update().actions.fire.down, true);
  assert.deepEqual(f.manager.pointer, {x: 20, y: 30, active: true});
  f.send('pointercancel', {pointerId: 3});
  assert.equal(f.manager.update().actions.fire.released, true);
  f.manager.destroy();
});
