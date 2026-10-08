// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { EditorView, keymap, lineNumbers, highlightActiveLine, highlightActiveLineGutter, drawSelection } from '@codemirror/view';
import { tags } from '@lezer/highlight';
import type { Extension } from '@codemirror/state';
import { EditorState } from '@codemirror/state';
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
import { syntaxHighlighting, HighlightStyle, bracketMatching } from '@codemirror/language';

export { StreamLanguage } from '@codemirror/language';

/**
 * Create an owned CodeMirror view with the shared lab theme, history, and keyboard controls.
 * The caller owns view.destroy(); language and callbacks are borrowed.
 * @param options
 */
export function createScriptEditor(options: {
    parent: HTMLElement;
    source: string;
    language: Extension;
    label: string;
    readOnly?: boolean;
    background?: string;
    onChange: () => void;
    onRun: () => void;
    onCursor: (line: number, column: number) => void;
}) {
  return new EditorView({ parent: options.parent, state: createScriptEditorState(options) });
}

/**
 * Create replacement state to reset history when the owner switches documents.
 * @param options
 */
export function createScriptEditorState({ source, language, label, onChange, onRun, onCursor, readOnly = false, background = 'var(--joy-script, #0c1522)' }: Parameters<typeof createScriptEditor>[0]) {
  return EditorState.create({
    doc: source,
    extensions: [
      EditorState.readOnly.of(readOnly),
      EditorView.editable.of(!readOnly),
      lineNumbers(),
      drawSelection(),
      history(),
      highlightActiveLine(),
      highlightActiveLineGutter(),
      bracketMatching(),
      language,
      syntaxHighlighting(HighlightStyle.define([
        { tag: tags.comment, color: 'var(--joy-script-comment, #90a4bf)' },
        { tag: tags.keyword, color: 'var(--joy-script-keyword, #ff9b70)' },
        { tag: tags.number, color: 'var(--joy-script-number, #b0cf81)' },
        { tag: tags.typeName, color: 'var(--joy-script-function, #ffd166)' },
        { tag: tags.variableName, color: 'var(--joy-text, #e3e9f1)' },
        { tag: tags.special(tags.variableName), class: 'cm-script-parameter' },
        { tag: tags.operator, color: 'var(--joy-script-operator, #c4d4e8)' },
        { tag: tags.string, color: 'var(--joy-script-function, #ffd166)' },
        { tag: tags.bool, color: 'var(--joy-script-keyword, #ff9b70)' },
      ])),
      keymap.of([
        {
          key: 'Mod-Enter',
          run: () => {
            onRun();
            return true;
          },
        },
        ...defaultKeymap,
        ...historyKeymap,
        indentWithTab,
      ]),
      EditorView.updateListener.of(update => {
        if(update.docChanged) {
          onChange();
        }
        if(update.selectionSet || update.docChanged) {
          const position = update.state.selection.main.head;
          const line = update.state.doc.lineAt(position);
          onCursor(line.number, position - line.from + 1);
        }
      }),
      EditorView.contentAttributes.of({ 'aria-label': label, 'aria-readonly': String(readOnly), spellcheck: 'false', ...(readOnly ? { tabindex: '0' } : {}) }),
      EditorView.theme({
        '&': { backgroundColor: background, color: 'var(--joy-text, #e3e9f1)' },
        '.cm-gutters': { backgroundColor: background, color: '#697e99', border: 'none', minWidth: '36px' },
        '.cm-activeLineGutter': { color: 'var(--joy-orange, #ed6c39)', backgroundColor: 'var(--joy-surface, #21324a)' },
        '.cm-activeLine': { backgroundColor: 'color-mix(in srgb, var(--joy-surface, #21324a) 40%, transparent)' },
        '.cm-script-parameter': { color: 'var(--joy-script-parameter, #87dce3)' },
        '.cm-cursor': { borderLeftColor: 'var(--joy-cream, #fff3cf)' },
        '&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection': { backgroundColor: 'var(--joy-selection, #47607b)' },
      }, { dark: true }),
    ],
  });
}
