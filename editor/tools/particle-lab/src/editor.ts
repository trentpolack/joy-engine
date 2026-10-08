// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { BROWSER_EVENT } from 'joy-engine/constants';
import type { StringStream } from '@codemirror/language';
import type { EditorView } from '@codemirror/view';
import { createScriptEditor, createScriptEditorState, StreamLanguage } from '@joy-games/joy-editor';

const particleLanguage = StreamLanguage.define({
  token(stream: StringStream) {
    if(stream.eatSpace()) {
      return null;
    }
    if(stream.match(/#.*|\/\/.*/)) {
      return 'comment';
    }
    if(stream.match(/(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?/)) {
      return 'number';
    }
    if(stream.match(/\b(?:let|if|else)\b/)) {
      return 'keyword';
    }
    if(stream.match(/\b(?:sin|cos|tan|abs|sqrt|floor|ceil|round|exp|log|min|max|pow|atan2|clamp|mix|smoothstep|noise|rand|vector|rgba)\b/)) {
      return 'typeName';
    }
    if(stream.match(/[A-Za-z_][A-Za-z_0-9]*/)) {
      return 'variableName';
    }
    stream.next();
    return 'operator';
  },
});

const jsonLanguage = StreamLanguage.define({
  token(stream: StringStream) {
    if(stream.eatSpace()) {
      return null;
    }
    if(stream.match(/"(?:[^"\\]|\\.)*"/)) {
      return 'string';
    }
    if(stream.match(/-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?/)) {
      return 'number';
    }
    if(stream.match(/\b(?:true|false|null)\b/)) {
      return 'bool';
    }
    stream.next();
    return 'operator';
  },
});

/** Owns one lab editor. Programmatic refreshes never become user edits or undo entries. */
export class ParticleScriptEditor {
  declare refreshing: boolean;
  declare status: HTMLDivElement;
  declare position: HTMLSpanElement;
  declare options: Parameters<typeof createScriptEditor>[0];
  declare view: EditorView;

  /** @param parent @param label @param json @param onChange @param onRun */
  constructor(parent: HTMLElement, label: string, json: boolean, onChange: () => void, onRun: () => void) {
    this.refreshing = false;
    this.status = document.createElement('div');
    this.status.className = 'editor-status';
    this.position = document.createElement('span');
    this.position.textContent = 'Ln 1, Col 1 · 1 line';
    this.status.append(this.position);
    if(!json) {
      const reference = document.createElement('button');
      reference.type = 'button';
      reference.className = 'editor-reference';
      reference.textContent = 'Script Reference';
      reference.addEventListener(BROWSER_EVENT.CLICK, () => {
         ((document.querySelector('#reference-dialog')) as HTMLDialogElement | null)?.showModal();
      });
      this.status.append(reference);
    }
    parent.after(this.status);
    this.options = {
      parent, source: '', label, language: json ? jsonLanguage : particleLanguage,
      readOnly: json,
      background: json ? 'var(--joy-surface)' : 'var(--joy-script)',
      onRun,
      onChange: () => {
        if(!this.refreshing) {
          onChange();
        }
      },
      onCursor: (line: number,  column: number) => {
        const lines = this.view.state.doc.lines;
        this.position.textContent = `Ln ${line}, Col ${column} · ${lines} ${lines === 1 ? 'line' : 'lines'}`;
      },
    };
    this.view = createScriptEditor(this.options);
  }

  get value() {
    return this.view.state.doc.toString();
  }

  /** @param source */
  set value(source) {
    if(source === this.value) {
      return;
    }
    this.refreshing = true;
    try {
      // A document/emitter switch starts a new history, preserving editor extensions.
      this.view.setState(createScriptEditorState({ ...this.options, source }));
      this.options.onCursor(1, 1);
    } finally {
      this.refreshing = false;
    }
  }

  destroy() {
    this.view.destroy();
    this.status.remove();
  }
}
