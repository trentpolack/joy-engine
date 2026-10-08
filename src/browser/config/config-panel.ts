// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { BROWSER_EVENT } from '../constants.ts';
import { KEY_CODE } from '../input/constants.ts';
import type { ConfigSession } from './config-session.ts';
import type { ConfigField, ConfigApplyMode } from '../../core/config-values.ts';
import styles from './config-panel.css?raw';
import launcherStyles from '../editor-launchers.css?raw';
import themeStyles from '../dev-widget-theme.css?raw';
import { announceEditorPanelOpen, EDITOR_PANEL_OPEN_EVENT, isAnotherEditorOpening } from '../editor-launchers.ts';
import { requireElement } from '../dom.ts';

export type { ConfigSession };

let nextPanelId = 1;

export interface ConfigPanelOptions {
  title: string;
  session: ConfigSession;
  onRestart?: () => void | Promise<void>;
  restartLabel?: string;
}

/**
 * Own the development-only configuration editor DOM and listeners. */
export class ConfigPanel {
  declare session: ConfigSession;
  declare onRestart: (() => void | Promise<void>) | undefined;
  declare view: ReturnType<typeof createView>;
  declare destroyed: boolean;
  declare pendingDrafts: Set<HTMLInputElement>;
  declare onEditEnd: () => void;
  declare onClick: (event: Event) => void;
  declare onInput: (event: Event) => void;
  declare onChange: (event: Event) => void;
  declare onKeyDown: (event: KeyboardEvent) => void;
  declare onDocumentKeyDown: (event: KeyboardEvent) => void;
  declare onEditorPanelOpen: (event: Event) => void;
  declare unsubscribe: () => void;

  /**
   * @param options
   * @param options.title Panel heading.
   * @param options.session Borrowed editing session; its creator owns destruction.
   * @param [options.onRestart] Optional scenario restart adapter.
   * @param [options.restartLabel] Optional restart button label.
   */
  constructor({ title, session, onRestart, restartLabel = 'Restart Scenario' }: ConfigPanelOptions) {
    this.session = session;
    this.onRestart = onRestart;
    this.view = createView(title, session.fields, Boolean(onRestart), restartLabel);
    this.destroyed = false;

    this.pendingDrafts = new Set();
    this.onEditEnd = () => this.session.endEdit();
    this.onClick = this.handleClick.bind(this);
    this.onInput = this.handleInput.bind(this);
    this.onChange = this.handleChange.bind(this);
    this.onKeyDown = this.handleKeyDown.bind(this);
    this.onDocumentKeyDown = this.handleDocumentKeyDown.bind(this);
    this.onEditorPanelOpen = this.handleEditorPanelOpen.bind(this);
    this.unsubscribe = session.subscribe(() => this.render());
    this.view.root.addEventListener(BROWSER_EVENT.FOCUSOUT, this.onEditEnd);
    this.view.root.addEventListener(BROWSER_EVENT.POINTERCANCEL, this.onEditEnd);
    this.view.root.addEventListener(BROWSER_EVENT.CLICK, this.onClick);
    this.view.root.addEventListener(BROWSER_EVENT.INPUT, this.onInput);
    this.view.root.addEventListener(BROWSER_EVENT.CHANGE, this.onChange);
    this.view.root.addEventListener(BROWSER_EVENT.KEYDOWN, this.onKeyDown);
    document.addEventListener(BROWSER_EVENT.KEYDOWN, this.onDocumentKeyDown);
    document.addEventListener(EDITOR_PANEL_OPEN_EVENT, this.onEditorPanelOpen);
    this.render();
  }

