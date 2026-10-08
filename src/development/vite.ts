// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Node-only entry point for Vite configuration files (`joy-engine/development/vite`).
// Every plugin here is development tooling: persistence endpoints apply only to
// `vite serve`, and none of these modules may be imported by browser runtime code.

// Live tuning persistence for a project's config document.
export { createConfigPlugin } from './config-plugin.ts';

// Live post-processing profile persistence for a project's level document.
export { createPostProcessingPlugin } from './post-processing-plugin.ts';

// Shared PNG favicon serving and packaging.
export { createFaviconPlugin } from './favicon-plugin.ts';
