// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { spawnSync } from 'node:child_process';
import { resolve } from 'node:path';

/**
 * @typedef {object} PatchTarget
 * @property {string} id Delivery filename prefix and npm command target.
 * @property {string} label Human-readable repository name.
 * @property {string} rootDirectory Standalone repository or hosting workspace owning the delivery; never staged.
 * @property {string} repositoryDirectory Repository whose files the patch changes.
 * @property {string} deliveryDirectory Ignored local transport directory.
 * @property {string} patchName
 * @property {string} baseName
 * @property {string} manifestName
 * @property {string} testsName
 * @property {string[]} deliveryNames Metadata first and patch last, for publication ordering.
 * @property {string[]} protectedPaths Repository-relative paths excluded from source patches.
 */

/**
 * Describe a delivery without changing files, staging, or refs. The caller owns repository layout and naming.
 * The transport directory is always excluded from source patches; hosts can protect additional repository paths.
 * @param {object} options
 * @param {string} options.id Delivery filename prefix and npm command target.
 * @param {string} options.label Human-readable repository name.
 * @param {string} options.rootDirectory Directory owning .patches/ and the cloud CLI context.
 * @param {string} [options.repositoryDirectory] Source repository; defaults to rootDirectory for standalone use.
 * @param {string[]} [options.protectedPaths] Additional repository-relative paths excluded from patches.
 * @returns {PatchTarget} New target record with absolute directories and its own protected-path list.
 */
export function createPatchTarget({id, label, rootDirectory, repositoryDirectory = rootDirectory, protectedPaths = []}) {
  const patchName = `${id}.patch`;
  const baseName = `${id}-base.txt`;
  const manifestName = `${id}-manifest.txt`;
  const testsName = `${id}-tests.txt`;
  return {
    id,
    label,
    rootDirectory: resolve(rootDirectory),
    repositoryDirectory: resolve(repositoryDirectory),
    deliveryDirectory: resolve(rootDirectory, '.patches'),
    patchName,
    baseName,
    manifestName,
    testsName,
    deliveryNames: [baseName, manifestName, testsName, patchName],
    protectedPaths: [...new Set(['.patches', ...protectedPaths])]
  };
}

/**
 * Parse Git's NUL-delimited destination paths, preserving whitespace and quoted names.
 * @param {PatchTarget} target
 * @param {string} patch
 * @param {boolean} [reverse] Read original paths by interpreting the reversed diff.
 * @returns {string[]}
 */
export function readPatchPaths(target, patch, reverse = false) {
  const arguments_ = ['apply', '--numstat', '-z'];
  if(reverse) {
    arguments_.push('--reverse');
  }
  const records = runPatchGit(target, arguments_, true, undefined, patch).stdout.split('\0').filter(Boolean);
  const paths = records.map(record => record.slice(record.indexOf('\t', record.indexOf('\t') + 1) + 1));
  if(paths.length === 0) {
    throw new Error('The delivery contains no file changes.');
  }
  return [...new Set(paths)];
}

/**
 * Refuse transport-file mutations and repository-boundary crossings before touching live source.
 * Git decodes quoted paths in both directions, exposing rename/copy sources without parsing headers ourselves.
 * @param {PatchTarget} target
 * @param {string} patch
 * @returns {string[]}
 */
export function validatePatchScope(target, patch) {
  if(/^(?:(?:new|deleted) file mode 160000|(?:old|new) mode 160000|index [^\n]* 160000)$/m.test(patch)) {
    throw new Error(`${target.label} patches cannot change submodule pointers. Keep repository management separate.`);
  }
  const paths = readPatchPaths(target, patch);
  const sourcePaths = readPatchPaths(target, patch, true);
  for(const protectedPath of target.protectedPaths) {
    if([...paths, ...sourcePaths].some(filePath => filePath === protectedPath || filePath.startsWith(`${protectedPath}/`))) {
      throw new Error(`${target.label} patch touches protected path ${protectedPath}. Return that work in the appropriate separate delivery.`);
    }
  }
  return paths;
}

/**
 * Run Git without shell expansion; only disposable repositories receive index/ref mutations.
 * @param {PatchTarget} target
 * @param {string[]} arguments_
 * @param {boolean} [required]
 * @param {string} [directory]
 * @param {string} [input]
 * @param {Record<string, string>} [environment]
 */
export function runPatchGit(target, arguments_, required = true, directory = target.repositoryDirectory, input = undefined, environment = {}) {
  const result = spawnSync('git', ['-C', directory, ...arguments_], {encoding: 'utf8', input, env: {...process.env, ...environment}, maxBuffer: 64*1024*1024});
  if(required && (result.error || result.status !== 0)) {
    throw new Error(result.error?.message ?? (result.stderr || `git ${arguments_.join(' ')} failed.`));
  }
  return result;
}
