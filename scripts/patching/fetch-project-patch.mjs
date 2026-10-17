// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { spawnSync } from 'node:child_process';
import { lstat, mkdir, mkdtemp, readFile, rename, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';

import { runPatchGit, validatePatchScope } from './patch-delivery.mjs';

/**
 * Retrieve and verify one target's delivery without changing source, either real index, or refs.
 * @param {import('./patch-delivery.mjs').PatchTarget} target
 * @param {string[]} [arguments_] CLI task/attempt or saved-diff arguments.
 */
export async function fetchProjectPatch(target, arguments_ = process.argv.slice(2)) {
  const PATCH_NAME = target.patchName;
  const BASE_NAME = target.baseName;
  const MANIFEST_NAME = target.manifestName;
  const DELIVERY_NAMES = target.deliveryNames;
  const USAGE = `Usage: npm run fetch-${target.id}-patch -- <task-id> [--attempt <number>]\n       npm run fetch-${target.id}-patch -- --diff <saved-cloud-diff>`;

  let diff;
  if(arguments_.length === 2 && arguments_[0] === '--diff') {
    diff = await readFile(resolve(arguments_[1]), 'utf8');
  } else {
    const [taskId, attemptFlag, attempt] = arguments_;
    const validAttempt = arguments_.length === 3 && attemptFlag === '--attempt' && /^[1-9][0-9]*$/.test(attempt);
    if(!/^task_[A-Za-z0-9_]+$/.test(taskId ?? '') || (arguments_.length !== 1 && !validAttempt)) {
      throw new Error(USAGE);
    }
    console.log('Downloading the Codex Cloud task diff...');
    const commandArguments = ['cloud', 'diff', taskId];
    if(validAttempt) {
      commandArguments.push('--attempt', attempt);
    }
    diff = runCommand('codex', commandArguments, target.rootDirectory);
  }

  const temporaryDirectory = await mkdtemp(resolve(tmpdir(), `${target.id}-fetch-`));
  try {
    const bundle = await extractDelivery(diff, temporaryDirectory);
    const base = bundle.get(BASE_NAME).trim();
    const filePaths = validateManifest(bundle);
    verifyPatch(base, temporaryDirectory);
    await publishDelivery(bundle);
    console.log(`Downloaded and verified ${filePaths.length} ${target.label} file changes against base ${base}.`);
    console.log(`Delivery saved in ignored .patches/. Apply separately with npm run apply-${target.id}-patch.`);
  } finally {
    await rm(temporaryDirectory, {recursive: true, force: true});
  }

  /**
   * Let Git decode the outer task diff in a disposable repository, allowing only the four delivery paths.
   * @param {string} diff
   * @param {string} directory
   * @returns {Promise<Map<string, string>>}
   */
  async function extractDelivery(diff, directory) {
    runGit(directory, ['init', '--quiet']);
    const includes = DELIVERY_NAMES.map(name => `--include=.patches/${name}`);
    runGit(directory, ['apply', '--whitespace=nowarn', ...includes, '-'], diff);
    const bundle = new Map();
    for(const name of DELIVERY_NAMES) {
      const filePath = resolve(directory, '.patches', name);
      const information = await lstat(filePath).catch(error => {
        if(error.code === 'ENOENT') {
          return null;
        }
        throw error;
      });
      if(!information?.isFile()) {
        throw new Error(`Cloud delivery is missing a regular .patches/${name} file. Ask the cloud task to return the complete bundle.`);
      }
      const contents = await readFile(filePath, 'utf8');
      if(!contents.trim()) {
        throw new Error(`Cloud delivery contains an empty .patches/${name} file.`);
      }
      bundle.set(name, contents);
    }
    return bundle;
  }

  /**
   * Check that the manifest names exactly the paths Git will apply, including binary files and deletions.
   * @param {Map<string, string>} bundle
   * @returns {string[]}
   */
  function validateManifest(bundle) {
    const paths = validatePatchScope(target, bundle.get(PATCH_NAME)).sort();
    const manifest = bundle.get(MANIFEST_NAME).trim().split(/\r?\n/).sort();
    if(paths.length === 0 || paths.length !== manifest.length || paths.some((filePath, index) => filePath !== manifest[index])) {
      throw new Error('Cloud delivery manifest does not match the repository patch. No local delivery files were replaced.');
    }
    return paths;
  }

  /**
   * Apply against the recorded commit in a disposable shared clone; never checkout, stage, or apply in the real repository.
   * @param {string} base
   * @param {string} directory
   */
  function verifyPatch(base, directory) {
    if(!/^[a-f0-9]{40}$/i.test(base)) {
      throw new Error('Cloud delivery has an invalid base revision; a full repository commit SHA is required.');
    }
    const snapshot = resolve(directory, 'repository-base');
    runGit(directory, ['clone', '--shared', '--no-checkout', '--quiet', target.repositoryDirectory, snapshot]);
    runGit(snapshot, ['checkout', '--quiet', '--detach', base]);
    const patchPath = resolve(directory, '.patches', PATCH_NAME);
    runGit(snapshot, ['apply', '--check', '--whitespace=error', patchPath]);
    runGit(snapshot, ['apply', '--whitespace=error', patchPath]);
    runGit(snapshot, ['apply', '--reverse', '--check', patchPath]);
  }

  /**
   * Preserve different pending work, then publish verified metadata before its patch. Stale reports alone are replaceable.
   * @param {Map<string, string>} bundle
   */
  async function publishDelivery(bundle) {
    await mkdir(target.deliveryDirectory, {recursive: true});
    if(!(await lstat(target.deliveryDirectory)).isDirectory()) {
      throw new Error('The local .patches path must be a directory, not a symlink.');
    }
    const pendingPatch = await readFile(resolve(target.deliveryDirectory, PATCH_NAME), 'utf8').catch(error => {
      if(error.code === 'ENOENT') {
        return null;
      }
      throw error;
    });
    if(pendingPatch !== null && pendingPatch !== bundle.get(PATCH_NAME)) {
      throw new Error(`A different ${target.label} patch is pending. Apply it or move its delivery files before fetching another task.`);
    }

    const stagingDirectory = await mkdtemp(resolve(target.deliveryDirectory, '.fetch-'));
    try {
      for(const name of DELIVERY_NAMES) {
        await writeFile(resolve(stagingDirectory, name), bundle.get(name));
      }
      for(const name of DELIVERY_NAMES) {
        await rename(resolve(stagingDirectory, name), resolve(target.deliveryDirectory, name));
      }
    } finally {
      await rm(stagingDirectory, {recursive: true, force: true});
    }
  }

  /**
   * Run Git without shell expansion in the specified real or disposable repository.
   * @param {string} directory
   * @param {string[]} arguments_
   * @param {string | undefined} [input]
   * @returns {string}
   */
  function runGit(directory, arguments_, input) {
    return runPatchGit(target, arguments_, true, directory, input).stdout;
  }

  /**
   * Bound captured output and surface CLI failures before any local delivery is published.
   * @param {string} executable
   * @param {string[]} arguments_
   * @param {string} directory
   * @param {string | undefined} [input]
   * @returns {string}
   */
  function runCommand(executable, arguments_, directory, input) {
    const result = spawnSync(executable, arguments_, {cwd: directory, encoding: 'utf8', input, maxBuffer: 64*1024*1024});
    if(result.error || result.status !== 0) {
      throw new Error(result.error?.message ?? (result.stderr || `${executable} failed.`));
    }
    return result.stdout;
  }
}
