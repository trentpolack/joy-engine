# Scene effects verification — 2026-10-02

Implementation starts at develop `6f8afe6edb4e998173aaa5147eb1345c7a037235` on isolated `feature/scene-effects-v1`. The separate widget-theme branch and main checkout's point-light changes were preserved.

During verification, remote and local develop advanced to `fc41b618b05281e8dcbe4c4c867cefc8c53ba3ee`. GitHub comparison confirms exactly one comments/formatting-only point-light change, with no runtime changes. The feature commits are rebased locally onto that current tip before delivery; no develop checkout is modified.

## Native Mac capture

Host hardware model was separately verified as MacBook Pro `Mac16,6`; no serial or device identifier was retained.

Production `npm run capture --workspace @joy-engine/example-scene-effects -- --built` passed 54 scenarios, 27 each on WebGL and WebGPU. Native installed headed Chrome 153.0.8010.37 ran on Apple M4 Max (32 GPU cores, 14 logical CPU cores, 36 GiB), Darwin 27.0.0. WebGPU adapter reports Apple / Metal 3; WebGL uses ANGLE Metal on Apple M4 Max. Browser viewport 1440×1000; baseline scene backing 1130×1000, pixel ratio 1; 25 meshes and 27 point lights. Camera clips 0.5/100.

Local gallery: `artifacts/captures/1790973832029-scene-effects/index.html`; complete settings, hardware, browser, backend, camera and distributions: adjacent `report.json`. These ignored captures remain in the execution worktree. Each scenario warms up 60 frames, then records 120 CPU submission samples and 119 RAF intervals. Individual effect comparisons use medium quality, frozen simulation time 2 seconds. Motion comparisons advance actual object-only or camera-only animation. Combined uses GTAO, SSGI, TAA, SSR, clustered lighting, adaptive exposure and HDR bloom; incompatible SSAO is disabled.

| WebGPU configuration | CPU submission mean (ms) | CPU p95 (ms) | RAF mean (ms) |
| --- | ---: | ---: | ---: |
| Baseline | 0.456 | 0.600 | 8.334 |
| Depth-aware blur | 0.547 | 0.700 | 8.325 |
| SSAO | 0.596 | 0.800 | 8.332 |
| GTAO | 0.786 | 0.900 | 8.326 |
| SSGI | 0.753 | 0.900 | 8.341 |
| Outlines | 0.492 | 0.600 | 8.339 |
| TAA | 0.596 | 0.700 | 8.335 |
| Motion blur, frozen | 0.551 | 0.700 | 8.339 |
| SSR | 0.683 | 0.800 | 8.334 |
| Height fog | 0.485 | 0.600 | 8.325 |
| Clustered lighting | 0.686 | 0.900 | 8.334 |
| Adaptive exposure | 0.544 | 0.700 | 8.333 |
| HDR bloom | 0.628 | 0.800 | 8.328 |
| Combined low | 1.683 | 1.900 | 8.332 |
| Combined medium | 1.737 | 1.900 | 8.339 |
| Combined high | 1.839 | 2.100 | 8.333 |
| Object motion baseline / blur | 0.456 / 0.516 | 0.600 / 0.700 | 8.329 / 8.333 |
| Camera motion baseline / blur | 0.407 / 0.505 | 0.500 / 0.700 | 8.333 / 8.334 |

These are short sequential CPU measurements, not GPU execution measurements or a claim of game-wide FPS. Native animation cadence remained around 8.33 ms; actual GPU time was not measured. The high combined preset adds about 1.38 ms CPU submission in this scene. GPU cost cannot be inferred from that difference or cadence.

Screenshots were inspected: contact AO, red/green bounce, floor/block reflections, outline edges, compact fog tint and bloom are visible; combined is softer/darker than baseline. TAA softens thin edges. Motion blur is subtle at the room's gentle motion and native cadence; screenshots alone do not establish its temporal quality. Contract tests and moving captures establish valid signed inputs without claiming a complete ghosting/disocclusion quality assessment.

## Checks and limits

The final test run passed 389 tests. Actual Level Editor and Vehicle Playground development pages also passed native Chrome WebGPU/WebGL checks: all eleven scene toggles, AO exclusivity, presets/quality, bloom settings preservation and unsupported-backend reporting, with zero page/console errors. Controls gallery: `artifacts/captures/1790974094034-scene-controls-integration/index.html`; final WebGL status screenshot: `artifacts/captures/1790974179233-scene-controls-integration/vehicle-webgl-panel.png`.

Review corrected complete-display replacement when bloom/quality changes and late controls construction after Vehicle teardown. Focused native regression then passed 21 transitions across editor WebGPU and Vehicle WebGPU/WebGL with deliberately nondefault exposure, contrast, ACES, bloom threshold/strength/radius, saturation, vibrance, brightness, antialiasing, vignette and grain. Every field stayed equal except the explicitly changed bloom strength/quality. Gallery, complete records and reproducible helper: `artifacts/captures/1790974414805-scene-controls-display-regression/index.html`.

The high combined screenshot was saved to Library as `/webgpu-combined-high.png`, native ID `libfile_ed94fdb5849c81919b4f0ae4c60c9f43`, file ID `file_00000000ed1881f595996873a448b1a4`; local identity metadata was applied successfully. Other screenshots and galleries remain local.

Root type/JavaScript checks, all unit/workspace/editor tests and all production builds passed. Blast Garden, God Game, Luna, Magic Colors, Space Scuffles and Vehicle Playground production browser captures passed. Root `npm run validate` failed at Form Lab's responsive horizontal-overflow assertion (`test/browser.mjs:515`) and Particle Lab's 900-pixel overflow assertion (`test/browser.mjs:240`); baseline comparison is recorded below. An earlier sandbox-only test run could not listen on localhost; the authorized run used host permission and completed the test stage.

Both overflow failures reproduce without the effects changes on an isolated exact develop `6f8afe6` archive. Native Chrome initial Form Lab bounds at 320 CSS pixels have document scroll width 362 pixels; its 820/390 layouts fit. The original unchanged Particle Lab production build/capture fails at the same `test/browser.mjs:240` assertion after its stateful editing sequence; its initial 900-pixel layout fits. Logs, bounds JSON, screenshots and the initial-viewport reproduction helper are retained in `artifacts/verification/scene-effects/baseline-overflow/`. These existing lab layout defects remain outside this rendering scope.

Native Safari 27.0.1 WebDriver was available but refused sessions because “Allow remote automation” is disabled. No Safari GPU/visual result is claimed; settings were preserved. Microsoft Edge is absent. No device-loss recovery or GPU timestamp query was tested. WebGL renders the base scene and existing HDR/display path while effect controls diagnose unsupported scene passes.

An explicit `npm run build:editor` was refused by the repository's `productionEnabled: false` policy. That setting remains unchanged. The editor was type-checked and exercised through its actual development page; root release builds intentionally skip its production output. Build logs contain existing large-chunk warnings.