  /**
   * Release owned DOM, style, listeners, and subscription. */
  destroy() {
    if(this.destroyed) {
      return;
    }
    this.session.endEdit();
    this.view.root.removeEventListener(BROWSER_EVENT.FOCUSOUT, this.onEditEnd);
    this.view.root.removeEventListener(BROWSER_EVENT.POINTERCANCEL, this.onEditEnd);
    this.view.root.removeEventListener(BROWSER_EVENT.CLICK, this.onClick);
    this.view.root.removeEventListener(BROWSER_EVENT.INPUT, this.onInput);
    this.view.root.removeEventListener(BROWSER_EVENT.CHANGE, this.onChange);
    this.view.root.removeEventListener(BROWSER_EVENT.KEYDOWN, this.onKeyDown);
    document.removeEventListener(BROWSER_EVENT.KEYDOWN, this.onDocumentKeyDown);
    document.removeEventListener(EDITOR_PANEL_OPEN_EVENT, this.onEditorPanelOpen);
    this.unsubscribe();
    this.view.root.remove();
    this.view.style.remove();
    this.destroyed = true;
  }

  /**
   * Refresh fields from session snapshots while preserving focused numeric drafts.
   * @private
   */
  private render() {
    const values = this.session.values;
    for(const field of this.session.fields) {
      const row = requireElement(this.view.fields, `[data-field-row="${CSS.escape(field.key)}"]`, HTMLElement);
      const range = requireElement(row, 'input[type="range"]', HTMLInputElement);
      const number = requireElement(row, 'input[type="number"]', HTMLInputElement);
      const marker = requireElement(row, '[data-config-marker]', HTMLElement);
      range.value = String(values[field.key]);
      // Do not collapse an in-progress multi-digit or decimal draft when an
      // unrelated session notification re-renders the panel.
      if(!this.pendingDrafts.has(number)) {
        number.value = String(values[field.key]);
        number.setAttribute('aria-invalid', 'false');
      }
      const isDefault = values[field.key] === field.defaultValue;
      const isStored = values[field.key] === this.session.storedValues[field.key];
      if(!isStored) {
        marker.textContent = 'Unsaved';
      } else if(isDefault) {
        marker.textContent = 'Default';
      } else {
        marker.textContent = 'Stored override';
      }
      row.dataset.changed = String(!isStored);
    }
    this.view.undo.disabled = !this.session.canUndo;
    this.view.redo.disabled = !this.session.canRedo;
    this.view.save.hidden = !this.session.canSave;
    this.view.revert.disabled = !this.session.dirty;
    this.view.save.disabled = !this.session.dirty || this.session.saving;
    this.view.save.textContent = this.session.saving ? 'Saving…' : 'Save';
    this.view.state.textContent = !this.session.canSave ? 'SESSION ONLY — changes are not persisted' : this.session.dirty ? 'Unsaved changes' : 'No unsaved edits';
    this.view.state.dataset.dirty = String(this.session.dirty);
  }

  /**
   * Route delegated button actions through the editing session or lifecycle adapters.
   * @private
   * @param event
   */
  private handleClick(event: Event) {
    const target = event.target;
    if(!(target instanceof Element)) {
      return;
    }
    const action = target.closest('[data-config-action]')?.getAttribute('data-config-action');
    if(!action) {
      return;
    }
    if(action === 'toggle') {
      const expanded = Boolean(this.view.panel.hidden);
      this.setPanelOpen(expanded);
      if(expanded) {
        announceEditorPanelOpen(this.view.root);
        this.view.search.focus();
      }
      return;
    }
    if(['undo', 'redo', 'reset-all', 'reset-field', 'revert'].includes(action)) {
      this.pendingDrafts.clear();
    }
    if(action === 'redo') {
      this.session.redo();
    }
    if(action === 'undo') {
      this.session.undo();
    }
    if(action === 'revert') {
      this.session.revert();
    }
    if(action === 'reset-all') {
      this.session.resetAll();
    }
    if(action === 'reset-field') {
      const key = target.closest('[data-field-row]')?.getAttribute('data-field-row');
      if(key) {
        this.session.resetField(key);
      }
    }
    if(action === 'save') {
      void this.persist();
    }
    if(action === 'restart') {
      void this.restart();
    }
    this.render();
  }

