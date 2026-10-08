// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import { readdir, readFile, lstat, realpath, writeFile, rename, unlink, mkdir, link, rm } from 'node:fs/promises';
import path from 'node:path';
import { parseLevel } from 'joy-engine/levels';
import { parseObject } from 'joy-engine/objects';
import { JOY_PROJECT_MANIFEST, parseProjectManifest } from 'joy-engine/project-manifest';
import { createHash, randomUUID } from 'node:crypto';

const MAX_BYTES = 6_000_000;

/** Directory, relative to the workspace root, whose child folders are authorable projects. */
export const DEFAULT_PROJECTS_DIRECTORY = 'projects';
const BUILT_IN_FORMATS = new Set(['.form', '.formlab', '.joyfx', '.joylevel', '.joyobject']);

/**
 * Local project files. Each instance owns its write queue; sources stay on disk.
 * root is the hosting workspace (the game repository); projects are discovered in
 * its projectsDirectory so the editor does not assume a particular checkout layout. */
export class ProjectFiles {
  constructor(root, {projectsDirectory = DEFAULT_PROJECTS_DIRECTORY} = {}) {
    this.root = path.resolve(root);
    this.projectsRoot = path.resolve(this.root, projectsDirectory);
    this.writes = new Map();
    /**
     * Desktop-selected projects live only for this process and never alter the checkout. */
    this.externalProjects = new Map();
  }

  /**
   * Register a compatible folder selected by the trusted desktop host. */
  async registerProject(directory) {
    const root = await realpath(directory);
    const manifest = await this.readManifest(root);
    const id = `external-${revision(root).slice(0, 12)}`;
    this.externalProjects.set(id, root);
    return this.describeProject(id, root, manifest, true);
  }

  /**
   * Discover authorable checkout projects from their package metadata. */
  async listProjects() {
    const directory = this.projectsRoot;
    const entries = await readdir(directory, {withFileTypes: true});
    const projects = [];
    for(const entry of entries) {
      if(!entry.isDirectory() || !/^[a-z0-9_-]+$/i.test(entry.name)) {
        continue;
      }
      try {
        const root = path.join(directory, entry.name);
        const manifest = await this.readManifest(root).catch(() => null);
        const metadata = JSON.parse(await readFile(path.join(root, 'package.json'), 'utf8'));
        projects.push(await this.describeProject(entry.name, root, manifest, false, metadata));
      } catch {
        // A directory without package metadata is not an authorable project.
      }
    }
    for(const [id, root] of this.externalProjects) {
      projects.push(await this.describeProject(id, root, await this.readManifest(root), true));
    }
    return projects.sort((a, b) => a.name.localeCompare(b.name));
  }

  /**
   * List supported assets beneath a project without traversing symlink entries. */
  async listFiles(project) {
    const root = await this.projectRoot(project);
    const files = [];
    const formats = await this.projectFormats(root);
    const visit = async (directory, relative) => {
      let entries;
      try {
        entries = await readdir(directory, {withFileTypes: true});
      } catch(error) {
        if(error.code === 'ENOENT') {
          return;
        }
        throw error;
      }
      for(const entry of entries) {
        const name = `${relative}/${entry.name}`;
        if(entry.isDirectory() && !entry.name.startsWith('.')) {
          await visit(path.join(directory, entry.name), name);
        } else if(entry.isFile() && formats.has(path.extname(entry.name).toLowerCase())) {
          files.push({path: name, kind: path.extname(entry.name).slice(1)});
        }
        if(files.length > 10000) {
          throw new Error('Project exceeds the 10,000 asset browsing limit.');
        }
      }
    };
    const assets = path.join(root, 'assets');
    try {
      if((await lstat(assets)).isSymbolicLink()) {
        throw new Error('Asset directories must not be symbolic links.');
      }
    } catch(error) {
      if(error.code !== 'ENOENT') {
        throw error;
      }
    }
    await visit(assets, 'assets');
    return files.sort((a, b) => a.path.localeCompare(b.path));
  }

