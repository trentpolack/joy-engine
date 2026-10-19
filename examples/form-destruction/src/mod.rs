// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The FORM destruction example's modules, beside their TypeScript counterparts: tuning (`config/`), the seeded
//! structure (`structure.ts`), its integrity and support views (`structure-view.ts`), rigid debris
//! (`destruction-debris.ts`), impact markers (`impact-debug.ts`), the HUD, and the demo itself (`site/src/main.ts`).
//! `main.rs` and the integration tests in `tests/integration/` both include this tree.

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
