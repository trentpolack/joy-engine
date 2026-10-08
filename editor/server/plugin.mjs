// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import path from 'node:path';
import { DEFAULT_PROJECTS_DIRECTORY, ProjectFiles } from './project-files.mjs';

export { DEFAULT_PROJECTS_DIRECTORY };

/** Vite-only local file API. No service or write endpoint enters production builds.
 * The hosting workspace installs this plugin; assetUpdates carries its project layout. */
export function createWorkspacePlugin(root, assetUpdates = new WorkspaceAssetUpdates(root)) {
  const files = new ProjectFiles(root, {projectsDirectory: assetUpdates.projectsDirectory});
  return {
    name: 'joy-editor-project-files',
    configureServer(server) {
      server.middlewares.use(async (request, response, next) => {
        const url = new URL(request.url, 'http://localhost');
        if(!url.pathname.startsWith('/__joy_editor/')) {
          next();
          return;
        }
        response.setHeader('Content-Type', 'application/json');
        response.setHeader('Cache-Control', 'no-store');
        try {
          // Reject cross-origin requests even when the editor service is accidentally LAN-bound.
          const origin = request.headers.origin;
          if(request.headers['sec-fetch-site'] === 'cross-site' || (origin && new URL(origin).host !== request.headers.host) || request.headers['x-joy-editor'] !== 'workspace') {
            throw Object.assign(new Error('Open files from the local Joy Editor workspace.'), {status: 403});
          }
          const project = url.searchParams.get('project');
          const file = url.searchParams.get('path');
          let result;
          if(request.method === 'GET' && url.pathname === '/__joy_editor/projects') {
            result = await files.listProjects();
          } else if(request.method === 'POST' && url.pathname === '/__joy_editor/register-project') {
            if(!process.env.JOY_EDITOR_DESKTOP_TOKEN || request.headers['x-joy-editor-desktop'] !== process.env.JOY_EDITOR_DESKTOP_TOKEN) {
              throw Object.assign(new Error('External folders can only be opened by the desktop host.'), {status: 403});
            }
            result = await files.registerProject((await readBody(request)).path);
          } else if(request.method === 'GET' && url.pathname === '/__joy_editor/files') {
            result = await files.listFiles(project);
          } else if(request.method === 'GET' && url.pathname === '/__joy_editor/recovery') {
            result = await files.listRecovery(project);
          } else if(request.method === 'DELETE' && url.pathname === '/__joy_editor/recovery') {
            result = await files.removeRecovery(project, file ?? '');
          } else if(request.method === 'GET' && url.pathname === '/__joy_editor/file') {
            result = await files.read(project, file);
          } else if(request.method === 'POST' && url.pathname === '/__joy_editor/file') {
            const payload = await readBody(request);
            result = await assetUpdates.write(project, file, () => files.create(project, file, payload.text));
          } else if(request.method === 'PUT' && url.pathname === '/__joy_editor/file') {
            const payload = await readBody(request);
            result = await assetUpdates.write(project, file, () => files.save(project, file, payload.text, payload.revision));
          } else if(request.method === 'PATCH' && url.pathname === '/__joy_editor/file') {
            const payload = await readBody(request);
            result = await assetUpdates.write(project, file, () => files.move(project, file, payload.path));
          } else if(request.method === 'DELETE' && url.pathname === '/__joy_editor/file') {
            result = await assetUpdates.write(project, file, () => files.remove(project, file));
          } else {
            throw Object.assign(new Error('Unknown workspace operation.'), {status: 404});
          }
          response.end(JSON.stringify(result));
        } catch(error) {
          response.statusCode = error.status ?? 400;
          response.end(JSON.stringify({error: error.message, current: error.current}));
        }
      });
    }
  };
}

/** Collect a bounded UTF-8 JSON payload; reject oversized streams before parsing. */
async function readBody(request) {
  const chunks = [];
  let size = 0;
  for await(const chunk of request) {
    size+= chunk.length;
    if(size > 12_100_000) {
      throw Object.assign(new Error('Asset payload is too large.'), {status: 413});
    }
    chunks.push(chunk);
  }
  return JSON.parse(Buffer.concat(chunks).toString('utf8'));
}

/** Root-owned coordinator for the file API and its mounted Vite servers. Only
 * editor-originated writes suppress HMR; direct VS Code edits keep normal updates.
 */
export class WorkspaceAssetUpdates {
  constructor(root, {projectsDirectory = DEFAULT_PROJECTS_DIRECTORY} = {}) {
    this.root = root;
    this.projectsDirectory = projectsDirectory;
    this.writes = new Map();
    // Authored assets whose editor writes suppress HMR, relative to the workspace root.
    const projects = escapeRegExp(projectsDirectory.replaceAll('\\', '/').replace(/\/+$/, ''));
    this.authoredAsset = new RegExp(`^${projects}/[^/]+/assets/.+\\.(form|formlab|joyfx|joylevel)$`);
  }

  /** Keep an exact path marked through atomic file replacement and watcher settling. */
  async write(project, relative, operation) {
    const file = path.resolve(this.root, this.projectsDirectory, String(project), String(relative));
    const now = Date.now();
    for(const [key, entry] of this.writes) {
      if(entry.active === 0 && entry.until < now) {
        this.writes.delete(key);
      }
    }
    const entry = this.writes.get(file) ?? {active: 0, until: 0};
    entry.active++;
    this.writes.set(file, entry);
    try {
      return await operation();
    } finally {
      entry.active--;
      // Chokidar coalesces the rename/link pair after the filesystem operation ends.
      entry.until = Date.now() + 1000;
    }
  }

  /** @returns {import('vite').Plugin} */
  createPlugin() {
    const owner = this;
    return {
      name: 'joy-editor-authored-content-updates',
      hotUpdate(context) {
        const entry = owner.writes.get(path.resolve(context.file));
        if(!entry || (entry.active === 0 && entry.until < Date.now())) {
          return;
        }
        const relative = path.relative(owner.root, context.file).replaceAll('\\', '/');
        if(!owner.authoredAsset.test(relative)) {
          return;
        }
        // Suppressing a message must not preserve stale raw modules for Stop/Run.
        for(const module of context.modules) {
          this.environment.moduleGraph.invalidateModule(module);
        }
        return [];
      }
    };
  }
}

/** Quote a configured directory for use inside a regular expression. */
function escapeRegExp(text) {
  return text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}