  /**
   * List retained pre-save copies without exposing arbitrary hidden files. */
  async listRecovery(project) {
    const root = await this.projectRoot(project);
    const recovered = [];
    const visit = async (directory, relative) => {
      for(const entry of await readdir(directory, {withFileTypes: true}).catch(() => [])) {
        const child = path.join(directory, entry.name);
        if(entry.isDirectory() && entry.name === '.joy-editor-recovery') {
          for(const copy of await readdir(child, {withFileTypes: true})) {
            if(copy.isFile()) {
              const info = await lstat(path.join(child, copy.name));
              recovered.push({path: `${relative}/${entry.name}/${copy.name}`, bytes: info.size, modified: info.mtime.toISOString()});
            }
          }
        } else if(entry.isDirectory() && !entry.name.startsWith('.')) {
          await visit(child, `${relative}/${entry.name}`);
        }
      }
    };
    await visit(path.join(root, 'assets'), 'assets');
    return recovered.sort((a, b) => b.modified.localeCompare(a.modified));
  }

  /**
   * Remove one recovery copy, or all copies for the project, after strict path validation. */
  async removeRecovery(project, relative = '') {
    const root = await this.projectRoot(project);
    const copies = await this.listRecovery(project);
    const selected = relative ? copies.filter(copy => copy.path === relative) : copies;
    if(relative && selected.length !== 1) {
      throw new Error('Recovery copy does not exist.');
    }
    for(const copy of selected) {
      await rm(path.join(root, copy.path), {force: true});
    }
    return {removed: selected.length};
  }

  /**
   * Rename an asset without replacing an existing path. */
  async move(project, relative, destination) {
    const source = await this.resolveFile(project, relative);
    const target = await this.resolveNewFile(project, destination);
    await link(source, target).catch(error => {
      if(error.code === 'EEXIST') {
        throw Object.assign(new Error('A file with this name already exists.'), {status: 409});
      }
      throw error;
    });
    await unlink(source);
    return {path: destination};
  }

  /**
   * Delete an asset only after the caller has explicitly selected its exact path. */
  async remove(project, relative) {
    const file = await this.resolveFile(project, relative);
    await unlink(file);
    return {path: relative};
  }

  /**
   * Read bounded UTF-8 source and its content revision for optimistic saves. */
  async read(project, relative) {
    const file = await this.resolveFile(project, relative);
    const info = await lstat(file);
    if(info.size > MAX_BYTES) {
      throw new Error('Assets must be 6 MB or smaller.');
    }
    const text = await readFile(file, 'utf8');
    return {text, revision: revision(text)};
  }

  /**
   * Compare revisions inside the serialized operation, including competing editor saves. */
  async save(project, relative, text, expectedRevision) {
    if(typeof text !== 'string' || Buffer.byteLength(text) > MAX_BYTES || typeof expectedRevision !== 'string') {
      throw new Error('Invalid asset save; maximum size is 6 MB.');
    }
    const key = `${project}/${relative}`;
    const previous = this.writes.get(key) ?? Promise.resolve();
    const operation = previous.catch(() => {}).then(async () => {
      const file = await this.resolveFile(project, relative);
      const current = await this.read(project, relative);
      if(current.revision !== expectedRevision) {
        throw Object.assign(new Error('This file changed on disk. Review the conflict before saving.'), {status: 409, current});
      }
      const temporary = `${file}.${randomUUID()}.tmp`;
      try {
        await writeFile(temporary, text, {flag: 'wx', mode: (await lstat(file)).mode});
        // Recheck after preparing the temporary file to catch external editor writes.
        const latest = await this.read(project, relative);
        if(latest.revision !== expectedRevision) {
          throw Object.assign(new Error('This file changed on disk. Review the conflict before saving.'), {status: 409, current: latest});
        }
        // Capture the exact replaced inode, then install exclusively. A competing
        // external atomic save wins; its data is never overwritten by rename.
        const recoveryDirectory = path.join(path.dirname(file), '.joy-editor-recovery');
        await mkdir(recoveryDirectory, {recursive: true});
        if((await lstat(recoveryDirectory)).isSymbolicLink()) {
          throw new Error('Recovery directory must not be a symbolic link.');
        }
        const backup = path.join(recoveryDirectory, `${path.basename(file)}.${randomUUID()}`);
        await rename(file, backup);
        const replacedText = await readFile(backup, 'utf8');
        if(revision(replacedText) !== expectedRevision) {
          await link(backup, file).catch(error => {
            if(error.code !== 'EEXIST') {
              throw error;
            }
          });
          throw Object.assign(new Error('This file changed during save. The external version was preserved.'), {status: 409, current: await this.read(project, relative)});
        }
        try {
          await link(temporary, file);
        } catch(error) {
          // The old inode is retained for recovery, including late in-place writes.
          await link(backup, file).catch(() => {});
          if(error.code === 'EEXIST') {
            throw Object.assign(new Error('This file changed during save. The external version was preserved.'), {status: 409, current: await this.read(project, relative)});
          }
          throw error;
        }
      } finally {
        await unlink(temporary).catch(() => {});
      }
      return {text, revision: revision(text)};
    });
    this.writes.set(key, operation);
    try {
      return await operation;
    } finally {
      if(this.writes.get(key) === operation) {
        this.writes.delete(key);
      }
    }
  }

