// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { ScriptLanguageCatalog, ScriptLanguageSymbol } from '../scripting/language-information.ts';
import { PARTICLE_VALUE_KIND } from './constants.ts';
import { PARTICLE_FIELDS, PARTICLE_FUNCTION_ARITIES, PARTICLE_INPUTS } from './script.ts';

const KEYWORDS = Object.freeze(['let', 'if', 'else']);

const keywordSymbols = KEYWORDS.map((name): ScriptLanguageSymbol => Object.freeze({
  name,
  kind: 'keyword',
  detail: 'Particle script keyword',
  documentation: `The ${name} keyword controls bounded particle-script execution.`
}));

const functionSymbols = Object.entries(PARTICLE_FUNCTION_ARITIES).map(([name, arities]): ScriptLanguageSymbol => {
  const signatures = arities.map(arity => `${name}(${Array.from({length: arity}, (_, index) => `value${index + 1}`).join(', ')})`);
  const returnKind = name === PARTICLE_VALUE_KIND.VECTOR ? PARTICLE_VALUE_KIND.VECTOR : name === 'rgba' ? PARTICLE_VALUE_KIND.COLOR : PARTICLE_VALUE_KIND.SCALAR;
  return Object.freeze({
    name,
    kind: 'function',
    detail: signatures.join(' | '),
    documentation: `The ${name} built-in returns a bounded ${returnKind} value.`,
    signatures
  });
});

const fieldSymbols = PARTICLE_FIELDS.map((name): ScriptLanguageSymbol => Object.freeze({
  name,
  kind: 'property',
  detail: 'Writable particle scalar',
  documentation: `Reads or updates the current particle's ${name} value.`,
  writable: true
}));

const inputSymbols = PARTICLE_INPUTS.map((name): ScriptLanguageSymbol => Object.freeze({
  name,
  kind: 'variable',
  detail: 'Read-only particle input',
  documentation: `Reads the ${name} value supplied by the particle simulation.`,
  writable: false
}));

/** Built-in particle language information shared by editor and documentation adapters. */
export const PARTICLE_LANGUAGE_CATALOG: ScriptLanguageCatalog = Object.freeze({
  id: 'particle',
  symbols: Object.freeze([...keywordSymbols, ...functionSymbols, ...fieldSymbols, ...inputSymbols])
});
