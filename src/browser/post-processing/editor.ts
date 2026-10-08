// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { BROWSER_EVENT } from '../constants.ts';
import { KEY_CODE } from '../input/constants.ts';
import { POST_PROCESSING_KEY, TONE_MAPPER } from '../../rendering/postprocessor/constants.ts';
import type { PostProcessingProfile as Profile } from '../../rendering/postprocessor/profile.ts';
import type { PostProcessingConfig } from '../../rendering/postprocessor/config.ts';
import type { GpuTriangleRenderer } from '../../rendering/renderers/gpu-triangle-renderer.ts';
import { PostProcessingSession } from '../../rendering/postprocessor/session.ts';
import { resolvePostProcessingProfile } from '../../rendering/postprocessor/profile.ts';
import { normalizePostProcessingConfig } from '../../rendering/postprocessor/config.ts';
import { createPostProcessingView } from './view.ts';
import { announceEditorPanelOpen, EDITOR_PANEL_OPEN_EVENT, isAnotherEditorOpening } from '../editor-launchers.ts';

export type { Profile };
export interface ProfileMessage {
  profile?: unknown;
  revision?: string;
  error?: string;
}
export interface PostProcessingEditorOptions {
  title: string;
  profile: unknown;
  renderer: Pick<GpuTriangleRenderer, 'setPostProcessingConfig'>;
  hot: ImportMeta['hot'];
  baseUrl: string;
  pauseShortcut?: boolean;
  onFocus?: () => void;
}

/**
 * Own a development-only editing session and its DOM/network subscriptions.
 * Games explicitly skip simulation when paused and may freeze presentation with renderTime().
 * Destroy this owner before its borrowed renderer. No gameplay objects are retained.
 */
export class PostProcessingEditor {
  declare renderer: Pick<GpuTriangleRenderer, "setPostProcessingConfig">;
  declare hot: ImportMeta['hot'];
  declare endpoint: string;
  declare title: string;
  declare pauseShortcut: boolean;
  declare onFocus: (() => void) | undefined;
  declare session: PostProcessingSession;
  declare paused: boolean;
  declare compareSaved: boolean;
  declare bypassed: boolean;
  declare saving: boolean;
  declare connected: boolean;
  declare disposed: boolean;
  declare error: string;
  declare clock: number;
  declare previousTimestamp: number | null;
  declare events: AbortController;
  declare view: ReturnType<typeof createPostProcessingView>;
  declare onFile: (message: ProfileMessage) => void;
  declare onDocumentKeyDown: (event: KeyboardEvent) => void;
  declare onEditorPanelOpen: (event: Event) => void;
  declare onReconnect: () => void;
  declare onDisconnect: () => void;

  /**
   * @param options
   */
  constructor({ title, profile, renderer, hot, baseUrl, pauseShortcut = true, onFocus }: PostProcessingEditorOptions) {
    this.renderer = renderer;
    this.hot = hot;
    this.endpoint = `${baseUrl}__joy/post-processing`;
    this.title = title;
    this.pauseShortcut = pauseShortcut;
    this.onFocus = onFocus;
    this.session = new PostProcessingSession(profile, '');
    this.paused = false;
    this.compareSaved = false;
    this.bypassed = false;
    this.saving = false;
    this.connected = false;
    this.disposed = false;
    this.error = '';
    this.clock = 0;

    this.previousTimestamp = null;
    this.events = new AbortController();
    this.view = createPostProcessingView(title);
    this.onFile = this.receiveFile.bind(this);
    this.onDocumentKeyDown = this.handleDocumentKeyDown.bind(this);
    this.onEditorPanelOpen = this.handleEditorPanelOpen.bind(this);
    this.onReconnect = () => {
      void this.load();
    };
    this.onDisconnect = () => {
      this.connected = false;
      this.error = 'Development server disconnected. Export preserves your edits.';
      this.refresh();
    };
    hot?.on('joy:post-processing', this.onFile);
    hot?.on('vite:ws:connect', this.onReconnect);
    hot?.on('vite:ws:disconnect', this.onDisconnect);
    this.bindView();
    document.addEventListener(BROWSER_EVENT.KEYDOWN, this.onDocumentKeyDown);
    document.addEventListener(EDITOR_PANEL_OPEN_EVENT, this.onEditorPanelOpen);
    this.refresh();
    void this.load();
  }

