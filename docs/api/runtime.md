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
