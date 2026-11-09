// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// A small static file server for built browser output (`dist/`), used by previews and captures.
//
// Browser builds are plain files (HTML, JavaScript, WebAssembly, and assets), so serving them needs no bundler. The
// server only reads files beneath its root, answers GET and HEAD, serves `index.html` for directories, and sends the
// WebAssembly media type so browsers can stream-compile `.wasm` modules. The caller owns the returned server and must
// call `close()` when finished.

import { createReadStream } from 'node:fs';
import { stat } from 'node:fs/promises';
import { createServer } from 'node:http';
import { extname, join, normalize, resolve, sep } from 'node:path';

/** Media types for the files browser builds emit; anything else is served as bytes. */
const MEDIA_TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.wasm': 'application/wasm',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.webp': 'image/webp',
  '.svg': 'image/svg+xml',
  '.ico': 'image/x-icon',
  '.wav': 'audio/wav',
  '.ogg': 'audio/ogg',
  '.mp3': 'audio/mpeg',
  '.txt': 'text/plain; charset=utf-8'
};

/** The fallback media type for unknown extensions. */
const BINARY_MEDIA_TYPE = 'application/octet-stream';

/** The document served for directory requests. */
const INDEX_DOCUMENT = 'index.html';

/**
 * A running static server.
 * @typedef {object} StaticServer
 * @property {string} url The server's base URL, ending in `/`.
 * @property {() => Promise<void>} close Stops accepting connections and resolves once the server is closed.
 */

/**
 * Serve the files beneath `root` on `host` and `port` (0 picks a free port).
 * @param {{ root: string, host?: string, port?: number }} options
 * @returns {Promise<StaticServer>}
 */
export async function startStaticServer({ root, host = '127.0.0.1', port = 0 }) {
  const rootDirectory = resolve(root);
  const server = createServer((request, response) => {
    handleRequest(rootDirectory, request, response).catch(error => {
      if(!response.headersSent) {
        response.writeHead(500, { 'Content-Type': 'text/plain; charset=utf-8' });
      }
      response.end(`Static server error: ${error.message}`);
    });
  });

  await new Promise((resolveListen, rejectListen) => {
    server.once('error', rejectListen);
    server.listen(port, host, () => {
      server.off('error', rejectListen);
      resolveListen();
    });
  });

  const address = server.address();
  const displayHost = address.family === 'IPv6' ? `[${address.address}]` : address.address;
  return {
    url: `http://${displayHost}:${address.port}/`,
    close: () => new Promise(resolveClose => {
      server.closeAllConnections?.();
      server.close(() => resolveClose());
    })
  };
}

/**
 * Answer one request from the files beneath `rootDirectory`.
 * @param {string} rootDirectory
 * @param {import('node:http').IncomingMessage} request
 * @param {import('node:http').ServerResponse} response
 */
async function handleRequest(rootDirectory, request, response) {
  if(request.method !== 'GET' && request.method !== 'HEAD') {
    response.writeHead(405, { Allow: 'GET, HEAD' }).end();
    return;
  }

  const file = await resolveRequestFile(rootDirectory, request.url ?? '/');
  if(!file) {
    response.writeHead(404, { 'Content-Type': 'text/plain; charset=utf-8' }).end('Not found');
    return;
  }

  const mediaType = MEDIA_TYPES[extname(file.path).toLowerCase()] ?? BINARY_MEDIA_TYPE;
  response.writeHead(200, {
    'Content-Type': mediaType,
    'Content-Length': file.size,
    // Previews and captures always want the latest build.
    'Cache-Control': 'no-store'
  });
  if(request.method === 'HEAD') {
    response.end();
    return;
  }
  createReadStream(file.path).pipe(response);
}

/**
 * The file a request URL names beneath `rootDirectory`, or null when it is missing or outside the root.
 * @param {string} rootDirectory
 * @param {string} requestUrl
 * @returns {Promise<{ path: string, size: number } | null>}
 */
async function resolveRequestFile(rootDirectory, requestUrl) {
  let pathname;
  try {
    pathname = decodeURIComponent(new URL(requestUrl, 'http://localhost').pathname);
  } catch {
    return null;
  }

  const candidate = normalize(join(rootDirectory, pathname));
  if(candidate !== rootDirectory && !candidate.startsWith(rootDirectory + sep)) {
    return null;
  }

  const information = await stat(candidate).catch(() => null);
  if(information?.isFile()) {
    return { path: candidate, size: information.size };
  }
  if(information?.isDirectory()) {
    const index = join(candidate, INDEX_DOCUMENT);
    const indexInformation = await stat(index).catch(() => null);
    if(indexInformation?.isFile()) {
      return { path: index, size: indexInformation.size };
    }
  }
  return null;
}
