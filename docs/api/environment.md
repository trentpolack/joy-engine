# Environment API
[API index](README.md) · [Engine guide](../README.md)

Generated from public exports, TypeScript declarations and source documentation with `npm run docs:api`. Implementations remain the source of truth; private methods are omitted.

## EnvironmentSystem
Import from `joy-engine`. [Source](../../src/environment/environment-system.ts)

Advance deterministic world environment state independently from rendering resources. Time values are seconds, directions use a Y-up world space, and colors are linear RGB. The instance owns mutable clock state; returned frames are immutable snapshots.

### constructor
```ts
constructor(options: EnvironmentOptions = {})
```
Validate and copy environment policy into an independently owned simulation.

- **param** [options]

### step
```ts
step(deltaSeconds: number): Readonly<EnvironmentFrame>
```
Advance the environment clock by real elapsed seconds and return its new frame.

- **param** deltaSeconds Nonnegative real elapsed time in seconds.

### setTimeOfDay
```ts
setTimeOfDay(timeOfDay: number)
```
Set normalized local solar time without changing accumulated wind time.

- **param** timeOfDay Normalized time in the half-open [0, 1) day interval.

### sample
```ts
sample(): Readonly<EnvironmentFrame>
```
Produce an immutable environment frame without advancing simulation.
