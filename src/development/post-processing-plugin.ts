// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { IncomingMessage, ServerResponse } from 'node:http';
import type { HmrContext, Plugin, ResolvedConfig, ViteDevServer } from 'vite';
import type { PostProcessingProfile } from '../rendering/postprocessor/profile.ts';

import { createHash, randomUUID } from 'node:crypto';
import { lstat, readFile, realpath, rename, unlink, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { validatePostProcessingProfile } from '../rendering/postprocessor/profile.ts';

// Upper bound for both the authored level file and a single profile write request.
const maximumBytes = 1024*1024;

// Hot-update event the in-game post-processing editor listens for.
const POST_PROCESSING_EVENT = 'joy:post-processing';

// Endpoint path relative to the Vite base; Vite may strip the base before project middleware runs.
const POST_PROCESSING_ENDPOINT = '__joy/post-processing';

/**
 * Validated profile plus the SHA-256 revision of the exact file bytes it came from.
 */
type ProfileSnapshot = {
  profile: PostProcessingProfile;
  revision: string;
};


/**
 * Serve and watch one explicitly configured authored profile. No request controls a path.
 * Each project owns an instance so root-site middleware and workspace dev behave alike.
 * Development only (`apply: 'serve'`); production builds never include the endpoint.
 * @param options profilePath is the absolute, project-owned level file holding `postProcessing`.
 */
export function createPostProcessingPlugin({ profilePath }: { profilePath: string }): Plugin {
  const filename = path.resolve(profilePath);
  let endpoint = `/${POST_PROCESSING_ENDPOINT}`;
  let queue: Promise<void> = Promise.resolve();
  let detach = () => {};

  return {
    name: 'joy-post-processing',
    apply: 'serve',
    configResolved(config: ResolvedConfig) {
      endpoint = `${config.base}${POST_PROCESSING_ENDPOINT}`;
      // Tuning metadata can import the game config, which also resolves this
      // profile. It is live application data, not a server configuration input;
      // otherwise Vite restarts before our custom update event can handle it.
      // Vite types the resolved config as read-only, but configResolved is the documented
      // place to adjust it; replace the list exactly as the original JavaScript plugin did.
      const writableConfig = config as { configFileDependencies: string[] };
      writableConfig.configFileDependencies = config.configFileDependencies.filter(dependency => path.resolve(dependency) !== filename);
    },
    configureServer(server: ViteDevServer) {
      server.watcher.add(filename);
      let notification: ReturnType<typeof setTimeout> | undefined;
      let sequence = 0;
      const publish = (generation: number) => {
        void readSnapshot(filename).then(snapshot => {
          if(generation === sequence) {
            server.ws.send({ type: 'custom', event: POST_PROCESSING_EVENT, data: snapshot });
          }
        }).catch(error => {
          if(generation === sequence) {
            server.ws.send({ type: 'custom', event: POST_PROCESSING_EVENT, data: { error: errorMessage(error) } });
          }
        });
      };
      const schedulePublish = () => {
        clearTimeout(notification);
        sequence+= 1;
        const generation = sequence;
        // Editors commonly save by replacing a file. Read after the atomic-write
        // watch window so a rapid second edit cannot leave clients one save behind.
        notification = setTimeout(() => publish(generation), 150);
      };
      const onFileEvent = (changed: string) => {
        if(path.resolve(changed) === filename) {
          schedulePublish();
        }
      };
      server.watcher.on('change', onFileEvent);
      server.watcher.on('add', onFileEvent);
      server.watcher.on('unlink', onFileEvent);
      detach = () => {
        clearTimeout(notification);
        sequence+= 1;
        server.watcher.off('change', onFileEvent);
        server.watcher.off('add', onFileEvent);
        server.watcher.off('unlink', onFileEvent);
      };
      server.middlewares.use((request: IncomingMessage, response: ServerResponse, next: () => void) => {
        const pathname = new URL(request.url ?? '/', 'http://localhost').pathname;
        // Vite can strip base before invoking project middleware.
        if(pathname !== endpoint && pathname !== `/${POST_PROCESSING_ENDPOINT}`) {
          next();
          return;
        }
        response.setHeader('Cache-Control', 'no-store');
        response.setHeader('Content-Type', 'application/json');
        const respond = (status: number, data: unknown) => {
          response.statusCode = status;
          response.end(JSON.stringify(data));
        };
        if(request.method === 'GET') {
          void readSnapshot(filename).then(snapshot => respond(200, snapshot)).catch(error => respond(400, { error: errorMessage(error) }));
          return;
        }
        if(request.method !== 'POST') {
          respond(405, { error: 'Use GET or POST.' });
          return;
        }
        const origin = request.headers.origin;
        if(!isSameOrigin(origin, request.headers.host) || request.headers['x-joy-postprocessing'] !== '1' ||
            request.headers['content-type'] !== 'application/json') {
          respond(403, { error: 'Profile writes require a same-origin editor request.' });
          return;
        }
        // Serialize writers so two tabs cannot both commit against one revision.
        const body = readBody(request);
        const operation = queue.then(async () => {
          const submitted = await body;
          if(!isRecord(submitted) || Object.keys(submitted).some(key => !['profile', 'revision'].includes(key))) {
            throw new Error('Expected only profile and revision.');
          }
          const profile = validatePostProcessingProfile(submitted.profile);
          const current = await readSnapshot(filename);
          if(submitted.revision !== current.revision) {
            respond(409, { ...current, error: 'The file changed. Choose Use Disk or Keep Local before saving.' });
            return;
          }
          const document = JSON.parse(await readFile(filename, 'utf8'));
          document.postProcessing = profile;
          const text = `${JSON.stringify(document, null, 2)}\n`;
          const temporary = `${filename}.${randomUUID()}.tmp`;
          try {
            await writeFile(temporary, text, { flag: 'wx' });
            // Check again after asynchronous I/O, including symlink replacement.
            const latest = await readSnapshot(filename);
            if(latest.revision !== current.revision) {
              respond(409, { ...latest, error: 'The file changed during saving.' });
              return;
            }
            await rename(temporary, filename);
          } finally {
            await unlink(temporary).catch(() => {});
          }
          respond(200, { profile, revision: revisionOf(text) });
          schedulePublish();
        });
        queue = operation.catch(error => respond(400, { error: errorMessage(error) }));
        // A queued request's body can fail before the prior request finishes.
        void body.catch(() => {});
      });
    },
    handleHotUpdate(context: HmrContext) {
      if(path.resolve(context.file) === filename) {
        // Keep imported production data fresh for new pages, but never propagate
        // profile changes into the current game's composition root.
        for(const module of context.modules) {
          context.server.moduleGraph.invalidateModule(module);
        }
        return [];
      }
    },
    closeBundle() { detach(); },
  };
}

/**
 * Reject symlinks, oversized files, and invalid data without replacing live settings.
 * @param filename
 */
async function readSnapshot(filename: string): Promise<ProfileSnapshot> {
  const info = await lstat(filename);
  if(!info.isFile() || info.isSymbolicLink() || info.size > maximumBytes || await realpath(filename) !== filename) {
    throw new Error('Level must be a regular file at its configured path, at most 1 MiB.');
  }
  const text = await readFile(filename, 'utf8');
  const document = JSON.parse(text);
  return {profile: validatePostProcessingProfile(document.postProcessing), revision: revisionOf(text)};
}

/**
 * Hash exact disk bytes so concurrent editors compare the same revision.
 * @param text
 */
function revisionOf(text: string): string {
  return createHash('sha256').update(text).digest('hex');
}

/**
 * Permit browser writes only from the development server's own HTTP origin.
 * @param origin
 * @param host
 */
function isSameOrigin(origin: string | undefined, host: string | undefined): boolean {
  if(!origin) {
    return false;
  }
  try {
    const url = new URL(origin);
    return ['http:', 'https:'].includes(url.protocol) && url.host === host;
  } catch {
    return false;
  }
}

/**
 * Read a bounded JSON payload before the serialized file-write operation.
 * @param request
 */
async function readBody(request: IncomingMessage): Promise<unknown> {
  let size = 0;
  const chunks: Buffer[] = [];
  for await(const chunk of request) {
    size+= chunk.length;
    if(size > maximumBytes) {
      throw new Error('Profile request exceeds 16 KiB.');
    }
    chunks.push(chunk);
  }
  return JSON.parse(Buffer.concat(chunks).toString('utf8'));
}

/**
 * Narrow a parsed JSON payload to a plain object record.
 * @param value
 */
function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}

/**
 * Report thrown values without assuming they are Error instances.
 * @param error
 */
function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
