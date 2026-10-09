# Core API
[API index](README.md) · [Engine guide](../README.md)

Generated from public exports, TypeScript declarations and source documentation with `npm run docs:api`. Implementations remain the source of truth; private methods are omitted.

## createSeededRandom
Import from `joy-engine`. [Source](../../src/core/random.ts)

```ts
function createSeededRandom(seed: number): () => number
```
Create an independently owned deterministic random stream in [0, 1). Keep separate instances for gameplay and presentation so changing cosmetic draw counts cannot alter simulation decisions. This is not cryptographic.

- **param** seed Finite integer, normalized to an unsigned 32-bit seed.
- **returns** Source retaining only its own sequence state.


## createConfigDocumentValues
Import from `joy-engine`. [Source](../../src/core/config-values.ts)

```ts
function createConfigDocumentValues(fields: readonly ConfigField[], document: unknown): Record<string, number>
```
Validate a serialized version-1 override document and return complete values.

- **param** fields Numeric configuration contract.
- **param** document Untrusted parsed JSON value.
- **returns** Independently owned complete values.


## createConfigSchema
Import from `joy-engine`. [Source](../../src/core/config-values.ts)

```ts
function createConfigSchema(fields: readonly ConfigField[]): object
```
Produce the strict JSON Schema used by editors and persistence boundaries. Values remain optional because override files only store authored differences.

- **param** fields Numeric configuration contract.
- **returns** Independently owned JSON Schema draft 2020-12 record.


## createConfigValues
Import from `joy-engine`. [Source](../../src/core/config-values.ts)

```ts
function createConfigValues(fields: readonly ConfigField[], overrides: Readonly<Record<string, number>> = {}): Record<string, number>
```
Validate field metadata and merge partial overrides into a new complete record.

- **param** fields Numeric configuration contract.
- **param** [overrides] Partial saved or runtime values.
- **returns** Independently owned complete values.


## TAU
Import from `joy-engine`. [Source](../../src/core/math.ts)

```ts
const TAU
```


## clamp
Import from `joy-engine`. [Source](../../src/core/math.ts)

```ts
function clamp(value: number, minimum: number, maximum: number): number
```
Constrain a number to an inclusive range.

- **param** value
- **param** minimum
- **param** maximum


## directionBetween
Import from `joy-engine`. [Source](../../src/core/math.ts)

```ts
function directionBetween(first: Vector2, second: Vector2): {x: number; y: number; length: number}
```
Return a unit direction and distance between two x/y positions. Coincident positions return a zero direction and a fallback length of 1, preserving a safe divisor for callers that normalize using the returned length.

- **param** first
- **param** second


## distanceBetween
Import from `joy-engine`. [Source](../../src/core/math.ts)

```ts
function distanceBetween(first: Vector2, second: Vector2): number
```
Measure Euclidean distance between two points in the caller's coordinate space.

- **param** first
- **param** second


## distanceToRectangleEdge
Import from `joy-engine`. [Source](../../src/core/math.ts)

```ts
function distanceToRectangleEdge(width: number, height: number, angle: number): number
```
Return the distance from a rectangle's center to its edge along a ray.

- **param** width Positive rectangle width in logical units.
- **param** height Positive rectangle height in logical units.
- **param** angle Ray direction in radians from +X.
- **returns** Distance along the ray in the same logical units.


## randomInteger
Import from `joy-engine`. [Source](../../src/core/math.ts)

```ts
function randomInteger(minimum: number, maximumExclusive: number, random: () => number = Math.random): number
```
Return an integer in [minimum, maximumExclusive); bounds should be integers.

- **param** minimum
- **param** maximumExclusive
- **param** [random] Injected source; defaults to gameplay Math.random.


## randomRange
Import from `joy-engine`. [Source](../../src/core/math.ts)

```ts
function randomRange(minimum: number, maximum: number, random: () => number = Math.random): number
```
Sample a floating-point number from the half-open requested range.

- **param** minimum
- **param** maximum
- **param** [random] Injected source; defaults to gameplay Math.random.


## parseHexColor
Import from `joy-engine`. [Source](../../src/core/color.ts)

```ts
function parseHexColor(value: string | undefined): Color | null
```
Decode #RGB or #RRGGBB notation into normalized RGBA. Unsupported forms return null so callers can select their own fallback.

- **param** value The hex color string to parse.
- **returns** The parsed RGBA color or null if the input is invalid.


## withAlpha
Import from `joy-engine`. [Source](../../src/core/color.ts)

```ts
function withAlpha(color: Color, alpha: number): Color
```
Return a new straight-alpha color without mutating the borrowed palette color.

- **param** color The original RGBA color.
- **param** alpha The new alpha value.
- **returns** A new RGBA color with the specified alpha.