  /**
   * Preview slider changes immediately while leaving typed numeric drafts uncommitted.
   * @private
   * @param event
   */
  private handleInput(event: Event) {
    const target = event.target;
    if(!(target instanceof HTMLInputElement)) {
      return;
    }
    if(target === this.view.search) {
      this.filter(target.value);
      return;
    }
    const key = target.dataset.configKey;
    if(!key) {
      return;
    }
    if(target.type === 'number') {
      this.pendingDrafts.add(target);
      const incomplete = target.value.trim() === '' || !Number.isFinite(target.valueAsNumber);
      target.setAttribute('aria-invalid', String(incomplete));
      this.setStatus(incomplete ? 'Enter a valid number to apply this setting.' : 'Value ready; press Enter or leave the field to apply.', incomplete);
      return;
    }
    this.session.beginEdit();
    this.commitNumericInput(target, key);
  }

  /**
   * Commit a completed number field after its change event.
   * @private
   * @param event
   */
  private handleChange(event: Event) {
    const target = event.target;
    if(!(target instanceof HTMLInputElement)) {
      return;
    }
    const key = target.dataset.configKey;
    if(!key) {
      return;
    }
    this.commitNumericInput(target, key);
    this.session.endEdit();
  }

  /**
   * Validate a numeric draft and present session rejection without losing the input.
   * @private
   * @param input
   * @param key
   */
  private commitNumericInput(input: HTMLInputElement, key: string) {
    const value = input.valueAsNumber;
    if(input.value.trim() === '' || !Number.isFinite(value)) {
      input.setAttribute('aria-invalid', 'true');
      this.setStatus('Enter a valid number to apply this setting.', true);
      return false;
    }
    try {
      this.session.setValue(key, value);
      this.pendingDrafts.delete(input);
      input.setAttribute('aria-invalid', 'false');
      this.setStatus('', false);
      return true;
    } catch(error) {
      input.setAttribute('aria-invalid', 'true');
      this.setStatus(error instanceof Error ? error.message : String(error), true);
      return false;
    }
  }

  /**
   * Keep gameplay key bindings from observing keystrokes entered in the panel.
   * @private
   * @param event
   */
  private handleKeyDown(event: KeyboardEvent) {
    event.stopPropagation();
    if((event.ctrlKey || event.metaKey) && event.code === KEY_CODE.KEY_Z) {
      event.preventDefault();
      this.pendingDrafts.clear();
      if(event.shiftKey) {
        this.session.redo();
      } else {
        this.session.undo();
      }
      this.render();
    }
    if(event.code === KEY_CODE.ESCAPE) {
      event.preventDefault();
      this.setPanelOpen(false);
      this.view.launcher.focus();
    }
  }

  /**
   * Close an open panel on Escape even when focus is outside its root.
   * @private
   * @param event
   */
  private handleDocumentKeyDown(event: KeyboardEvent) {
    if(event.code !== KEY_CODE.ESCAPE || this.view.panel.hidden) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    this.setPanelOpen(false);
    this.view.launcher.focus();
  }

  /**
   * Release the shared editor screen space when a sibling panel opens.
   * @private
   * @param event
   */
  private handleEditorPanelOpen(event: Event) {
    if(isAnotherEditorOpening(event, this.view.root)) {
      this.setPanelOpen(false);
    }
  }

  /**
   * Keep visibility and the launcher accessibility state synchronized.
   * @private
   * @param open
   */
  private setPanelOpen(open: boolean) {
    if(!open) {
      this.session.endEdit();
    }
    this.view.panel.hidden = !open;
    this.view.launcher.setAttribute('aria-expanded', String(open));
  }

