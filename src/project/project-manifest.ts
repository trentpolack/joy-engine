// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

const MANIFEST_FILE = 'project.joyproject';
const IDENTIFIER = /^[a-z][a-z0-9-]*$/;

export interface ProjectCapability {
  id: string;
  label: string;
  extensions: string[];
  assetRoot: string;
  renderer: string;
  features: Record<string, boolean>;
}
export interface ProjectManifest {
  $schema?: string;
  schemaVersion: 1;
  name: string;
  preview: {
      root: string;
      entry: string;
  };
  capabilities: ProjectCapability[];
}

/** The filename shared by browser directory handles and the desktop host. */
export const JOY_PROJECT_MANIFEST = MANIFEST_FILE;

/**
 * Validate and copy a project-owned editor manifest. Paths remain relative to the
 * selected project directory; consumers must enforce that boundary when reading.
 * Optional $schema metadata is preserved for authoring tools, never fetched at runtime.
 * @param value
 */
export function parseProjectManifest(value: unknown): ProjectManifest {
  if(!isRecord(value) || value.schemaVersion !== 1) {
    throw new TypeError('Joy project manifest must use schemaVersion 1.');
  }
  const name = requiredText(value.name, 'name');
  const schema = Object.hasOwn(value, '$schema') ? requiredText(value.$schema, '$schema') : undefined;
  if(!isRecord(value.preview)) {
    throw new TypeError('Joy project manifest requires a preview object.');
  }
  const preview = {
    root: safeRelativePath(requiredText(value.preview.root, 'preview.root')),
    entry: safeRelativePath(requiredText(value.preview.entry, 'preview.entry'))
  };
  if(!Array.isArray(value.capabilities)) {
    throw new TypeError('Joy project manifest capabilities must be an array.');
  }
  const ids = new Set();
  const capabilities = value.capabilities.map((item, index) => {
    if(!isRecord(item)) {
      throw new TypeError(`Capability ${index + 1} must be an object.`);
    }
    const id = requiredText(item.id, `capabilities[${index}].id`);
    if(!IDENTIFIER.test(id) || ids.has(id)) {
      throw new TypeError(`Capability id "${id}" must be unique kebab-case.`);
    }
    ids.add(id);
    if(!Array.isArray(item.extensions) || item.extensions.length === 0) {
      throw new TypeError(`Capability "${id}" requires at least one extension.`);
    }
    const extensions = item.extensions.map(extension => normalizeExtension(extension, id));
    const assetRoot = safeRelativePath(requiredText(item.assetRoot, `capability ${id} assetRoot`));
    const renderer = requiredText(item.renderer, `capability ${id} renderer`);
    const features = isRecord(item.features)
      ? Object.fromEntries(Object.entries(item.features).map(([feature, enabled]) => [feature, Boolean(enabled)]))
      : {};
    return {id, label: requiredText(item.label, `capability ${id} label`), extensions, assetRoot, renderer, features};
  });

  const manifest: ProjectManifest = {schemaVersion: 1, name, preview, capabilities};
  return schema === undefined ? manifest : {$schema: schema, ...manifest};
}

/**
 * Trim required manifest text and report its field when validation fails.
 * @param value
 * @param field
 */
function requiredText(value: unknown, field: string) {
  if(typeof value !== 'string' || value.trim() === '') {
    throw new TypeError(`Joy project manifest ${field} must be a non-empty string.`);
  }
  return value.trim();
}

/**
 * Normalize separators and reject absolute paths or traversal before host resolution.
 * @param value
 */
function safeRelativePath(value: string) {
  const path = value.replaceAll('\\', '/');
  if(path.startsWith('/') || path.split('/').some(part => part === '..') || /^[a-z]:/i.test(path)) {
    throw new TypeError(`Project path must stay inside the selected folder: ${value}`);
  }
  return path.replace(/^\.\//, '');
}

/**
 * Normalize a capability extension to lowercase dot notation.
 * @param value
 * @param id Capability name used in validation errors.
 */
function normalizeExtension(value: unknown, id: string) {
  const extension = requiredText(value, `capability ${id} extension`).toLowerCase();
  if(!/^\.[a-z0-9]+$/.test(extension)) {
    throw new TypeError(`Capability "${id}" has invalid extension "${extension}".`);
  }
  return extension;
}

/**
 * Narrow manifest objects while excluding null and arrays.
 * @param value
 */
function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}
