// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/** Editor-neutral symbol categories shared by Joy Engine scripting languages. */
export type ScriptSymbolKind = 'keyword' | 'function' | 'variable' | 'property';

/**
 * Immutable language information borrowed by editors, documentation, and other tooling.
 * Signatures are display text; consumers must not infer runtime behavior from them.
 */
export interface ScriptLanguageSymbol {
  name: string;
  kind: ScriptSymbolKind;
  detail: string;
  documentation: string;
  signatures?: readonly string[];
  writable?: boolean;
}

/** A complete, editor-neutral view of one scripting language's built-in symbols. */
export interface ScriptLanguageCatalog {
  id: string;
  symbols: readonly ScriptLanguageSymbol[];
}
