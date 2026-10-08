// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { ConfigField } from '../core/config-values.ts';
import type { Plugin, ResolvedConfig, ViteDevServer, HmrContext } from 'vite';
import type { IncomingMessage, ServerResponse } from 'node:http';

import { createHash, randomUUID } from 'node:crypto';
import { readFile, rename, unlink, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { createConfigValues, createConfigDocumentValues } from '../core/config-values.ts';

/**
 * Development-only persistence for one registered project file. Requests cannot
 * choose a path. Saves validate before writing, serialize concurrent requests,
 * and require the revision the editor originally read to avoid lost updates.
 * @param options
 */
export function createConfigPlugin({ file, fields }: {
    file: string;
    fields: readonly ConfigField[];
}): Plugin {
  const configFile = path.resolve(file);
  let base = '/';
  let writes = Promise.resolve();
  return {
    name: 'joy-config-tuning',
    apply: 'serve',
    configResolved(config: ResolvedConfig) {
      base = config.base;
    },
    configureServer(server: ViteDevServer) {
      server.middlewares.use((request, response, next) => {
        const pathname = new URL(request.url ?? '/', 'http://localhost').pathname;
        if(pathname !== `${base}__joy/config`) {
          next();
          return;
        }
        response.setHeader('Cache-Control', 'no-store');
        response.setHeader('Content-Type', 'application/json');
        // Queue the entire operation, including the revision check. Two tabs
        // saving the same baseline can never both report success.
        const operation = writes.then(() => handleRequest(request, response));
        writes = operation.catch(() => {});
      });
    },
    handleHotUpdate(context: HmrContext) {
      if(path.resolve(context.file) === configFile) {
        // The running session already owns the saved values. Reloading would
        // discard the exact gameplay situation the designer is tuning.
        // External file edits are picked up on reload; revision checks protect
        // them if this running editor subsequently tries to save.
        return [];
      }
    },
  };

  /**
   * Serve one queued snapshot read or validated revision-checked save.
   * @param request
   * @param response
   */
  async function handleRequest(request: IncomingMessage, response: ServerResponse) {
    try {
      if(request.method === 'GET') {
        const saved = await readSnapshot();
        response.end(JSON.stringify({ revision: saved.revision, values: saved.values }));
        return;
      }
      if(request.method !== 'PUT') {
        response.setHeader('Allow', 'GET, PUT');
        throw new ConfigRequestError(405, 'Use GET or PUT for config tuning.');
      }
      const origin = request.headers.origin;
      const sameOrigin = origin === `http://${request.headers.host}` || origin === `https://${request.headers.host}`;
      if(!sameOrigin || request.headers['x-joy-config'] !== '1') {
        throw new ConfigRequestError(403, 'Config saves require a same-origin editor request.');
      }
      if(request.headers['content-type']?.split(';')[0] !== 'application/json') {
        throw new ConfigRequestError(415, 'Config saves require JSON.');
      }
      const body = JSON.parse(await readBody(request));
      if(!body || typeof body !== 'object' || typeof body.revision !== 'string' || !body.values || typeof body.values !== 'object' || Array.isArray(body.values) || Object.keys(body).some(key => key !== 'revision' && key !== 'values')) {
        throw new ConfigRequestError(400, 'Expected a config revision and numeric values.');
      }
      const values = createConfigValues(fields, body.values);
      const before = await readSnapshot();
      if(before.revision !== body.revision) {
        throw new ConfigRequestError(409, 'The config file changed outside this editor. Your edits are retained; reload to reconcile before saving.');
      }
      // Save only differences, so future authored default changes still flow
      // through fields the designer did not override.
      const overrides = Object.fromEntries(
        fields
          .filter(field => values[field.key] !== field.defaultValue)
          .map(field => [field.key, values[field.key]])
      );
      const text = `${JSON.stringify({ $schema: './tuning-overrides.schema.json', version: 1, values: overrides }, null, 2)}\n`;
      const temporary = `${configFile}.${randomUUID()}.tmp`;
      try {
        await writeFile(temporary, text, { flag: 'wx' });
        if((await readSnapshot()).revision !== before.revision) {
          throw new ConfigRequestError(409, 'The config file changed during save. Your edits are retained.');
        }
        await rename(temporary, configFile);
      } finally {
        await unlink(temporary).catch(() => {});
      }
      response.end(JSON.stringify({ revision: revisionOf(text), values }));
    } catch (error) {
      response.statusCode = error instanceof ConfigRequestError ? error.status : 400;
      response.end(JSON.stringify({ error: error instanceof Error ? error.message : 'Unable to save config.' }));
    }
  }

  /** Read values and their revision from the same file contents. */
  async function readSnapshot() {
    const text = await readFile(configFile, 'utf8');
    const document = JSON.parse(text);
    return { revision: revisionOf(text), values: createConfigDocumentValues(fields, document) };
  }
}

/** Carry the client-facing HTTP status through the shared request failure path. */
class ConfigRequestError extends Error {
  declare status: number;

  /**
   * @param status
   * @param message
   */
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

/**
 * Hash exact file contents so even external formatting changes invalidate stale saves.
 * @param text
 */
function revisionOf(text: string) {
  return createHash('sha256').update(text).digest('hex');
}

/**
 * Read a bounded UTF-8 request body; oversized input fails before JSON parsing.
 * @param request
 */
async function readBody(request: IncomingMessage) {
  let size = 0;
  const chunks = [];
  for await(const chunk of request) {
    size += chunk.length;
    if(size > 16 * 1024) {
      throw new ConfigRequestError(413, 'Config request exceeds 16 KiB.');
    }
    chunks.push(chunk);
  }
  return Buffer.concat(chunks).toString('utf8');
}
