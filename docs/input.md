# Input
`InputManager` owns one player's keyboard, pointer, gamepad and virtual controls. Import it and the `InputAction`, `InputAxis`, `InputBinding` and `InputSnapshot` JSDoc types from `joy-engine`. Each instance owns subscriptions and copies its definitions; the browser targets and virtual buttons remain borrowed. Call `destroy()` before disposing those targets.

```js
const input = new InputManager({
  pointerTarget: canvas,
  actions: {
    left: {bindings: [{key: 'KeyA'}, {virtual: 'left'}]},
    right: {bindings: [{key: 'KeyD'}]},
    jump: {bindings: [{key: 'Space'}, {pointer: 0}, {button: 0}]}
  },
  axes: {move: {positive: 'right', negative: 'left', gamepadAxis: 0}},
  onPress: name => { if(name === 'jump') jump(); }
});
input.bindVirtualButton(leftButton, 'left');
const frame = input.update();
// frame.actions.jump: {down, pressed, released, value}
// frame.axes.move: -1 through 1
```

Call `update()` once per simulation frame. It polls all connected pads and consumes accumulated edges. A tap that starts and ends between frames reports both `pressed` and `released`; holding another binding prevents a false release. `onPress` runs immediately for keyboard/pointer/virtual events and at polling time for gamepads, preserving short taps without browser key repeat. Use either the callback or the frame's pressed edge for a discrete action, not both. State snapshots are copies; pointer coordinates are client pixels and must be projected by the game.

Actions combine bindings using their maximum value. Axes subtract negative from positive digital actions, then choose the strongest gamepad stick contribution, clamped to [-1, 1]. Stick dead zones default to 0.15 and rescale the remaining range. Gamepad button and axis indexes follow the browser's mapping; no controller-specific remapping is assumed. `rebind(name, bindings)` replaces copied bindings at runtime; persist binding preferences using the application's own storage boundary.

Actions and axes default to the `gameplay` context. Declare `contexts: ['menu']` for menu-only controls and call `setContexts(['menu'])`, `setContexts(['gameplay'])`, or an empty array to disable input. Context changes, rebinding, blur, hidden documents and editable focus release held controls. Inputs, textareas, selects, contenteditable descendants and textbox roles suppress gameplay across devices. Gamepad input must return to neutral after a release boundary, preventing a held stick from immediately resuming gameplay. Keyboard repeats cannot resurrect a cleared hold.

`setVirtual(name, value)` accepts finite [0, 1] values. `bindVirtualButton(element, name)` supplies multi-pointer capture and release/cancel/lost-capture handling, returns an idempotent unsubscribe, and is automatically removed on destruction. Use a distinct virtual name per button. Tests may inject EventTargets and a gamepad polling adapter; no browser globals are needed when all targets and the adapter are supplied.

Vehicle Playground and Magic Colors use this manager while retaining their keyboard and touch labels. Their project wrappers own world-space aim and gameplay callbacks; the engine owns device aggregation and lifetime management.