  /**
   * Match field text or whole groups, then hide groups with no visible rows.
   * @private
   * @param query
   */
  private filter(query: string) {
    const normalizedQuery = query.trim().toLowerCase();
    for(const group of this.view.fields.querySelectorAll('fieldset')) {
      if(!(group instanceof HTMLElement)) {
        continue;
      }
      const legend = group.querySelector('legend');
      const groupMatches = legend?.textContent?.toLowerCase().includes(normalizedQuery) ?? false;
      for(const row of group.querySelectorAll('[data-field-row]')) {
        if(!(row instanceof HTMLElement)) {
          continue;
        }
        const rowMatches = row.textContent?.toLowerCase().includes(normalizedQuery) ?? false;
        row.hidden = normalizedQuery.length > 0 && !groupMatches && !rowMatches;
      }
      group.hidden = !group.querySelector('[data-field-row]:not([hidden])');
    }
  }

  /**
   * Save a snapshot and distinguish success from edits made during the request.
   * @private
   */
  private async persist() {
    for(const input of this.pendingDrafts) {
      if(!this.commitNumericInput(input, input.dataset.configKey ?? '')) {
        input.focus();
        return;
      }
    }
    this.session.endEdit();
    this.setStatus('Saving…', false);
    try {
      await this.session.save();
      this.setStatus(this.session.dirty ? 'Saved snapshot; newer edits remain unsaved.' : 'Saved.', false);
    } catch(error) {
      this.setStatus(`Save failed: ${error instanceof Error ? error.message : String(error)}`, true);
    }
  }

  /**
   * Invoke the borrowed scenario adapter and surface any failure in the panel.
   * @private
   */
  private async restart() {
    if(!this.onRestart) {
      return;
    }
    try {
      await this.onRestart();
      this.setStatus('Scenario restarted.', false);
    } catch(error) {
      this.setStatus(`Restart failed: ${error instanceof Error ? error.message : String(error)}`, true);
    }
  }

  /**
   * Publish a plain-text operation result to the live status region.
   * @private
   * @param message
   * @param error
   */
  private setStatus(message: string, error: boolean) {
    this.view.status.textContent = message;
    this.view.status.dataset.error = String(error);
  }
}

/**
 * Mount an independent panel and style element; the caller owns their removal.
 * @param title
 * @param fields
 * @param hasRestart
 * @param restartLabel
 */
function createView(title: string, fields: ConfigField[], hasRestart: boolean, restartLabel: string) {
  const idPrefix = `joy-config-${nextPanelId++}`;
  const root = document.createElement('aside');
  root.className = 'joy-config-tools';
  root.setAttribute('aria-label', 'Config tuning');

  const launcher = document.createElement('button');
  launcher.type = 'button';
  launcher.className = 'joy-config-launcher';
  launcher.dataset.configAction = 'toggle';
  launcher.setAttribute('aria-label', 'Toggle config tuning');
  launcher.setAttribute('aria-expanded', 'false');
  launcher.textContent = 'TUNE';

  const panel = document.createElement('section');
  panel.className = 'joy-config-panel';
  panel.hidden = true;
  const heading = document.createElement('h2');
  heading.textContent = title;
  const search = document.createElement('input');
  search.type = 'search';
  search.className = 'joy-config-search';
  search.setAttribute('aria-label', 'Search settings');
  search.placeholder = 'Search settings';
  const fieldContainer = document.createElement('div');
  fieldContainer.className = 'joy-config-fields';
  appendFieldGroups(fieldContainer, fields, idPrefix);
  const actions = createActions(hasRestart, restartLabel);
  const status = document.createElement('p');
  status.className = 'joy-config-status';
  status.setAttribute('role', 'status');
  status.setAttribute('aria-live', 'polite');
  const state = document.createElement('p');
  state.className = 'joy-config-state';
  panel.append(heading, search, fieldContainer, state, actions.container, status);
  root.append(panel, launcher);

  const style = document.createElement('style');
  style.textContent = `${themeStyles}\n${styles}\n${launcherStyles}`;
  document.head.append(style);
  document.body.append(root);
  return { root, style, launcher, panel, search, fields: fieldContainer, state, status, ...actions };
}

/**
 * Append fields in authored order, reusing each group’s first fieldset.
 * @param container
 * @param fields
 * @param idPrefix
 */
