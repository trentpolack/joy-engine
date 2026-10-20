# Input
The runtime shell (`joy-app`) supplies `InputManager` (`joy-input`) through `frame.input`. Keyboard bindings use `KeyboardEvent.code` names, gamepads use the standard W3C mapping, and native gamepads use gilrs. Games consume the same actions and axes across targets.

Read `pressed(name)` for discrete actions each frame. Actions combine their bindings; axes choose the stronger digital or rescaled analog value. The default analog dead zone is 0.15. `set_contexts` selects active action contexts, and `rebind` changes bindings without changing game rules.

Focus loss, hidden windows, and egui text entry release gameplay input. Gamepads must return to neutral before reactivating. Touch input exposes individual points and stable finger IDs so simultaneous controls can track different touches. Touches also hold the primary pointer button.

Use `Frame::set_pointer_lock(true)` for mouse steering; browsers require a user gesture. `pointer_motion` reports raw frame movement in logical pixels while locked, and `pointer_locked` reports current ownership. The platform can release the lock at any time. Games own coordinate projection, on-screen control layout, and mapping actions into gameplay intent.
