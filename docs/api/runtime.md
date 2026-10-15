# Runtime API
[API index](README.md) · [Engine guide](../README.md)

Generated from public exports, TypeScript declarations and source documentation with `npm run docs:api`. Implementations remain the source of truth; private methods are omitted.

## startGameLoop
Import from `joy-engine`. [Source](../../src/runtime/game-loop.ts)

```ts
function startGameLoop({ update, render, updatesPerSecond = DEFAULT_UPDATE_RATE }: GameLoopOptions): () => void
```
Run simulation at a fixed rate and render once per animation frame. Update receives elapsed seconds; render receives the frame timestamp in milliseconds and the remaining fraction of a simulation step for interpolation.

- **param** options Configuration options for the game loop.
- **returns** A function to stop the game loop.


## GameShell
Import from `joy-engine`. [Source](../../src/runtime/game-shell.ts)

Own one browser game's configuration, renderer, input, diagnostics, phased frame loop, and page lifecycle. Factories transfer ownership of their returned resources to the shell. Frame phases run in input, simulation, presentation, render, and after-frame order, using seconds for elapsed time and the animation-frame clock.

### constructor
```ts
constructor(options: GameShellOptions<TConfiguration, TRenderer, TInput>)
```

### isDisposed
```ts
get isDisposed(): boolean
```
Whether this shell has begun its terminal cleanup path.


### initialize
```ts
async initialize(): Promise<void>
```
Create each owned resource in dependency order, initialize the application, and start rendering.


### resetClock
```ts
resetClock(): void
```
Reset elapsed time before resuming simulation after a pause or visibility change.


### setConfiguration
```ts
setConfiguration(configuration: TConfiguration): void
```
Replace the configuration record supplied to subsequent phases. Ownership remains with the application.


### dispose
```ts
dispose(): void
```
Stop scheduling frames and release application, input, renderer, diagnostics, and listeners in dependency order. Repeated disposal is harmless.
