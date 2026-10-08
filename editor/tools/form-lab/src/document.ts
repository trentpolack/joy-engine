// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { ParameterValue, Material } from 'joy-engine/form';

import { readParameterValue } from 'joy-engine/form';
import { readMaterials } from 'joy-engine/form';
import { DOCUMENT_LIMITS } from 'joy-engine/form';
export interface ParameterVariant {
  name: string;
  overrides: Record<string, ParameterValue>;
}
export interface FormDocument {
  version: 1;
  name: string;
  source: string;
  materials?: Material[];
  variants?: ParameterVariant[];
  overrides: Record<string, ParameterValue>;
}

/** Validate a saved document before it reaches the editor or worker. @param text */
export function readDocument(text: string): FormDocument {
  if(text.length > DOCUMENT_LIMITS.fileBytes || new TextEncoder().encode(text).byteLength > DOCUMENT_LIMITS.fileBytes) {
    throw new Error('FORM LAB documents must be 64 MB or smaller.');
  }
  const value = JSON.parse(text);
  if(
    !value ||
    value.version !== 1 ||
    typeof value.source !== 'string' ||
    value.source.length > 200000 ||
    typeof value.name !== 'string' ||
    value.name.length > 120
  ) {
    throw new Error('This is not a valid FORM LAB version 1 document.');
  }
  const overrides = readOverrides(value.overrides ?? {});
  const variants = value.variants === undefined ? {} : {variants: readVariants(value.variants)};
  const materials = value.materials === undefined ? {} : { materials: readMaterials(value.materials) };
  return { version: 1, name: value.name, source: value.source, overrides, ...materials, ...variants };
}

/** Copy and validate values at document and variant ownership boundaries. @param overrides */
export function readOverrides(overrides: unknown) {
  if(
    typeof overrides !== 'object' ||
    Array.isArray(overrides) ||
    overrides === null
  ) {
    throw new Error(
      'Document parameters must be finite scalars, XYZ vectors, or RGBA colors.',
    );
  }
  return Object.fromEntries(Object.entries(overrides).map(([name, value]) => [name, readParameterValue(value)]));
}
/** @param value */
export function readVariants(value: unknown): ParameterVariant[] {
  if(!Array.isArray(value) || value.length > 16) {
    throw new Error('A document supports up to 16 parameter variants.');
  }
  const names = new Set();
  return value.map(variant => {
    if(!variant || typeof variant.name !== 'string' || !variant.name.trim() || variant.name.length > 64 || names.has(variant.name.trim().toLowerCase())) {
      throw new Error('Variant names must be unique and contain 1–64 characters.');
    }
    names.add(variant.name.trim().toLowerCase());
    return {name: variant.name.trim(), overrides: readOverrides(variant.overrides)};
  });
}

/** Serialize only documents that can be reopened. Compact JSON avoids whitespace amplification of mesh data.
 * @param document
 */
export function serializeDocument(document: FormDocument) {
  const text = JSON.stringify(document);
  readDocument(text);
  return text;
}

/** Download a user-owned text artifact; release the temporary object URL.
 * @param name @param text @param [type]
 */
export function download(name: string, text: string | ArrayBuffer, type: string = 'text/plain') {
  const url = URL.createObjectURL(new Blob([text], { type }));
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = name;
  anchor.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
/** @param name */
export function fileStem(name: string) {
  return (
    name
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-|-$/g, '') || 'form-study'
  );
}

