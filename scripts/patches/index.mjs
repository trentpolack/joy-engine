// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Node-only tooling for standalone repositories and hosting workspaces. Hosts supply their own paths and boundaries.
export { applyProjectPatch } from './apply-project-patch.mjs';
export { fetchProjectPatch } from './fetch-project-patch.mjs';
export { createPatchTarget } from './patch-delivery.mjs';