  /**
   * Return a presentation clock in milliseconds, frozen while simulation is paused.
   * @param timestampMilliseconds Monotonic animation-frame time.
   */
  renderTime(timestampMilliseconds: number) {
    if(this.previousTimestamp === null) {
      this.clock = timestampMilliseconds;
    } else if(!this.paused) {
      this.clock += timestampMilliseconds - this.previousTimestamp;
    }
    this.previousTimestamp = timestampMilliseconds;
    return this.clock;
  }

  /** Toggle the simulation pause owned by this development editor. */
  togglePaused() {
    this.paused = !this.paused;
    this.view.pause.checked = this.paused;
    this.apply();
    this.refresh();
  }

  /** Release listeners, requests, DOM and preview overrides; repeated calls are safe. */
  destroy() {
    if(this.disposed) {
      return;
    }
    this.disposed = true;
    this.events.abort();
    document.removeEventListener(BROWSER_EVENT.KEYDOWN, this.onDocumentKeyDown);
    document.removeEventListener(EDITOR_PANEL_OPEN_EVENT, this.onEditorPanelOpen);
    this.hot?.off('joy:post-processing', this.onFile);
    this.hot?.off('vite:ws:connect', this.onReconnect);
    this.hot?.off('vite:ws:disconnect', this.onDisconnect);
    this.renderer.setPostProcessingConfig(resolvePostProcessingProfile(this.session.saved));
    this.view.root.remove();
    this.view.style.remove();
  }

  /**
   * Attach editor interactions to the abortable lifetime shared by requests and listeners.
   * @private
   */
  private bindView() {
    const { signal } = this.events;
    this.view.root.addEventListener(BROWSER_EVENT.FOCUSIN, () => this.onFocus?.(), { signal });
    this.view.root.addEventListener(BROWSER_EVENT.POINTERDOWN, () => this.onFocus?.(), { signal });
    this.view.toggle.addEventListener(BROWSER_EVENT.CLICK, () => this.togglePanel(), { signal });
    // Keyboard editing must never reach game controls registered on window.
    // Keyup still propagates so a movement key released here cannot remain held.
    this.view.root.addEventListener(BROWSER_EVENT.KEYDOWN, event => {
      event.stopPropagation();
      if(event.code === KEY_CODE.ESCAPE) {
        this.togglePanel(false);
        this.view.toggle.focus();
      }
    }, { signal });
    for(const [key, field] of this.view.fields) {
      const edit = (input: HTMLInputElement | HTMLSelectElement) => {
        this.change(() => this.session.setValue(key, input instanceof HTMLSelectElement ? input.value : Number(input.value)));
      };
      field.input.addEventListener(BROWSER_EVENT.CHANGE, () => edit(field.input), { signal });
      field.slider?.addEventListener(BROWSER_EVENT.INPUT, () => {
        if(field.slider) {
          edit(field.slider);
        }
      }, { signal });
      field.reset.addEventListener(BROWSER_EVENT.CLICK, () => this.change(() => this.session.resetValue(key)), { signal });
    }
    this.view.preset.addEventListener(BROWSER_EVENT.CHANGE, () => this.change(() => this.session.selectPreset(this.view.preset.value)), { signal });
    for(const element of this.view.root.querySelectorAll('input[data-mode]')) {
      const checkbox = ((element) as HTMLInputElement);
      checkbox.addEventListener(BROWSER_EVENT.CHANGE, () => {
        if(checkbox.dataset.mode === 'pause') {
          this.paused = checkbox.checked;
        } else if(checkbox.dataset.mode === 'saved') {
          this.compareSaved = checkbox.checked;
        } else {
          this.bypassed = checkbox.checked;
        }
        this.apply();
        this.refresh();
      }, { signal });
    }
    for(const [action, button] of this.view.actions) {
      button.addEventListener(BROWSER_EVENT.CLICK, () => this.perform(action), { signal });
    }
    this.view.file.addEventListener(BROWSER_EVENT.CHANGE, () => {
      void this.importProfile();
    }, { signal });
  }

