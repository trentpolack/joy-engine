// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { autocompletion, snippetCompletion } from '@codemirror/autocomplete';
import type { Completion } from '@codemirror/autocomplete';
import type { Extension } from '@codemirror/state';
import { hoverTooltip } from '@codemirror/view';
import type { ScriptLanguageCatalog, ScriptLanguageSymbol } from 'joy-engine/scripting';

const SCRIPT_WORD = /[A-Za-z_][A-Za-z_0-9]*/;

/** Convert editor-neutral language information into CodeMirror completion entries. */
export function createScriptCompletionOptions(catalog: ScriptLanguageCatalog): readonly Completion[] {
  return catalog.symbols.map(symbol => {
    const base = {
      label: symbol.name,
      type: completionType(symbol),
      detail: symbol.detail,
      info: symbol.documentation
    };
    if(symbol.kind !== 'function') {
      return base;
    }
    return snippetCompletion(symbol.name + '(${})', base);
  });
}

/** Find a catalog symbol by the exact source word under the pointer. */
export function findScriptSymbol(catalog: ScriptLanguageCatalog, word: string): ScriptLanguageSymbol | undefined {
  return catalog.symbols.find(symbol => symbol.name === word);
}

/** Create reusable completion and hover behavior for a Joy Engine scripting language. */
export function scriptLanguageTools(catalog: ScriptLanguageCatalog): Extension {
  const completionOptions = createScriptCompletionOptions(catalog);
  return [
    autocompletion({
      override: [context => {
        const word = context.matchBefore(SCRIPT_WORD);
        if(!word && !context.explicit) {
          return null;
        }
        return {
          from: word?.from ?? context.pos,
          options: completionOptions,
          validFor: SCRIPT_WORD
        };
      }]
    }),
    hoverTooltip((view, position) => {
      const line = view.state.doc.lineAt(position);
      const before = view.state.sliceDoc(line.from, position).match(/[A-Za-z_][A-Za-z_0-9]*$/)?.[0] ?? '';
      const after = view.state.sliceDoc(position, line.to).match(/^[A-Za-z_0-9]*/)?.[0] ?? '';
      const word = `${before}${after}`;
      const symbol = findScriptSymbol(catalog, word);
      if(!symbol) {
        return null;
      }
      return {
        pos: position - before.length,
        end: position + after.length,
        above: true,
        create: () => ({dom: createSymbolTooltip(symbol)})
      };
    })
  ];
}

function completionType(symbol: ScriptLanguageSymbol) {
  if(symbol.kind === 'property') {
    return 'property';
  }
  if(symbol.kind === 'variable') {
    return 'variable';
  }
  return symbol.kind;
}

function createSymbolTooltip(symbol: ScriptLanguageSymbol) {
  const tooltip = document.createElement('div');
  tooltip.className = 'cm-script-tooltip';

  const heading = document.createElement('strong');
  heading.textContent = symbol.signatures?.join(' | ') ?? symbol.name;
  tooltip.append(heading);

  const documentation = document.createElement('p');
  documentation.textContent = symbol.documentation;
  tooltip.append(documentation);
  return tooltip;
}
