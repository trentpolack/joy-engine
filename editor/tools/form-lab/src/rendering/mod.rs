// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The preview through the engine renderer: the viewport, its studio lighting, point markers, and inspection overlay.
//! Ported from `src/rendering/*.ts`; the TypeScript preview shaders and triangle batches are gone.

pub mod batches;
pub mod inspection_overlay;
pub mod preview_lighting;
pub mod viewport;