  /**
   * Synchronize visibility and announce opening to sibling development panels.
   * @private
   * @param [open]
   */
  private togglePanel(open: boolean = Boolean(this.view.panel.hidden)) {
    this.view.panel.hidden = !open;
    this.view.toggle.setAttribute('aria-expanded', String(open));
    if(open) {
      announceEditorPanelOpen(this.view.root);
    }
  }

  /**
   * Close the editor with Escape and toggle simulation pause with P.
   * @private
   * @param event
   */
  private handleDocumentKeyDown(event: KeyboardEvent) {
    const target = event.target;
    const isEditing = target instanceof HTMLElement && Boolean(target.closest('input, textarea, select, [contenteditable]'));
    if(event.code === KEY_CODE.ESCAPE && !this.view.panel.hidden) {
      event.preventDefault();
      event.stopPropagation();
      this.togglePanel(false);
      this.view.toggle.focus();
      return;
    }
    if(!this.pauseShortcut || event.code !== KEY_CODE.KEY_P || event.repeat || event.ctrlKey || event.metaKey || event.altKey || isEditing) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    this.togglePaused();
  }

  /**
   * Close this panel when another development editor claims the same screen space.
   * @private
   * @param event
   */
  private handleEditorPanelOpen(event: Event) {
    if(isAnotherEditorOpening(event, this.view.root)) {
      this.togglePanel(false);
    }
  }

  /**
   * Route toolbar actions through session transitions or profile persistence.
   * @private
   * @param action
   */
  private perform(action: string) {
    switch(action) {
      case 'close':
        this.togglePanel(false);
        break;
      case 'save':
        void this.save();
        break;
      case 'undo':
        this.change(() => this.session.undo());
        break;
      case 'redo':
        this.change(() => this.session.redo());
        break;
      case 'revert':
        this.change(() => this.session.revert());
        break;
      case 'disk':
        this.change(() => this.session.useDisk());
        break;
      case 'local':
        this.change(() => this.session.keepLocal());
        break;
      case 'export':
        this.exportProfile();
        break;
      case 'import':
        this.view.file.click();
        break;
    }
  }

  /**
   * Apply a session operation and retain a visible error if validation fails.
   * @private
   * @param operation
   */
  private change(operation: () => void) {
    try {
      operation();
      this.error = '';
      this.apply();
    } catch(error) {
      this.error = error instanceof Error ? error.message : String(error);
    }
    this.refresh();
  }

  /**
   * Resolve the active preview mode before updating the borrowed renderer.
   * @private
   */
  private apply() {
    let config = this.compareSaved ? resolvePostProcessingProfile(this.session.saved) : this.session.config;
    if(this.bypassed) {
      // Still resolve linear scene color to the display; never change target formats.
      config = normalizePostProcessingConfig({ toneMapper: TONE_MAPPER.NONE, antialiasStrength: 0 });
    }
    this.renderer.setPostProcessingConfig(config);
  }

  /**
   * Reflect draft values, inheritance, conflicts, and persistence availability in the view.
   * @private
   */
  private refresh() {
    const { session, view } = this;
    const config = session.config;
    const draft = session.draft;
    for(const [key, field] of view.fields) {
      field.input.value = String(config[key]);
      if(field.slider) {
        field.slider.value = String(config[key]);
      }
      const overridden = Object.hasOwn(draft.overrides, key);
      field.source.textContent = overridden ? 'Game Override' : `Inherited · ${draft.preset}`;
      field.reset.disabled = !overridden;
      field.input.closest('[data-setting-row]')?.toggleAttribute('hidden', key === POST_PROCESSING_KEY.AGX_LOOK && config.toneMapper !== TONE_MAPPER.AGX);
    }
    view.preset.value = draft.preset;
    view.status.dataset.error = String(Boolean(this.error));
    const state = this.saving ? 'Saving…' : session.dirty ? 'Unsaved Changes' : 'Saved';
    const preview = this.bypassed ? ' · Effects Bypassed' : this.compareSaved ? ' · Showing Saved' : '';
    view.status.textContent = this.error || `${state}${preview}`;
    view.toggle.textContent = session.dirty ? 'JOY-ENGINE •' : 'JOY-ENGINE';
    view.conflict.hidden = !session.conflict;
    for(const [action, button] of view.actions) {
      if(action === 'save') {
        button.disabled = this.saving || !this.connected || !session.dirty || Boolean(session.conflict);
      } else if(action === 'undo') {
        button.disabled = !session.canUndo;
      } else if(action === 'redo') {
        button.disabled = !session.canRedo;
      } else if(action === 'revert') {
        button.disabled = !session.dirty;
      }
    }
  }

