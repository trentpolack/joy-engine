// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { existsSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import path from 'node:path';

// Workspace-level settings file; its directory is the authoring workspace root.
export const WORKSPACE_CONFIG_FILE = 'joy.joyconfig';

const productionDefaults = {joyEditor: false, formEditor: true, joyfxEditor: true};
const editorKeys = {
  'joy-editor': 'joyEditor',
  'form-editor': 'formEditor',
  'joyfx-editor': 'joyfxEditor'
};

// Settings keys from before FORM LAB and PARTICLE LAB became FORM EDITOR and JOYFX EDITOR. Existing
// workspace configs keep their production flags; the current key wins when a config holds both.
const legacyEditorKeys = {formLab: 'formEditor', particleLab: 'joyfxEditor'};

/**
 * Find the nearest directory at or above startDirectory that holds joy.joyconfig.
 * Joy Editor is hosted by a game workspace (JoyGames today, a separate checkout after
 * the repository split), so the editor locates that workspace instead of assuming its
 * own position in a monorepo.
 * @param {string} startDirectory Absolute or cwd-relative directory to search from.
 * @returns {string | null} The workspace root, or null when no workspace config exists.
 */
export function findWorkspaceRoot(startDirectory) {
  let directory = path.resolve(startDirectory);
  while(true) {
    if(existsSync(path.join(directory, WORKSPACE_CONFIG_FILE))) {
      return directory;
    }
    const parent = path.dirname(directory);
    if(parent === directory) {
      return null;
    }
    directory = parent;
  }
}

/**
 * Read build-time availability. Development always enables all editors; the
 * workspace requires both tool editors regardless of their individual production flags.
 * Missing settings use defaults, but malformed settings fail the build.
 * @param {string | null} [rootDirectory] Workspace containing joy.joyconfig; defaults to
 * the nearest one above the current directory, and null uses the defaults.
 * @param {boolean} [development] Whether this is a development session.
 * @returns {Promise<{joyEditor:boolean, formEditor:boolean, joyfxEditor:boolean}>}
 */
export async function readEditorPolicy(rootDirectory = findWorkspaceRoot(process.cwd()), development = false) {
  let config = {};
  if(rootDirectory) {
    try {
      config = JSON.parse(await readFile(path.join(rootDirectory, WORKSPACE_CONFIG_FILE), 'utf8'));
    } catch(error) {
      if(error.code !== 'ENOENT') {
        throw error;
      }
    }
  }
  assertRecord(config, WORKSPACE_CONFIG_FILE);
  const editors = config.editors ?? {};
  assertRecord(editors, 'editors');
  const policy = {...productionDefaults};
  for(const name of Object.keys(editors)) {
    const key = legacyEditorKeys[name] ?? name;
    if(!Object.hasOwn(productionDefaults, key)) {
      throw new Error(`Unknown editor setting: editors.${name}`);
    }
    const settings = editors[name];
    assertRecord(settings, `editors.${name}`);
    for(const setting of Object.keys(settings)) {
      if(setting !== 'productionEnabled') {
        throw new Error(`Unknown editor setting: editors.${name}.${setting}`);
      }
    }
    const supersededByCurrentKey = (key !== name) && Object.hasOwn(editors, key);
    if(supersededByCurrentKey) {
      continue;
    }
    if(Object.hasOwn(settings, 'productionEnabled')) {
      if(typeof settings.productionEnabled !== 'boolean') {
        throw new Error(`editors.${name}.productionEnabled must be a boolean.`);
      }
      policy[key] = settings.productionEnabled;
    }
  }
  if(development) {
    return {joyEditor: true, formEditor: true, joyfxEditor: true};
  }
  if(policy.joyEditor) {
    policy.formEditor = true;
    policy.joyfxEditor = true;
  }
  return policy;
}

/**
 * Apply the policy only to known editor workspaces; games remain independent.
 * @param {string} slug Workspace directory name.
 * @param {Awaited<ReturnType<typeof readEditorPolicy>>} policy Resolved flags.
 * @returns {boolean}
 */
export function isEditorEnabled(slug, policy) {
  const key = editorKeys[slug];
  return key ? policy[key] : true;
}

/** @param {unknown} value @param {string} label */
function assertRecord(value, label) {
  if(!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error(`${label} must be an object.`);
  }
}
