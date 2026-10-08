# Browser API
[API index](README.md) · [Engine guide](../README.md)

Generated from public exports, TypeScript declarations and source documentation with `npm run docs:api`. Implementations remain the source of truth; private methods are omitted.

## AudioBank
Import from `joy-engine`. [Source](../../src/browser/audio-bank.ts)

Owns one Web Audio context, decoded samples, and bounded overlapping voices. Await `create()` before gameplay, call `resume()` directly from a user gesture, and call `destroy()` when the owning session ends. Playback never loads or decodes assets and does not queue sounds while the context is suspended.

### create
```ts
static async create(definitions: Record<string, SoundDefinition>, options: AudioBankOptions = {}): Promise<AudioBank>
```
Fetch and decode all samples before returning. Rejects with the sound name on load failure and releases the context on any initialization failure.

- **param** definitions Borrowed configuration, not mutated.
- **param** [options]

### activeVoiceCount
```ts
get activeVoiceCount()
```
Number of currently retained playback voices.


### resume
```ts
resume(): Promise<boolean>
```
Unlock audio directly inside a pointer, keyboard, or click handler. May be called again after a browser/OS interruption. Rejects if browser activation fails; returns false after disposal. Each gesture retries native activation, even if an earlier request is still pending while playback is disallowed.


### play
```ts
play(name: string): boolean
```
Start a sound immediately; false means unknown, suspended, or disposed. Samples are reused; Web Audio requires a new, inexpensive source per play. At capacity, retire the oldest matching sound, then the oldest bank voice.

- **param** name
- **returns** Whether playback started.

### destroy
```ts
destroy()
```
Stop voices, discard samples, and close the owned context. Idempotent.



## InputController
Import from `joy-engine`. [Source](../../src/browser/input-controller.ts)

Tracks held keys and a pointer, with optional handlers for discrete actions. Pointer coordinates stay in client pixels; games with inset or scaled arenas must convert them to their own coordinate space. Call destroy when disposing.

### constructor
```ts
constructor(pointerTarget: HTMLElement | Window = window)
```
Register owned listeners on the borrowed pointer target and window.

- **param** pointerTarget

### isDown
```ts
isDown(...codes: string[])
```
Test whether any requested KeyboardEvent.code is currently held.

- **param** codes

### bind
```ts
bind(code: string, handler: (event: KeyboardEvent | PointerEvent) => void)
```
Assign one action handler, replacing any previous binding for the same code.

- **param** code KeyboardEvent.code or PointerDown.
- **param** handler Receives the original browser event, including key repeats.

### destroy
```ts
destroy()
```
Remove owned listeners; the borrowed pointer target remains in place.



## requireElement
Import from `joy-engine`. [Source](../../src/browser/dom.ts)

```ts
function requireElement<T extends HTMLElement>(root: Document | HTMLElement, selector: string, elementType: {
    new (...args: never[]): T;
}): T
```
Require a matching HTML element during startup, with a useful markup error.

- **param** root
- **param** selector
- **param** elementType


## DevTools
Import from `joy-engine`. [Source](../../src/browser/diagnostics/dev-tools.ts)

Own the development summary, detail panel, and their event listeners. Games supply runtime metadata and one animation-frame timestamp per render. A disabled instance creates no DOM or listeners and emits no logs. The owner must call destroy() on disposal; destroying one instance leaves others intact.

### constructor
```ts
constructor({ title = 'JOY DEV', enabled = true, sampleWindowMilliseconds = 500 }: DevToolsOptions = {})
```
- **param** [options] Runtime identity and sampling configuration.

### frame
```ts
frame(timestamp: number)
```
Sample the interval between rendered frames, including time spent waiting for the next frame. This is frame cadence, not CPU work or GPU execution time.

- **param** timestamp Monotonic requestAnimationFrame time in milliseconds.

### set
```ts
set(name: string, value: string | number)
```
Add or replace a display value without retaining the caller's mutable data.

- **param** name Stable detail label, such as renderer or viewport.
- **param** value Current value, copied as text.

### setRenderer
```ts
setRenderer(renderer: RendererDiagnosticSource | null)
```
Borrow an optional renderer for on-demand detail reads; no subscriptions are added. Disabled instances do not retain it. Pass null to detach before replacing its owner.


### log
```ts
log(message: string, details?: unknown)
```
Emit an intentional startup or lifecycle message in the browser console.

- **param** message Human-readable event description.
- **param** [details] Optional browser-inspectable diagnostic data.

### togglePanel
```ts
togglePanel()
```
Toggle runtime details through the same path for pointer and keyboard input.


### destroy
```ts
destroy()
```
Release owned DOM, styles, and listeners. Repeated calls are harmless.



## InputManager
Import from `joy-engine`. [Source](../../src/browser/input/input-manager.ts)

Own device state and subscriptions for one player. Actions aggregate by maximum value; axes choose the strongest digital or analog input. Call update once per simulation frame to consume edges, including taps completed between frames.

### constructor
```ts
constructor({actions, axes = {}, target = window, documentTarget = document, pointerTarget = target,
    getGamepads = () => navigator.getGamepads(), onPress = () => {}}: InputManagerOptions)
```
- **param** options
- **param** options.actions Copied binding definitions.
- **param** [options.axes] Copied axis definitions.
- **param** [options.target] Borrowed keyboard/window event source.
- **param** [options.documentTarget] Borrowed focus/visibility source.
- **param** [options.pointerTarget] Borrowed pointer surface.
- **param** [options.getGamepads] Poll adapter; all connected pads contribute.
- **param** [options.onPress] Immediate edge callback; never repeats while held.

### suspended
```ts
get suspended()
```
Whether focus or visibility prevents all gameplay devices from contributing.


### update
```ts
update(): InputSnapshot
```
Poll pads and return independently owned action/axis snapshots, consuming edges. Analog sticks use a rescaled dead zone (default 0.15). Values have no time units.


### setContexts
```ts
setContexts(contexts: readonly string[])
```
Replace active contexts and release previous holds. Pads must return to neutral.

- **param** contexts Empty disables all actions.

### rebind
```ts
rebind(name: string, bindings: readonly InputBinding[])
```
Replace one action's copied bindings; old device holds are released.

- **param** name
- **param** bindings

### setVirtual
```ts
setVirtual(name: string, value: number)
```
Set a named virtual control (0 releases, 1 fully presses); supports touch adapters.

- **param** name
- **param** value Finite value between zero and one.

### bindVirtualButton
```ts
bindVirtualButton(button: EventTarget & {
    setPointerCapture?: (id: number) => void;
}, name: string): () => void
```
Bind a borrowed touch button with pointer capture and multi-touch ownership.

- **param** button
- **param** name Virtual binding name; use one button per name.
- **returns** Idempotent unbind, also called by destroy.

### clear
```ts
clear()
```
Release all holds; gamepads must pass through neutral before they resume.


### destroy
```ts
destroy()
```
Release state and all owned listeners; repeated calls are harmless.
