# Console and Profiling
Development builds open the console with the backquote key: a drop-down command line over the game, after Unreal Engine's. Production builds (the shell's `development` option off) never open it and never apply console variables.

## Console Variables
Console variables are named, typed settings (`development::ConsoleVariables`): on/off, whole numbers or numbers within a range, one of a set of choices, or text. Names compare without regard to case. Type a variable's name to print its value, what it accepts, and its help; follow it with a value to set it. Values outside a variable's kind or range are refused and leave it unchanged.

The engine registers its rendering variables (`development::render_variables`), which the shell applies to the renderer each frame before the game renders and then mirrors from the renderer, so the console reports what actually runs, including changes a game makes itself:

| Variable | Accepts | Effect |
| --- | --- | --- |
| `r.ScreenPercentage` | 25 to 100 | Scene resolution per axis (`Renderer::set_render_scale`). |
| `r.DebugView` | `lit`, `baseColor`, `roughness`, ... | What mesh surfaces draw as (`DebugView::name`). |
| `r.Profile` | 0 or 1 | Frame profiling (see below). |
| `r.Deferred` | 0 or 1 | Deferred lighting where the device supports it; off shades every surface forward (`Renderer::set_deferred`). |
| `r.Atmosphere` | 0 or 1 | The procedural sky. |
| `r.Shadows` | 0 or 1 | Directional and point-light shadows. |
| `r.TAA`, `r.MotionBlur`, `r.AdaptiveExposure`, `r.Volumetrics` | 0 or 1 | Each scene effect. |

Turning shadows or a scene effect off parks its settings, and turning it back on restores them rather than the defaults. Games register their own variables through `Frame::dev_tools` (`DevTools::console_variables`) with a prefix of their own, read them each frame, and take their own changes with `ConsoleVariables::take_changes(prefix)`; the engine takes only `r.` changes.

## Commands
- `help` lists commands; `find <text>` lists variables whose name or help contains the text, with their values.
- `reset <variable>` returns a variable to its default; `clear` empties the output.
- `stat unit` shows frame, CPU, and GPU time; `stat gpu` shows each frame phase's GPU and CPU time; `stat none` hides both. Profiling stays on while either shows.
- Tab completes names, Up and Down recall earlier commands, and Escape or backquote closes the console.

## Profiling
`Renderer::set_profiling` (or `r.Profile`) records a `FrameProfile` per frame: each phase the frame ran (prepare, shadows, volumetrics, depth pre-pass, G-buffer, deferred lighting, scene, fog, transparency, composite effects, and post-processing) with the CPU time spent recording it and, where the device supports timestamp queries (`GpuCapabilities::timestamps`), the GPU time it took. GPU timestamps are written by empty timestamped compute passes at phase boundaries, which WebGPU allows as well as native backends do, and read back after the frame's commands finish, so `Renderer::frame_profile` describes a frame a few frames old and never stalls the CPU. WebGL 2 has no timestamp queries; its profiles carry CPU times only. Browsers may coarsen timestamps for privacy, so compare GPU times on the backend that ships.

The STATS panel in the development dock shows the same timings and a Rendering section with the debug view, profiling, and deferred lighting switches, written through the same console variables. Its GPU Resources section shows the frame's lighting path (deferred lighting passes and forward-shaded batches), drawn and omitted point lights, light-cluster references, and shadow casters, point lights, and views. `r.DebugView lightComplexity` shows how many lights reach each surface; see [lighting and shadows](lighting.md).
