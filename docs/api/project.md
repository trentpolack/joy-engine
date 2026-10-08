# Project API
[API index](README.md) · [Engine guide](../README.md)

Generated from public exports, TypeScript declarations and source documentation with `npm run docs:api`. Implementations remain the source of truth; private methods are omitted.

## parseProjectManifest
Import from `joy-engine/project-manifest`. [Source](../../src/project/project-manifest.ts)

```ts
function parseProjectManifest(value: unknown): ProjectManifest
```
Validate and copy a project-owned editor manifest. Paths remain relative to the selected project directory; consumers must enforce that boundary when reading. Optional $schema metadata is preserved for authoring tools, never fetched at runtime.

- **param** value
