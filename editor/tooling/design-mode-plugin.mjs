// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import {appendFile, realpath} from 'node:fs/promises';
import {relative, resolve} from 'node:path';

const ENDPOINT = '/__joy-design/css';

/** Creates the development-only CSS persistence endpoint used by Joy design mode. */
export function createDesignModePlugin() {
  let root = '';
  return {
    name: 'joy-design-mode',
    apply: 'serve',
    configResolved(config) {
      root = config.root;
    },
    configureServer(server) {
      server.middlewares.use(ENDPOINT, async (request, response) => {
        if(request.method !== 'PATCH') {
          response.statusCode = 405;
          response.end();
          return;
        }
        try {
          const payload = JSON.parse(await readBody(request));
          validatePayload(payload);
          const stylesheetUrl = new URL(payload.href);
          const path = stylesheetUrl.pathname.startsWith('/@fs/')
            ? stylesheetUrl.pathname.slice('/@fs'.length)
            : resolve(root, `.${stylesheetUrl.pathname}`);
          const [resolvedPath, resolvedRoot] = await Promise.all([realpath(path), realpath(root)]);
          const relativePath = relative(resolvedRoot, resolvedPath);
          if(relativePath.startsWith('..') || relativePath === '' || !resolvedPath.endsWith('.css')) {
            throw new Error('Design mode can only update CSS beneath the active Vite root.');
          }
          const declarations = Object.entries(payload.declarations).map(([name, value]) => `${name}: ${value};`).join(' ');
          await appendFile(resolvedPath, `\n${payload.selector} { ${declarations} }\n`);
          response.statusCode = 204;
          response.end();
        } catch(error) {
          response.statusCode = 400;
          response.end(error instanceof Error ? error.message : 'Invalid design update.');
        }
      });
    }
  };
}

function readBody(request) {
  return new Promise((resolve, reject) => {
    let body = '';
    request.setEncoding('utf8');
    request.on('data', chunk => body+= chunk);
    request.on('end', () => resolve(body));
    request.on('error', reject);
  });
}

function validatePayload(payload) {
  if(!payload || typeof payload.href !== 'string' || typeof payload.selector !== 'string') {
    throw new Error('Design update requires a stylesheet and selector.');
  }
  if(/[{}\r\n]/.test(payload.selector) || payload.selector.length > 500) {
    throw new Error('Design update contains an invalid selector.');
  }
  const declarations = Object.entries(payload.declarations ?? {});
  if(declarations.length === 0 || declarations.some(([name, value]) => name !== 'width' || !/^\d{1,5}px$/.test(value))) {
    throw new Error('Design update contains an unsupported declaration.');
  }
}
