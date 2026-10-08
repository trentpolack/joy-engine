// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Node entry point for engine-source consumers outside Vite. Engine modules import
// authored shaders, styles and assets with Vite's `?raw` suffix; register these hooks
// with `node --import joy-engine/testing/register-raw-loader` before loading them.
import { registerHooks } from 'node:module';
import { resolve, load } from './raw-loader.mjs';

registerHooks({resolve, load});
