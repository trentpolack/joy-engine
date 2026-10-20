// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The FORM destruction example's modules: tuning (`config`), the seeded structure (`structure`), its integrity and
//! support views (`structure_view`), rigid debris (`destruction_debris`), impact markers (`impact_debug`), the HUD
//! (`hud`), and the demo itself (`destruction_demo`). `main.rs` and the integration tests in `tests/integration/` both
//! include this tree.

/// Named tuning values.
pub mod config {
    pub mod destruction_config;
}
pub mod destruction_debris;
pub mod destruction_demo;
pub mod hud;
pub mod impact_debug;
pub mod structure;
pub mod structure_view;