  /**
   * Create a validated supported document; exclusive writes never replace existing content. */
  async create(project, relative, text) {
    if(typeof text !== 'string' || Buffer.byteLength(text) > MAX_BYTES) {
      throw new Error('Invalid level creation; maximum size is 6 MB.');
    }
    if(path.extname(relative) === '.joyobject') {
      parseObject(text);
    }
    if(path.extname(relative) === '.joylevel') {
      parseLevel(text);
    }
    const file = await this.resolveNewFile(project, relative);
    try {
      await writeFile(file, text, {flag: 'wx'});
    } catch(error) {
      if(error.code === 'EEXIST') {
        throw Object.assign(new Error('A file with this name already exists.'), {status: 409});
      }
      throw error;
    }
    return {text, revision: revision(text)};
  }

  /**
   * Resolve a checkout-owned project directory, rejecting aliases outside the root. */
  async projectRoot(project) {
    if(typeof project !== 'string' || !/^[a-z0-9_-]+$/i.test(project)) {
      throw new Error('Invalid project.');
    }
    const external = this.externalProjects.get(project);
    if(external) {
      return external;
    }
    const root = path.join(this.projectsRoot, project);
    if((await lstat(root)).isSymbolicLink() || !inside(await realpath(this.root), await realpath(root))) {
      throw new Error('Project must be inside this checkout.');
    }
    return root;
  }

  /**
   * Resolve an existing asset only after validating every path component. */
  async resolveFile(project, relative) {
    const root = await this.projectRoot(project);
    if(!await this.isSupportedPath(root, relative)) {
      throw new Error('Only project FORM, particle and level assets can be opened.');
    }
    let file = root;
    for(const part of relative.split('/')) {
      file = path.join(file, part);
      if((await lstat(file)).isSymbolicLink()) {
        throw new Error('Symbolic links are not editable project assets.');
      }
    }
    if(!inside(await realpath(root), await realpath(file)) || !(await lstat(file)).isFile()) {
      throw new Error('File is outside the project.');
    }
    return file;
  }

  async resolveNewFile(project, relative) {
    const root = await this.projectRoot(project);
    if(!await this.isSupportedPath(root, relative)) {
      throw new Error('Asset paths must stay beneath assets/ and use a project-supported extension.');
    }
    const parent = path.dirname(path.join(root, relative));
    await mkdir(parent, {recursive: true});
    if((await lstat(parent)).isSymbolicLink() || !inside(await realpath(root), await realpath(parent))) {
      throw new Error('Asset directories must not be symbolic links.');
    }
    return path.join(root, relative);
  }

  async isSupportedPath(root, relative) {
    return typeof relative === 'string' && relative.startsWith('assets/') && !relative.includes('\\') &&
      relative.split('/').every(part => part && part !== '.' && part !== '..') &&
      (await this.projectFormats(root)).has(path.extname(relative).toLowerCase());
  }

  async projectFormats(root) {
    const manifest = await this.readManifest(root).catch(() => null);
    return new Set([...BUILT_IN_FORMATS, ...(manifest?.capabilities.flatMap(capability => capability.extensions) ?? [])]);
  }

  async readManifest(root) {
    return parseProjectManifest(JSON.parse(await readFile(path.join(root, JOY_PROJECT_MANIFEST), 'utf8')));
  }

  async describeProject(id, root, manifest, external, metadata = null) {
    const name = manifest?.name ?? metadata?.gameSite?.title ?? id.replaceAll('-', ' ');
    return {
      id, name, external, path: root,
      preview: external ? null : `/games/${id}/`,
      connected: Boolean(manifest?.capabilities.length),
      capabilities: manifest?.capabilities ?? []
    };
  }
}

function revision(text) {
  return createHash('sha256').update(text).digest('hex');
}

function inside(root, file) {
  const relative = path.relative(root, file);
  return relative !== '..' && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative);
}
