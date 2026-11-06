#!/usr/bin/env node
// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Native API exporters share the actual Rust registration code. This launcher owns generated files and optional
// source watching; it never parses Rust signatures or executes a game's Lua scripts.

import {spawn} from 'node:child_process';
import {mkdir, readFile, readdir, rename, rm, stat, writeFile} from 'node:fs/promises';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {parseArgs} from 'node:util';

const ENGINE_DIRECTORY = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

/**
 * Replace a generated definition atomically, or check it without writing. Unchanged files retain their mtime so
 * editor indexers and source watchers do not repeatedly invalidate themselves.
 */
export async function updateDefinition(output, contents, check) {
  let existing;
  try {
    existing = await readFile(output, 'utf8');
  } catch(error) {
    if(error.code !== 'ENOENT') {
      throw error;
    }
  }
  if(existing === contents) {
    return;
  }
  if(check) {
    throw new Error(`${existing === undefined ? 'Missing' : 'Stale'} Lua definitions: ${output}. Run npm run generate:lua-definitions.`);
  }
  await mkdir(path.dirname(output), {recursive: true});
  const temporary = `${output}.${process.pid}.tmp`;
  try {
    await writeFile(temporary, contents);
    await rename(temporary, output);
  } finally {
    await rm(temporary, {force: true});
  }
}

/**
 * One owner serializes Cargo exporters, coalesces edits during a build, and releases its source-scan timer and child on exit.
 */
export class DefinitionGenerator {
  constructor(targets, check) {
    this.targets = targets;
    this.check = check;
    this.child = null;
    this.scanning = false;
    this.previousSources = null;
    this.running = false;
    this.pending = false;
    this.closed = false;
    this.timer = null;
  }

  async generate() {
    if(this.running) {
      this.pending = true;
      return;
    }
    this.running = true;
    try {
      do {
        this.pending = false;
        try {
          const outputs = [];
          for(const target of this.targets) {
            if(this.closed) {
              return;
            }
            outputs.push({target, contents: await this.export(target)});
          }
          // Keep the previous set if any exporter fails, rather than mixing new engine and old game APIs.
          for(const {target, contents} of outputs) {
            if(!this.closed) {
              await updateDefinition(target.output, contents, this.check);
            }
          }
        } catch(error) {
          if(!this.pending || this.closed) {
            throw error;
          }
          console.error(`${error.message} Retrying the queued source edit.`);
        }
      } while(this.pending && !this.closed);
      console.log(`Lua definitions ${this.check ? 'checked' : 'generated'} (${this.targets.length} libraries).`);
    } finally {
      this.running = false;
    }
  }

  export(target) {
    return new Promise((resolve, reject) => {
      const args = ['run', '--quiet', '--manifest-path', target.manifest, '-p', target.package, '--bin', target.binary];
      if(target.features) {
        args.push('--features', target.features);
      }
      const child = spawn('cargo', args, {cwd: ENGINE_DIRECTORY, stdio: ['ignore', 'pipe', 'inherit']});
      this.child = child;
      let contents = '';
      child.stdout.setEncoding('utf8');
      child.stdout.on('data', data => { contents+= data; });
      child.on('error', reject);
      child.on('close', code => {
        this.child = null;
        if(code !== 0) {
          reject(new Error(`${target.binary} exited with ${code}. Existing definitions were retained.`));
        } else if(!contents.includes('---@meta')) {
          reject(new Error(`${target.binary} did not emit Lua definitions.`));
        } else {
          resolve(contents);
        }
      });
    });
  }

  watch() {
    const roots = new Set(this.targets.flatMap(target => target.sources));
    const scan = async () => {
      if(this.closed || this.scanning) {
        return;
      }
      this.scanning = true;
      try {
        const sources = [];
        for(const root of roots) {
          await collectSourceTimes(root, sources);
        }
        const current = sources.sort().join('\n');
        const changed = this.previousSources !== null && current !== this.previousSources;
        this.previousSources = current;
        if(changed && !this.closed) {
          await this.generate();
        }
      } catch(error) {
        console.error(`Lua definition watcher: ${error.message}`);
      } finally {
        this.scanning = false;
      }
    };
    // A small metadata scan avoids recursive filesystem watcher limits and discovers added/removed source files.
    this.timer = setInterval(scan, 1000);
    void scan();
    console.log('Watching Rust API sources; press Ctrl+C to stop.');
  }

  close() {
    this.closed = true;
    clearInterval(this.timer);
    this.child?.kill('SIGTERM');
  }
}

/** Collect Rust sources and crate manifests; generated output and dependency/build directories never enter the scan. */
async function collectSourceTimes(root, sources) {
  const information = await stat(root, {bigint: true});
  if(information.isFile()) {
    if(/\.rs$|Cargo\.(?:toml|lock)$/.test(root)) {
      sources.push(`${root}:${information.mtimeNs}:${information.size}`);
    }
    return;
  }
  for(const entry of await readdir(root, {withFileTypes: true})) {
    if(entry.name === 'target' || entry.name === 'node_modules' || entry.name.startsWith('.')) {
      continue;
    }
    if(entry.isFile() || entry.isDirectory()) {
      await collectSourceTimes(path.join(root, entry.name), sources);
    }
  }
}

async function main() {
  const {values} = parseArgs({options: {
    project: {type: 'string'},
    check: {type: 'boolean', default: false},
    watch: {type: 'boolean', default: false}
  }});
  if(values.check && values.watch) {
    throw new Error('--check and --watch are separate modes.');
  }
  const targets = [{
    manifest: path.join(ENGINE_DIRECTORY, 'Cargo.toml'),
    package: 'joy-gameplay',
    binary: 'joy-lua-definitions',
    features: 'game-script',
    output: path.join(ENGINE_DIRECTORY, '.lua-definitions', 'joy-engine.d.lua'),
    sources: [path.join(ENGINE_DIRECTORY, 'Cargo.toml'), path.join(ENGINE_DIRECTORY, 'Cargo.lock'), path.join(ENGINE_DIRECTORY, 'crates', 'joy-game-script'), path.join(ENGINE_DIRECTORY, 'crates', 'joy-gameplay')]
  }];
  if(values.project) {
    const directory = path.resolve(values.project);
    const metadata = JSON.parse(await readFile(path.join(directory, 'package.json'), 'utf8')).luaDefinitions;
    if(!metadata?.package || !metadata?.binary) {
      throw new Error(`${directory}/package.json needs luaDefinitions.package and luaDefinitions.binary.`);
    }
    targets.push({
      manifest: path.join(directory, 'Cargo.toml'),
      package: metadata.package,
      binary: metadata.binary,
      output: path.join(directory, '.lua-definitions', `${metadata.package}.d.lua`),
      sources: [path.join(directory, 'src'), path.join(directory, 'Cargo.toml')]
    });
  }
  const generator = new DefinitionGenerator(targets, values.check);
  const shutdown = () => generator.close();
  process.once('SIGINT', shutdown);
  process.once('SIGTERM', shutdown);
  try {
    if(values.watch) {
      generator.watch();
    }
    await generator.generate();
  } catch(error) {
    if(!values.watch) {
      generator.close();
      throw error;
    }
    console.error(error.message);
  } finally {
    if(!values.watch) {
      process.off('SIGINT', shutdown);
      process.off('SIGTERM', shutdown);
    }
  }
}

if(process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch(error => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
