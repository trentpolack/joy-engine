// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { ScriptLanguageCatalog, ScriptLanguageSymbol } from '../scripting/language-information.ts';
import { FORM_COMMAND, FORM_KEYWORD } from './constants.ts';

const FORM_FUNCTION_SIGNATURES: Readonly<Record<string, readonly string[]>> = Object.freeze({
  vector: ['vector(x, y, z)'],
  rgba: ['rgba(r, g, b, alpha)'],
  noise: ['noise(value[, seed])'],
  smoothstep: ['smoothstep(edge0, edge1, value)'],
  meshBox: ['meshBox(width, height, depth)'],
  meshSphere: ['meshSphere(radius[, segments])'],
  materialPbr: ['materialPbr(color, metallic, roughness)'],
  grid: ['grid(columns, rows, width, depth)'],
  box: ['box(width, height, depth)'],
  sphere: ['sphere(radius[, segments])'],
  output: ['output(name, geometry)'],
  inspect: ['inspect(name, value)']
});

const formCommandNames = new Set<string>(Object.values(FORM_COMMAND));
const keywordSymbols = Object.values(FORM_KEYWORD).filter(name => !formCommandNames.has(name)).map((name): ScriptLanguageSymbol => Object.freeze({
  name,
  kind: 'keyword',
  detail: 'FORM keyword',
  documentation: `The ${name} keyword is part of the FORM language grammar.`
}));

const commandSymbols = Object.values(FORM_COMMAND).map((name): ScriptLanguageSymbol => {
  const signatures = FORM_FUNCTION_SIGNATURES[name] ?? [`${name}(…)`];
  return Object.freeze({
    name,
    kind: 'function',
    detail: signatures.join(' | '),
    documentation: `The ${name} built-in is evaluated by the bounded FORM runtime.`,
    signatures
  });
});

/** Built-in FORM language information shared by editor and documentation adapters. */
export const FORM_LANGUAGE_CATALOG: ScriptLanguageCatalog = Object.freeze({
  id: 'form',
  symbols: Object.freeze([...keywordSymbols, ...commandSymbols])
});