function appendFieldGroups(container: HTMLElement, fields: ConfigField[], idPrefix: string) {
  const groups = new Map();
  for(const [fieldIndex, field] of fields.entries()) {
    let group = groups.get(field.group);
    if(!group) {
      group = document.createElement('fieldset');
      const legend = document.createElement('legend');
      legend.textContent = field.group;
      group.append(legend);
      groups.set(field.group, group);
      container.append(group);
    }
    group.append(createFieldRow(field, `${idPrefix}-${fieldIndex}`));
  }
}

/**
 * Create paired slider and numeric controls with shared descriptions and reset action.
 * @param field
 * @param idPrefix
 */
function createFieldRow(field: ConfigField, idPrefix: string) {
  const row = document.createElement('div');
  row.className = 'joy-config-field';
  row.dataset.fieldRow = field.key;
  const label = document.createElement('label');
  label.id = `${idPrefix}-label`;
  label.htmlFor = `${idPrefix}-number`;
  label.textContent = field.label;
  label.title = field.description;
  const mode = document.createElement('span');
  mode.id = `${idPrefix}-mode`;
  mode.className = 'joy-config-mode';
  mode.textContent = formatApplyMode(field.applyMode);
  const marker = document.createElement('span');
  marker.className = 'joy-config-marker';
  marker.dataset.configMarker = '';
  const range = createNumericInput(field, 'range');
  range.id = `${idPrefix}-range`;
  const number = createNumericInput(field, 'number');
  number.id = `${idPrefix}-number`;
  const description = document.createElement('span');
  description.id = `${idPrefix}-description`;
  description.className = 'joy-config-description';
  description.textContent = field.description;
  const describedBy = `${description.id} ${mode.id}`;
  range.setAttribute('aria-labelledby', label.id);
  range.setAttribute('aria-describedby', describedBy);
  number.setAttribute('aria-labelledby', label.id);
  number.setAttribute('aria-describedby', describedBy);
  const units = document.createElement('span');
  units.className = 'joy-config-units';
  units.textContent = field.units;
  const reset = document.createElement('button');
  reset.type = 'button';
  reset.dataset.configAction = 'reset-field';
  reset.textContent = 'Reset';
  reset.setAttribute('aria-label', `Reset ${field.label}`);
  row.append(label, mode, marker, description, range, number, units, reset);
  return row;
}

/**
 * Apply authored bounds and increments to one numeric control.
 * @param field
 * @param type
 */
function createNumericInput(field: ConfigField, type: 'range' | 'number') {
  const input = document.createElement('input');
  input.type = type;
  input.min = String(field.min);
  input.max = String(field.max);
  input.step = String(field.step);
  input.dataset.configKey = field.key;
  return input;
}

/**
 * Build delegated action buttons, including restart only when supported.
 * @param hasRestart
 * @param restartLabel
 */
function createActions(hasRestart: boolean, restartLabel: string) {
  const container = document.createElement('div');
  container.className = 'joy-config-actions';
  /**
   * @param action
   * @param label
   */
  const makeButton = (action: string, label: string) => {
    const button = document.createElement('button');
    button.type = 'button';
    button.dataset.configAction = action;
    button.textContent = label;
    container.append(button);
    return button;
  };
  const undo = makeButton('undo', 'Undo');
  const redo = makeButton('redo', 'Redo');
  const revert = makeButton('revert', 'Revert');
  makeButton('reset-all', 'Reset All');
  if(hasRestart) {
    makeButton('restart', restartLabel);
  }
  const save = makeButton('save', 'Save');
  return { container, undo, redo, revert, save };
}

/**
 * Translate the configuration timing contract into a short editor label.
 * @param applyMode
 */
function formatApplyMode(applyMode: ConfigApplyMode) {
  if(applyMode === 'next-spawn') {
    return 'Next Spawn';
  }
  if(applyMode === 'restart') {
    return 'On Restart';
  }
  return 'Live';
}