  /**
   * Reconcile an external profile revision without silently replacing local edits.
   * @private
   * @param message
   */
  private receiveFile(message: ProfileMessage) {
    if(this.disposed) {
      return;
    }
    if(message.error || !message.revision) {
      this.error = message.error ?? 'Invalid profile response.';
      this.refresh();
      return;
    }
    this.change(() => this.session.receiveExternal(message.profile,  ((message.revision) as string)));
  }

  /**
   * Load the latest server profile and preserve local state when the connection fails.
   * @private
   */
  private async load() {
    try {
      const response = await fetch(this.endpoint, { signal: this.events.signal });
      const message = await response.json();
      if(this.disposed) {
        return;
      }
      this.connected = response.ok;
      this.receiveFile(message);
    } catch(error) {
      if(!this.disposed) {
        this.connected = false;
        this.error = `Cannot load profile: ${error instanceof Error ? error.message : error}`;
        this.refresh();
      }
    }
  }

  /**
   * Persist one submitted snapshot against its revision; concurrent disk edits become conflicts.
   * @private
   */
  private async save() {
    if(this.saving || !this.connected || this.session.conflict) {
      return;
    }
    const submitted = this.session.beginSave();
    this.saving = true;
    this.error = '';
    this.refresh();
    try {
      const response = await fetch(this.endpoint, {
        method: 'POST', signal: this.events.signal,
        headers: { 'Content-Type': 'application/json', 'X-Joy-Postprocessing': '1' },
        body: JSON.stringify({ profile: submitted, revision: this.session.revision }),
      });
      const message = await response.json();
      if(this.disposed) {
        return;
      }
      if(response.status === 409) {
        this.session.receiveExternal(message.profile, message.revision);
        throw new Error(message.error);
      }
      if(!response.ok || typeof message.revision !== 'string') {
        throw new Error(message.error ?? 'Save failed.');
      }
      this.session.markSaved(submitted, message.revision);
      this.apply();
    } catch(error) {
      this.error = error instanceof Error ? error.message : String(error);
    } finally {
      this.session.endSave();
      this.saving = false;
      if(!this.disposed) {
        this.refresh();
      }
    }
  }

  /**
   * Download a flattened profile that is independent of later preset changes.
   * @private
   */
  private exportProfile() {
    // Flatten inheritance for a portable look that survives later preset edits.
    const profile = { version: 1, preset: 'default', overrides: this.session.config };
    const blob = new Blob([`${JSON.stringify(profile, null, 2)}\n`], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = url;
    link.download = `${this.title.toLowerCase().replace(/[^a-z0-9]+/g, '-')}.post-processing.json`;
    link.click();
    setTimeout(() => URL.revokeObjectURL(url), 0);
  }

  /**
   * Preview a bounded JSON file; persistence remains a separate explicit save.
   * @private
   */
  private async importProfile() {
    const file = this.view.file.files?.[0];
    this.view.file.value = '';
    if(!file) {
      return;
    }
    try {
      if(file.size > 16 * 1024) {
        throw new Error('Profile must be at most 16 KiB.');
      }
      const text = await file.text();
      if(!this.disposed) {
        this.change(() => this.session.replace(JSON.parse(text)));
      }
    } catch(error) {
      if(!this.disposed) {
        this.error = error instanceof Error ? error.message : String(error);
        this.refresh();
      }
    }
  }
}
