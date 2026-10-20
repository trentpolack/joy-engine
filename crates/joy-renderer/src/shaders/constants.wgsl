// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Constants every composed scene module shares. The renderer prepends this file to scene-common.wgsl and to the
// shadow-caster modules, which compile shading models without the scene's frame and shadow groups.

const PI: f32 = 3.14159265359;
