// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { existsSync } from 'node:fs';
import { cp, lstat, mkdir, mkdtemp, readFile, rm } from 'node:fs/promises';
import { dirname, isAbsolute, relative, resolve, sep } from 'node:path';
import { tmpdir } from 'node:os';
import { spawnSync } from 'node:child_process';

import { readPatchPaths as readDeliveryPaths, runPatchGit, validatePatchScope } from './patch-delivery.mjs';

/**
 * Apply one verified delivery, reconciling conflicts outside the live repository and cleaning up only on success.
 * @param {import('./patch-delivery.mjs').PatchTarget} target
 * @param {string[]} [arguments_] Pass --no-mergetool for noninteractive failure handling.
 */
export async function applyProjectPatch(target, arguments_ = process.argv.slice(2)) {
  const REPOSITORY_DIRECTORY = target.repositoryDirectory;
  const PATCH_PATH = resolve(target.deliveryDirectory, target.patchName);
  const BASE_PATH = resolve(target.deliveryDirectory, target.baseName);
  const useMergetool = !arguments_.includes('--no-mergetool');
  if(arguments_.length > 1 || (arguments_.length === 1 && useMergetool)) {
    throw new Error(`Usage: npm run apply-${target.id}-patch -- [--no-mergetool]`);
  }

  if(!existsSync(PATCH_PATH)) {
    console.log(`No pending ${target.label} patch.`);
    return;
  }

  const patch = await readFile(PATCH_PATH, 'utf8');
  validatePatchScope(target, patch);

  const reverseCheck = runGit(['apply', '--reverse', '--check', PATCH_PATH], false);
  // Git reports mode mismatches as warnings with status 0; those are not proof of application.
  if(reverseCheck.status === 0 && !reverseCheck.stderr) {
    await removeAppliedPatchFiles();
    console.log(`${target.label} patch is already applied.`);
    return;
  }

  if(runGit(['apply', '--check', '--whitespace=error', PATCH_PATH], false).status === 0) {
    runGit(['apply', '--whitespace=error', PATCH_PATH], true);
    await removeAppliedPatchFiles();
    console.log(`Applied ${target.label} patch cleanly.`);
    return;
  }

  const baseRevision = existsSync(BASE_PATH) ? (await readFile(BASE_PATH, 'utf8')).trim() : 'unknown';
  const currentRevision = runGit(['rev-parse', 'HEAD'], true).stdout.trim();
  try {
    await reconcilePatch(patch);
    await removeAppliedPatchFiles();
    console.log(`Applied ${target.label} patch with three-way resolution (base ${baseRevision}, current ${currentRevision}).`);
  } catch(error) {
    console.error(`${target.label} patch could not be applied. Expected base ${baseRevision}; current repository revision is ${currentRevision}.`);
    console.error(error instanceof Error ? error.message : String(error));
    console.error('Patch files retained. Local files and staging were preserved during reconciliation.');
    process.exitCode = 1;
  }

  /**
   * Resolve against a disposable snapshot of the working files, including untracked additions.
   * Git's --3way implies --index, so it must never run against the user's index.
   * Only publish a complete delta after every file resolves without conflicts.
   * @param {string} patch
   */
  async function reconcilePatch(patch) {
    const filePatches = patch.split(/(?=^diff --git )/m).filter(part => {
      if(!part.trim()) {
        return false;
      }
      const fileReverseCheck = runGit(['apply', '--reverse', '--check'], false, REPOSITORY_DIRECTORY, part);
      return fileReverseCheck.status !== 0 || Boolean(fileReverseCheck.stderr);
    });
    if(filePatches.length === 0) {
      return;
    }
    const pendingPatch = filePatches.join('');
    const temporaryDirectory = await mkdtemp(resolve(tmpdir(), `${target.id}-patch-merge-`));
    const snapshotDirectory = resolve(temporaryDirectory, 'repository');
    let retainSnapshot = false;

    try {
      // Share history read-only; all new objects, index entries, and conflicts stay in this copy.
      runGit(['clone', '--shared', '--no-checkout', '--quiet', REPOSITORY_DIRECTORY, snapshotDirectory], true);
      runGit(['read-tree', 'HEAD'], true, snapshotDirectory);
      const ancestorIndex = resolve(temporaryDirectory, 'ancestor.index');
      runGit(['apply', `--build-fake-ancestor=${ancestorIndex}`], true, snapshotDirectory, pendingPatch);
      // apply --numstat lists destinations only; the fake ancestor supplies rename/copy sources.
      const originalPaths = runGit(['ls-files', '-z'], true, snapshotDirectory, undefined, {GIT_INDEX_FILE: ancestorIndex}).stdout.split('\0').filter(Boolean);
      const filePaths = [...new Set([...readPatchPaths(pendingPatch), ...originalPaths])];
      for(const filePath of filePaths) {
        await copyWorkingFile(filePath, snapshotDirectory);
      }
      runGit(['update-index', '--add', '--remove', '-z', '--stdin'], true, snapshotDirectory, `${filePaths.join('\0')}\0`);
      const originalTree = runGit(['write-tree'], true, snapshotDirectory).stdout.trim();

      for(const filePatch of filePatches) {
        const result = runGit(['apply', '--3way', '--whitespace=error'], false, snapshotDirectory, filePatch);
        if(result.status === 0) {
          continue;
        }
        const unmerged = runGit(['ls-files', '--unmerged', '-z'], true, snapshotDirectory).stdout;
        if(!useMergetool || !unmerged) {
          throw new Error(result.error?.message ?? (result.stderr || 'Three-way application failed.'));
        }
        const {tool, configurationArguments} = readMergetoolConfiguration();
        retainSnapshot = true;
        console.log(`Opening ${tool} for ${target.label} patch conflicts in the disposable checkout...`);
        const mergeResult = spawnSync('git', ['-C', snapshotDirectory, ...configurationArguments, 'mergetool', '--no-prompt'], {stdio: 'inherit'});
        if(mergeResult.error || mergeResult.status !== 0) {
          throw new Error(mergeResult.error?.message ?? `${tool} did not complete the merge.`);
        }
        if(runGit(['ls-files', '--unmerged', '-z'], true, snapshotDirectory).stdout) {
          throw new Error(`${tool} left unresolved conflicts. No source files were changed.`);
        }
      }

      // A tool can report success while leaving conflict markers; Git checks the staged result before publication.
      const patchPaths = ['--', ...filePaths];
      runGit(['--literal-pathspecs', 'diff', '--cached', '--check', originalTree, ...patchPaths], true, snapshotDirectory);
      const delta = runGit(['--literal-pathspecs', 'diff', '--cached', '--binary', '--full-index', originalTree, ...patchPaths], true, snapshotDirectory).stdout;
      if(!delta) {
        retainSnapshot = false;
        return;
      }
      // Plain apply leaves staging alone and refuses the complete delta if any hunk fails.
      runGit(['apply', '--check', '--whitespace=error'], true, REPOSITORY_DIRECTORY, delta);
      runGit(['apply', '--whitespace=error'], true, REPOSITORY_DIRECTORY, delta);
      retainSnapshot = false;
    } finally {
      if(retainSnapshot) {
        console.error(`Merge snapshot retained: ${snapshotDirectory}`);
      } else {
        await rm(temporaryDirectory, {recursive: true, force: true});
      }
    }
  }

  /**
   * Forward repository-local merge settings explicitly; clones inherit global configuration but not local tool commands.
   * @returns {{tool: string, configurationArguments: string[]}}
   */
  function readMergetoolConfiguration() {
    const tool = runGit(['config', '--get', 'merge.tool'], false).stdout.trim();
    if(!tool) {
      throw new Error('No Git merge tool is configured. Set merge.tool, or use --no-mergetool for noninteractive runs.');
    }
    const configurationArguments = ['-c', `merge.tool=${tool}`];
    const configuration = runGit(['config', '--null', '--get-regexp', '^mergetool\\.'], false).stdout;
    for(const record of configuration.split('\0').filter(Boolean)) {
      const separator = record.indexOf('\n');
      const setting = separator === -1 ? record : `${record.slice(0, separator)}=${record.slice(separator + 1)}`;
      configurationArguments.push('-c', setting);
    }
    return {tool, configurationArguments};
  }

  /**
   * Read Git's NUL-delimited destination paths without parsing quoted patch headers.
   * @param {string} patch
   * @returns {string[]}
   */
  function readPatchPaths(patch) {
    return readDeliveryPaths(target, patch);
  }

  /**
   * Copy one affected file without following directory symlinks outside the repository or snapshot.
   * Missing files represent local deletions and remain absent in the snapshot.
   * @param {string} filePath
   * @param {string} snapshotDirectory
   */
  async function copyWorkingFile(filePath, snapshotDirectory) {
    const sourcePath = resolve(REPOSITORY_DIRECTORY, filePath);
    const repositoryRelativePath = relative(REPOSITORY_DIRECTORY, sourcePath);
    const segments = repositoryRelativePath.split(sep);
    if(isAbsolute(repositoryRelativePath) || segments.includes('..') || segments.includes('.git') || !repositoryRelativePath) {
      throw new Error(`Unsafe repository patch path: ${filePath}`);
    }
    let parentPath = REPOSITORY_DIRECTORY;
    for(const segment of segments.slice(0, -1)) {
      parentPath = resolve(parentPath, segment);
      const information = await lstat(parentPath).catch(error => {
        if(error.code === 'ENOENT') {
          return null;
        }
        throw error;
      });
      if(!information) {
        return;
      }
      if(!information.isDirectory()) {
        throw new Error(`Repository patch path has a non-directory or symlink parent: ${filePath}`);
      }
    }
    const destinationPath = resolve(snapshotDirectory, repositoryRelativePath);
    await mkdir(dirname(destinationPath), {recursive: true});
    try {
      await cp(sourcePath, destinationPath, {verbatimSymlinks: true});
    } catch(error) {
      if(error.code !== 'ENOENT') {
        throw error;
      }
    }
  }

  /**
   * Remove this delivery's files only after Git confirms successful application.
   */
  async function removeAppliedPatchFiles() {
    // Remove metadata first so interrupted cleanup can still retry using the patch.
    for(const fileName of target.deliveryNames) {
      await rm(resolve(target.deliveryDirectory, fileName), {force: true});
    }
    console.log(`Removed applied ${target.label} patch files.`);
  }

  /**
   * Run Git in the repository or its disposable copy; optional input carries a patch without shell expansion.
   * @param {string[]} arguments_
   * @param {boolean} required
   * @param {string} directory
   * @param {string | undefined} input
   * @param {Record<string, string>} environment
   */
  function runGit(arguments_, required, directory = REPOSITORY_DIRECTORY, input = undefined, environment = {}) {
    return runPatchGit(target, arguments_, required, directory, input, environment);
  }
}
