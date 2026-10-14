# Dependency Layers
Joy Engine source follows a one-way dependency model so simulation remains runnable without a DOM, canvas, or GPU:

1. **Foundation** contains shared data and algorithms.
2. **Simulation** contains environment, entity, gameplay, and physics state.
3. **Content** contains asset loading, compilation, and scripting.
4. **Rendering** contains GPU presentation and render features.
5. **Platform** contains browser, development, and frame-loop integrations.

Higher layers may import the allowed lower layers recorded in `architecture/layers.json`. The checker reads static imports, re-exports, and literal dynamic imports, and rejects new upward edges. Existing upward edges are named individually as migration debt; removing one also requires removing its now-stale exception. This makes the allowlist shrink during the renderer migration rather than silently becoming a permanent architectural bypass.

`World` is the headless composition boundary for entities and physics. Rendering observes simulation through a separate application integration and must not become owned by `World`.
