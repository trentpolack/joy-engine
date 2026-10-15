# joy-engine
The crate games and tools depend on. It re-exports Joy Engine's Rust layers (`foundation`, `simulation`, `form`, `render`, and `platform`), egui for HUDs, and a `prelude` with the names most games use. Games never depend on wgpu, winit, rapier, or egui directly.

```rust
use joy_engine::prelude::*;

struct MyGame;

impl Game for MyGame {
    fn render(&mut self, _frame: &mut Frame<'_>, target: &mut RenderTarget<'_>) -> GameResult {
        Ok(target.draw(&RenderScene::new(), &OrbitCamera::default(), &FrameInputs::default())?)
    }
}

fn start() {
    if let Err(error) = run(ShellOptions::default(), MyGame) {
        joy_engine::log::error!("{error}");
    }
}

joy_engine::web_main!(start);
```

Build the crate as `crate-type = ["cdylib", "rlib"]` for browsers and use `scripts/rust/build-web.mjs` to produce a static page. `cargo run -p joy-engine --example form-viewer` shows FORM assets drawn by the engine renderer with live parameters.
