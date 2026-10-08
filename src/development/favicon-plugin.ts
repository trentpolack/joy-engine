// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { IncomingMessage, ServerResponse } from 'node:http';
import type { Plugin, ResolvedConfig, ViteDevServer } from 'vite';

import { readFileSync } from 'node:fs';

// Served and emitted beside each app's index; browsers request it relative to the Vite base.
const FAVICON_FILE = 'favicon.png';

/**
 * Serve and package a PNG site icon for an independently hosted Vite app. The icon is
 * read once at configuration time; callers own the file and pass its absolute path, so
 * games, labs and the editor can share one implementation with different branding.
 * @param options iconPath is an absolute path to a PNG file.
 */
export function createFaviconPlugin({ iconPath }: { iconPath: string }): Plugin {
  const source = readFileSync(iconPath);
  let base = '/';

  return {
    name: 'joy-shared-favicon',
    configResolved(config: ResolvedConfig) {
      base = config.base;
    },
    configureServer(server: ViteDevServer) {
      server.middlewares.use((request: IncomingMessage, response: ServerResponse, next: () => void) => {
        const pathname = new URL(request.url ?? '/', 'http://localhost').pathname;
        if(pathname !== `${base}${FAVICON_FILE}`) {
          next();
          return;
        }
        response.setHeader('Content-Type', 'image/png');
        response.end(source);
      });
    },
    generateBundle() {
      this.emitFile({type: 'asset', fileName: FAVICON_FILE, source});
    },
    transformIndexHtml() {
      return [{
        tag: 'link',
        attrs: {rel: 'icon', type: 'image/png', href: `${base}${FAVICON_FILE}`},
        injectTo: 'head'
      }];
    }
  };
}
