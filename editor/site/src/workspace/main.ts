// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { BROWSER_EVENT, KEY_CODE, PREVIEW_COMMAND } from 'joy-engine/constants';
import { WORKSPACE_CHANNEL, WORKSPACE_MESSAGE, WORKSPACE_OPERATION, WORKSPACE_STORAGE_KEY } from './constants.ts';
import type { DiskDocument } from './documents.ts';
import type { PlayMode, PreviewState } from './game-preview-session.ts';
import {PLAY_MODE} from './preview-protocol.ts';
import {GamePreviewSession} from './game-preview-session.ts';
import '../../css/workspace/style.css';
import '@fontsource/archivo/400.css';
import '@fontsource/archivo/600.css';
import { WorkspaceDocument } from './documents.ts';
import { createAssetText, editorUrl, isSupportedAssetPath } from './asset-types.ts';

export interface Project {
  id: string;
  name: string;
  preview: string | null;
  connected: boolean;
  external?: boolean;
  path?: string;
  capabilities?: Array<{
      extensions: string[];
  }>;
}
export interface Asset {
  path: string;
  kind: string;
}
/**
 * Owns project selection, document frames, local file polling and the game connection. */
class JoyWorkspace {
  declare projects: Project[];
  declare files: Asset[];
  declare documents: Map<string, WorkspaceDocument>;
  declare frames: Map<string, HTMLIFrameElement>;
  declare ready: Set<string>;
  declare repairs: Map<string, HTMLElement>;
  declare captures: Map<string, (ok: boolean) => void>;
  declare assetsWidth: number;
  declare previewTabActive: boolean;
  declare previewPlaceholder: HTMLElement;
  declare project: string;
  declare active: string;
  declare filters: Set<string>;
  declare selectedAsset: string;
  declare menuPath: string;
  declare maximizedPath: string;
  declare menuAnchor: HTMLElement | null;
  declare returnPath: string;
  declare polling: boolean;
  declare generation: number;
  declare disposed: boolean;
  declare listeners: AbortController;
  declare session: { project: string; tabs: Record<string, string[]>; active: Record<string, string>; width: number; };
  declare preview: GamePreviewSession;
  declare pollTimer: number;
  declare refreshTimer: number;

  constructor() {

    this.projects = [];

    this.files = [];

    this.documents = new Map();

    this.frames = new Map();

    this.ready = new Set();

    this.repairs = new Map();

    this.captures = new Map();
    this.assetsWidth = 220;
    this.previewTabActive = false;
    // Retained while the game frame occupies the stage, restored when it stops.
    this.previewPlaceholder = element('preview-placeholder');
    this.project = '';
    this.active = '';
    this.filters = new Set();
    this.selectedAsset = '';
    this.menuPath = '';
    this.maximizedPath = '';

    this.menuAnchor = null;
    this.returnPath = '';
    this.polling = false;
    this.generation = 0;
    this.disposed = false;
    this.listeners = new AbortController();
    this.session = this.restore();
    this.applyPreviewWidth();
    this.preview = new GamePreviewSession(element('game-stage'), state => this.previewChanged(state));
    this.restorePlay();
    this.restoreAssets();
    this.bind();
    this.pollTimer = window.setInterval(() => void this.poll(), 1500);
    this.refreshTimer = window.setInterval(() => void this.refreshFiles(), 15000);
  }

  /**
   * Discover checkout projects, then restore the requested project and document tabs. */
  async start() {
    try {
      this.projects = await api('projects');
      for(const project of this.projects) {
        const option = document.createElement('option');
        option.value = project.id;
        option.textContent = project.name;
        select('project').append(option);
      }
      const requested = new URLSearchParams(location.search).get('project') ?? (this.session.project || 'god-game');
      await this.selectProject(this.projects.find(item => item.id === requested)?.id ?? this.projects[0]?.id ?? '');
    } catch(error) {
      this.error(error);
    }
  }

  /**
   * Register workspace input under one abortable lifetime. @private */
  private bind() {
    const signal = this.listeners.signal;
    /** @param id @param action */
    const click = (id: string, action: () => void) => element(id).addEventListener(BROWSER_EVENT.CLICK, action, {signal});
    select('project').addEventListener(BROWSER_EVENT.CHANGE, () => void this.selectProject(select('project').value), {signal});
    input('search').addEventListener(BROWSER_EVENT.INPUT, () => this.renderFiles(), {signal});
    click('return-owner', () => {
      if(this.documents.has(this.returnPath)) {
        this.activate(this.returnPath);
      }
      this.returnPath = '';
      element('return-owner').hidden = true;
      element('return-owner').parentElement?.setAttribute('hidden', '');
    });
    click('filter-toggle', () => this.setFiltersOpen(Boolean(element('asset-filters').hidden)));
    click('filter-all', () => {
      this.filters.clear();
      this.renderFiles();
    });
    click('rename-asset', () => {
      const path = this.menuPath;
      this.closeAssetMenu();
      void this.renameAsset(path);
    });
    element('asset-filters').addEventListener(BROWSER_EVENT.CHANGE, event => {
      const target = event.target;
      if(target instanceof HTMLInputElement) {
        if(target.checked) {
          this.filters.add(target.value);
        } else {
          this.filters.delete(target.value);
        }
        this.renderFiles();
      }
    }, {signal});
    window.addEventListener(BROWSER_EVENT.POINTERDOWN, event => {
      const target = event.target;
      if(target instanceof Node) {
        if(!element('asset-menu').contains(target) && !this.menuAnchor?.contains(target)) {
          this.closeAssetMenu(false);
        }
        if(!element('asset-filters').contains(target) && !button('filter-toggle').contains(target)) {
          this.setFiltersOpen(false, false);
        }
      }
    }, {signal});
    element('asset-menu').addEventListener(BROWSER_EVENT.KEYDOWN, event => {
      if(event.key === KEY_CODE.ESCAPE || event.key === KEY_CODE.TAB) {
        this.closeAssetMenu();
      }
    }, {signal});
    click('assets-toggle', () => this.setAssets(!element('workspace').classList.contains('assets-collapsed')));
    click('new-asset', () => this.showCreateAsset());
    click('cancel-new-asset', () => this.hideCreateAsset());
    select('new-asset-type').addEventListener(BROWSER_EVENT.CHANGE, () => this.updateCreateHint(), {signal});
    const help = ((element('workspace-help')) as HTMLDialogElement);
    click('workspace-help-open', () => help.showModal());
    click('workspace-help-close', () => help.close());
    help.addEventListener('close', () => button('workspace-help-open').focus(), {signal});
    click('import-asset', () => input('import-file').click());
    input('import-file').addEventListener(BROWSER_EVENT.CHANGE, () => void this.importAsset(), {signal});
    element('new-asset-form').addEventListener('submit', event => {
      event.preventDefault();
      void this.createAsset();
    }, {signal});
    click('refresh', () => void this.refreshFiles());
    click('save', () => void this.save(this.active));
    click('open-first', () => {
      if(this.files[0]) {
        void this.open(this.files[0].path);
      }
    });
    click('run', () => this.run());
    click('restart', () => {
      if(confirm('Restart the game? This resets the current simulation.')) {
        this.run(true);
      }
    });
    click('pause', () => this.sendGame({type: PREVIEW_COMMAND.PAUSE}));
    click('stop', () => this.stop());
    click('finish-external', () => {
      if(confirm('Close the independent browser tab first. Is it closed?')) {
        this.preview.finishIndependent();
      }
    });
    select('play-mode').addEventListener(BROWSER_EVENT.CHANGE, () => this.rememberPlay(), {signal});
    select('play-size').addEventListener(BROWSER_EVENT.CHANGE, () => this.rememberPlay(), {signal});
    click('regenerate-global', () => this.sendGame({type: PREVIEW_COMMAND.REGENERATE}));
    click('regenerate', () => this.sendGame({type: PREVIEW_COMMAND.REGENERATE}));
    click('toggle-preview', () => this.collapsePreview(true));
    click('preview-mode', () => this.focusGame());
    click('show-preview', () => this.collapsePreview(false));
    click('reload-disk', () => {
      const doc = this.documents.get(this.active);
      if(doc && confirm('Discard your local edits and use the version on disk?')) {
        doc.reloadDisk();
        this.loadFrame(doc);
        this.renderDocuments();
      }
    });
    click('keep-local', () => {
      this.documents.get(this.active)?.keepLocal();
      this.renderDocuments();
      this.status('Kept your edits. Save File will replace the reviewed disk version.');
    });
    window.addEventListener(BROWSER_EVENT.MESSAGE, event => this.message(event), {signal});
    window.addEventListener(BROWSER_EVENT.KEYDOWN, event => {
      if(help.open) {
        return;
      }
      if(event.key === KEY_CODE.ESCAPE) {
        if(this.maximizedPath && !event.defaultPrevented) {
          this.setWorkspaceMaximized('', false);
        }
        if(!element('asset-filters').hidden) {
          this.setFiltersOpen(false);
        }
        this.closeAssetMenu();
      }
      if(event.metaKey || event.ctrlKey) {
        if(event.key.toLowerCase() === 's') {
          event.preventDefault();
          void this.save(this.active);
        } else if(event.key.toLowerCase() === 'p') {
          event.preventDefault();
          this.focusSearch();
        }
      }
    }, {signal});
    window.addEventListener(BROWSER_EVENT.BEFOREUNLOAD, event => {
      if([...this.documents.values()].some(doc => doc.dirty || doc.saving)) {
        event.preventDefault();
      }
    }, {signal});
    const assetsDivider = element('assets-divider');

    let assetsPointer: number | null = null;
    assetsDivider.addEventListener(BROWSER_EVENT.POINTERDOWN, event => {
      if(event.button === 0) {
        assetsPointer = event.pointerId;
        assetsDivider.setPointerCapture(event.pointerId); document.body.classList.add('resizing');
      }
    }, {signal});
    assetsDivider.addEventListener(BROWSER_EVENT.POINTERMOVE, event => {
      if(assetsDivider.hasPointerCapture(event.pointerId)) {
        this.assetsWidth = Math.max(160, Math.min(window.innerWidth*0.3, event.clientX));
        this.applyAssets();
      }
    }, {signal});
    const finishAssets = () => {
      const id = assetsPointer;
      assetsPointer = null;
      if(id !== null && assetsDivider.hasPointerCapture(id)) {
        assetsDivider.releasePointerCapture(id);
      }
      document.body.classList.remove('resizing');
      this.saveAssets();
    };
    for(const type of [BROWSER_EVENT.POINTERUP, BROWSER_EVENT.POINTERCANCEL, BROWSER_EVENT.LOSTPOINTERCAPTURE]) { assetsDivider.addEventListener(type, finishAssets, {signal}); }
    window.addEventListener(BROWSER_EVENT.BLUR, finishAssets, {signal});
    assetsDivider.addEventListener(BROWSER_EVENT.KEYDOWN, event => {
      if(event.key === KEY_CODE.ARROW_LEFT || event.key === KEY_CODE.ARROW_RIGHT) {
        event.preventDefault();
        this.assetsWidth = Math.max(160, Math.min(window.innerWidth*0.3, this.assetsWidth + (event.key === KEY_CODE.ARROW_RIGHT ? 20 : -20)));
        this.applyAssets(); this.saveAssets();
      }
    }, {signal});
    window.addEventListener(BROWSER_EVENT.RESIZE, () => {
      this.applyAssets();
      this.applyPreviewWidth();
    }, {signal});
    const divider = element('preview-divider');

    let previewPointer: number | null = null;
    divider.addEventListener(BROWSER_EVENT.POINTERDOWN, event => {
      if(event.button !== 0) {
        return;
      }
      previewPointer = event.pointerId;
      divider.setPointerCapture(event.pointerId);
      document.body.classList.add('resizing');
    }, {signal});
    divider.addEventListener(BROWSER_EVENT.POINTERMOVE, event => {
      if(divider.hasPointerCapture(event.pointerId)) {
        this.resizePreview(window.innerWidth - event.clientX);
      }
    }, {signal});
    const finishPreview = () => {
      const id = previewPointer;
      previewPointer = null;
      if(id !== null && divider.hasPointerCapture(id)) {
        divider.releasePointerCapture(id);
      }
      document.body.classList.remove('resizing');
    };
    for(const type of [BROWSER_EVENT.POINTERUP, BROWSER_EVENT.POINTERCANCEL, BROWSER_EVENT.LOSTPOINTERCAPTURE]) {
      divider.addEventListener(type, finishPreview, {signal});
    }
    window.addEventListener(BROWSER_EVENT.BLUR, finishPreview, {signal});
    divider.addEventListener(BROWSER_EVENT.KEYDOWN, event => {
      if(event.key === KEY_CODE.ARROW_LEFT || event.key === KEY_CODE.ARROW_RIGHT) {
        event.preventDefault();
        this.resizePreview(element('preview-panel').clientWidth + (event.key === KEY_CODE.ARROW_LEFT ? 30 : -30));
      }
    }, {signal});
    const desktop = window.joyDesktop;
    if(desktop) {
      element('open-folder').hidden = false;
      click('open-folder', () => {
        void desktop.chooseProject().then(project => {
          if(project) {
            const existing = this.projects.find(item => item.id === project.id);
            if(!existing) {
              this.projects.push(project);
              const option = document.createElement('option');
              option.value = project.id;
              option.textContent = project.name;
              select('project').append(option);
            }
            return this.selectProject(project.id);
          }
        }).catch(error => this.error(error));
      });
    }
  }

  /**
   * Capture pending edits before replacing the active project and its owned frames.
   * @param project
   */
  async selectProject(project: string) {
    const captured = await Promise.all([...this.documents.keys()].map(path => this.capture(path)));
    if(captured.some(ok => !ok)) {
      select('project').value = this.project;
      return;
    }
    if([...this.documents.values()].some(doc => doc.dirty || doc.saving) && !confirm('Switch projects and discard unsaved edits?')) {
      select('project').value = this.project;
      return;
    }
    if(!this.stop()) {
      this.status('Close the independent browser game and finish external preview before changing projects.');
      select('project').value = this.project;
      return;
    }
    this.setWorkspaceMaximized('', false);
    this.closeAssetMenu(false);
    this.selectedAsset = '';
    const generation = ++this.generation;
    for(const frame of this.frames.values()) {
      frame.remove();
    }
    for(const repair of this.repairs.values()) {
      repair.remove();
    }
    this.repairs.clear();
    this.documents.clear();
    this.frames.clear();
    this.ready.clear();
    this.active = '';
    this.project = project;
    select('project').value = project;
    const metadata = this.projects.find(item => item.id === project);
    element('project-label').textContent = metadata?.name ?? project;
    element('project-path').textContent = metadata?.path ?? `projects/${project}`;
    anchor('external-preview').href = metadata?.preview ?? '#';
    anchor('external-preview').hidden = !metadata?.preview;
    this.renderDocuments();
    await this.refreshFiles();
    if(generation !== this.generation) {
      return;
    }
    this.status('Project opened. Saves write directly to this checkout.');
    const restored = [...(this.session.tabs?.[project] ?? [])];
    const active = this.session.active?.[project];
    for(const path of restored) {
      if(generation !== this.generation) {
        return;
      }
      if(this.files.some(file => file.path === path)) {
        await this.open(path);
      }
    }
    if(generation !== this.generation) {
      return;
    }
    if(active && this.documents.has(active)) {
      this.activate(active);
    }
    this.remember();
  }

  /**
   * Show the inline asset type and name fields without interrupting the workflow.
   */
  showCreateAsset() {
    element('new-asset-form').hidden = false;
    this.updateCreateHint();
    input('new-asset-name').focus();
  }

  /** Explain authored persistence without treating generated geometry as source. */
  updateCreateHint() {

    const hints: Record<string, string> = {
      formlab: 'Editable procedural document. Saves script, parameter overrides, materials and variants.',
      joylevel: 'Scene document. Stores entities and their authored settings.',
      joyobject: 'Reusable object definition. Stores entity components and defaults.',
      joyfx: 'Editable effect. Stores emitters, scripts and effect parameters.'
    };
    element('new-asset-hint').textContent = hints[select('new-asset-type').value] ?? '';
  }

  /**
   * Hide and reset the inline asset creation fields.
   */
  hideCreateAsset() {
    element('new-asset-form').hidden = true;
    input('new-asset-name').value = '';
  }

  /**
   * Create the selected supported asset from a small valid starter without overwriting a file. */
  async createAsset() {
    const name = input('new-asset-name').value.trim();
    const extension = select('new-asset-type').value;
    if(!name) {
      this.status('Name the new asset before creating it.');
      return;
    }
    const filename = name.toLowerCase().endsWith(`.${extension}`) ? name : `${name}.${extension}`;
    if(!/^[a-zA-Z0-9_/-]+\.(joylevel|joyobject|form|formlab|joyfx)$/.test(filename) || filename.includes('..')) {
      this.status('Use letters, numbers, dashes, underscores and optional folders for the asset name.');
      return;
    }
    const project = this.project;
    const path = `assets/${filename}`;
    try {
      await api('file', {project, path}, {text: createAssetText(path), revision: ''}, 'POST');
      if(project !== this.project) {
        return;
      }
      await this.refreshFiles();
      await this.open(path);
      this.hideCreateAsset();
      this.status(`Created ${path}`);
    } catch(error) {
      this.error(error);
    }
  }

  /**
   * Import one supported text asset without replacing an existing project file.
   */
  async importAsset() {
    const project = this.project;
    const picker = input('import-file');
    const file = picker.files?.[0];
    picker.value = '';
    if(!file) {
      return;
    }
    const path = `assets/${file.name}`;
    if(!isSupportedAssetPath(path)) {
      this.status('Choose a supported level, object, FORM or particle asset.');
      return;
    }
    try {
      await api('file', {project, path}, {text: await file.text(), revision: ''}, 'POST');
      if(project !== this.project) {
        return;
      }
      await this.refreshFiles();
      await this.open(path);
      this.status(`Imported ${path}`);
    } catch(error) {
      this.error(error);
    }
  }

  /**
   * Rename an asset without opening a closed document or replacing another file.
   * @param path
   */
  async renameAsset(path: string) {
    const project = this.project;
    if(!await this.capture(path)) {
      return;
    }
    if(project !== this.project) {
      return;
    }
    const doc = this.documents.get(path);
    if(doc?.dirty || doc?.saving) {
      this.status('Save or discard edits before renaming this asset.');
      return;
    }
    const destination = prompt('Rename asset to:', path.slice('assets/'.length));
    if(!destination || destination === path.slice('assets/'.length)) {
      return;
    }
    try {
      await api('file', {project, path}, {path: `assets/${destination}`}, 'PATCH');
      if(project !== this.project) {
        return;
      }
      if(doc) {
        await this.close(path);
      }
      await this.refreshFiles();
      if(doc && project === this.project) {
        await this.open(`assets/${destination}`);
      }
      this.status(`Renamed to assets/${destination}`);
    } catch(error) {
      this.error(error);
    }
  }

  /**
   * Delete an asset after explicit confirmation, retaining guards for open drafts.
   * @param path
   */
  async deleteAsset(path: string) {
    const project = this.project;
    if(!await this.capture(path)) {
      return;
    }
    if(project !== this.project) {
      return;
    }
    const doc = this.documents.get(path);
    if(doc?.dirty || doc?.saving) {
      this.status('Save or discard edits before deleting this asset.');
      return;
    }
    if(!confirm(`Permanently delete ${path}?`)) {
      return;
    }
    try {
      await api('file', {project, path}, undefined, 'DELETE');
      if(project !== this.project) {
        return;
      }
      if(doc) {
        await this.close(path);
      }
      await this.refreshFiles();
      this.status(`Deleted ${path}`);
    } catch(error) {
      this.error(error);
    }
  }

  /**
   * Find project assets referenced by a loaded document.
   * @param path
   */
  async showDependencies(path: string) {
    const doc = this.documents.get(path);
    if(!doc) {
      return;
    }
    const dependencies = this.files.filter(file => file.path !== doc.path && (doc.text.includes(file.path) || doc.text.includes(file.path.split('/').at(-1) ?? '')));
    if(dependencies.length === 0) {
      this.status('No project asset references were found in this document.');
      return;
    }
    const choice = prompt(`Referenced assets:\n${dependencies.map((file, index) => `${index + 1}. ${file.path}`).join('\n')}\n\nEnter a number to open:`, '1');
    const dependency = dependencies[Number(choice) - 1];
    if(dependency) {
      await this.open(dependency.path);
    }
  }

  /**
   * Refresh asset discovery only while the originating project remains active. */
  async refreshFiles() {
    if(!this.project || this.disposed) {
      return;
    }
    const project = this.project;
    try {
      const files = await api('files', {project});
      if(project !== this.project || this.disposed) {
        return;
      }
      this.files = files;
      this.renderFiles();
    } catch(error) {
      this.error(error);
    }
  }

  /**
   * Open one owned tool frame. Inactive tools suspend their preview work, so tabs
   * remain available without an arbitrary document-count limit.
   * @param path Project-relative asset path.
   */
  async open(path: string) {
    if(this.documents.has(path)) {
      this.activate(path);
      return;
    }
    const project = this.project;
    try {
      const disk = await api('file', {project, path});
      if(project !== this.project || this.disposed) {
        return;
      }
      if(this.documents.has(path)) {
        this.activate(path);
        return;
      }
      const doc = new WorkspaceDocument(project, path, disk);
      this.documents.set(path, doc);
      const frame = document.createElement('iframe');
      frame.className = 'tool-frame';
      frame.title = `Edit ${path}`;
      frame.src = editorUrl(path);
      this.frames.set(path, frame);
      element('editors').append(frame);
      this.activate(path);
    } catch(error) {
      this.error(error);
    }
  }

  /** @param path */
  activate(path: string) {
    if(path !== this.maximizedPath) {
      this.setWorkspaceMaximized('', false);
    }
    this.previewTabActive = false;
    element('game-stage').hidden = this.preview.running && this.preview.mode === PLAY_MODE.EDITOR;
    this.active = path;
    this.selectedAsset = path;
    for(const [key, frame] of this.frames) {
      frame.hidden = key !== path;
      this.sendTool(key, {type: WORKSPACE_MESSAGE.ACTIVE, active: key === path});
    }
    for(const [key, repair] of this.repairs) {
      repair.hidden = key !== path;
    }
    this.renderDocuments();
    this.renderFiles();
    this.remember();
  }

  /** @param path */
  async close(path: string) {
    if(!await this.capture(path)) {
      return;
    }
    const doc = this.documents.get(path);
    if(doc?.saving) {
      this.status('Wait for this save to finish before closing.');
      return;
    }
    if(doc?.dirty && !confirm(`Discard unsaved edits to ${path}?`)) {
      return;
    }
    this.repairs.get(path)?.remove();
    this.repairs.delete(path);
    this.frames.get(path)?.remove();
    this.frames.delete(path);
    this.documents.delete(path);
    this.ready.delete(path);
    if(this.active === path) {
      this.active = [...this.documents.keys()].at(-1) ?? '';
    }
    this.activate(this.active);
  }

  /**
   * Save captured source against its disk revision; later edits remain in the tab.
   * @param path Project-relative asset path.
   */
  async save(path: string) {
    if(!await this.capture(path)) {
      return;
    }
    const doc = this.documents.get(path);
    if(!doc || doc.saving || !doc.dirty) {
      return;
    }
    if(doc.conflict) {
      this.status('Resolve the disk conflict before saving.');
      return;
    }
    const snapshot = doc.snapshot();
    doc.saving = true;
    this.renderDocuments();
    try {
      const disk = await api('file', {project: doc.project, path}, snapshot);
      doc.saved(snapshot, disk);
      if(this.documents.get(path) !== doc) {
        return;
      }
      this.status(`Saved ${path}`);
      if(this.repairs.has(path)) {
        this.loadFrame(doc);
      }
      // Runtime consumes committed source. Unsaved edits remain isolated in the asset preview.
      this.sendGame({type: PREVIEW_COMMAND.ASSET, path, text: snapshot.text});
      this.notifyAssets(path);
    } catch(error) {
      const failure = ((error) as Error & {
    current?: DiskDocument;
});
      if(failure.current) {
        doc.conflict = failure.current;
      }
      this.error(error);
    } finally {
      doc.saving = false;
      this.renderDocuments();
    }
  }

  /**
   * Reconcile disk revisions after capturing each tool, without overlapping polls. */
  async poll() {
    if(this.polling || this.disposed) {
      return;
    }
    this.polling = true;
    try {
      for(const doc of this.documents.values()) {
        if(doc.saving) {
          continue;
        }
        if(!await this.capture(doc.path, true)) {
          continue;
        }
        const disk = await api('file', {project: doc.project, path: doc.path});
        if(this.documents.get(doc.path) !== doc || this.disposed) {
          continue;
        }
        if(doc.external(disk)) {
          this.loadFrame(doc);
          this.sendGame({type: PREVIEW_COMMAND.ASSET, path: doc.path, text: disk.text});
          this.notifyAssets(doc.path);
          this.status(`Reloaded external change: ${doc.path}`);
        }
      }
      this.renderDocuments();
    } catch(error) {
      this.error(error);
    }
    finally {
      this.polling = false;
    }
  }

  /**
   * Route same-origin messages only after matching their owning game or tool frame.
   * @param event
   */
  message(event: MessageEvent) {
    if(event.origin !== location.origin || !event.data) {
      return;
    }
    const message = event.data;
    if(this.preview.receive(event)) {
      return;
    }
    if(message.channel !== WORKSPACE_CHANNEL.TOOL) {
      return;
    }
    const entry = [...this.frames].find(([, frame]) => frame.contentWindow === event.source);
    if(!entry) {
      return;
    }
    const [path] = entry;
    const doc = this.documents.get(path);
    if(!doc) {
      return;
    }
    if(message.type === WORKSPACE_MESSAGE.MAXIMIZE_WORKSPACE) {
      if(path === this.active && !this.previewTabActive) {
        this.setWorkspaceMaximized(path, message.maximized === true);
      }
    } else if(message.type === WORKSPACE_MESSAGE.FOCUS_SEARCH) {
      if(path === this.active) {
        this.focusSearch();
      }
    } else if(message.type === WORKSPACE_MESSAGE.REQUEST) {
      void this.toolRequest(path, message);
    } else if(message.type === WORKSPACE_MESSAGE.READY) {
      this.loadFrame(doc);
      this.sendTool(path, {type: WORKSPACE_MESSAGE.ACTIVE, active: this.active === path});
    } else if(message.type === WORKSPACE_MESSAGE.LOADED) {
      this.ready.add(path);
      this.repairs.get(path)?.remove();
      this.repairs.delete(path);
    } else if(message.type === WORKSPACE_MESSAGE.CHANGED && typeof message.text === 'string') {
      doc.edit(message.text);
      this.renderDocuments();
    } else if(message.type === WORKSPACE_MESSAGE.CAPTURED) {
      this.captures.get(message.request)?.(message.ok === true);
    } else if(message.type === WORKSPACE_MESSAGE.SAVE) {
      void this.save(path);
    } else if(message.type === WORKSPACE_MESSAGE.ERROR) {
      this.status(`${path}: ${message.message}`);
      if(message.phase === 'load') {
        this.repair(doc, String(message.message));
      }
    }
  }

  /**
   * Perform a tool request within the owning document's project and lifetime.
   * @param owner @param message
   */
  async toolRequest(owner: string, message: {
    request: string;
    operation: string;
    path?: string;
    text?: string;
}) {
    const document = this.documents.get(owner);
    if(!document) {
      return;
    }
    const project = document.project;
    try {
      let result;
      if(message.operation === WORKSPACE_OPERATION.FOCUS_SEARCH) {
        this.focusSearch();
        result = true;
      } else if(message.operation === WORKSPACE_OPERATION.CATALOG) {
        result = await api('files', {project});
      } else if(message.operation === WORKSPACE_OPERATION.READ && typeof message.path === 'string') {
        result = (await api('file', {project, path: message.path})).text;
      } else if(message.operation === WORKSPACE_OPERATION.CREATE && typeof message.path === 'string' && typeof message.text === 'string') {
        result = await api('file', {project, path: message.path}, {text: message.text, revision: ''}, 'POST');
        if(project === this.project) {
          await this.refreshFiles();
        }
      } else if(message.operation === WORKSPACE_OPERATION.OPEN && typeof message.path === 'string') {
        if(!await this.capture(owner)) {
          throw new Error('Fix invalid edits before opening the source.');
        }
        await this.open(message.path);
        if(this.active !== message.path) {
          throw new Error('Close a document tab before opening the source.');
        }
        this.returnPath = owner;
        const back = element('return-owner');
        back.textContent = `← ${owner.split('/').at(-1)}`;
        back.hidden = false;
        back.parentElement?.removeAttribute('hidden');
        result = true;
      } else {
        throw new Error('Unsupported asset operation.');
      }
      if(this.documents.get(owner) === document) {
        this.sendTool(owner, {type: WORKSPACE_MESSAGE.RESPONSE, request: message.request, result});
      }
    } catch(error) {
      if(this.documents.get(owner) === document) {
        this.sendTool(owner, {type: WORKSPACE_MESSAGE.RESPONSE, request: message.request, error: error instanceof Error ? error.message : String(error)});
      }
    }
  }

  /**
   * Refresh dependent previews after committed source changes; preserve their authoring history.
   * @param changedPath
   */
  notifyAssets(changedPath: string) {
    for(const path of this.frames.keys()) {
      if(path !== changedPath) { this.sendTool(path, {type: WORKSPACE_MESSAGE.ASSETS_CHANGED, changedPath}); }
    }
  }

  /** @param doc */
  loadFrame(doc: WorkspaceDocument) {
    this.ready.delete(doc.path);
    this.sendTool(doc.path, {type: WORKSPACE_MESSAGE.LOAD, path: doc.path, text: doc.text});
  }

  /**
   * Keep invalid structured assets editable as their real source, never as a stale example.
   * @param doc @param message
   */
  repair(doc: WorkspaceDocument, message: string) {
    this.ready.delete(doc.path);
    this.repairs.get(doc.path)?.remove();
    const panel = document.createElement('section');
    panel.className = 'repair-editor';
    panel.hidden = doc.path !== this.active;
    const heading = document.createElement('p');
    heading.textContent = `Preview unavailable: ${message} Edit the source below and save to retry.`;
    const source = document.createElement('textarea');
    source.setAttribute('aria-label', `Repair ${doc.path}`);
    source.spellcheck = false;
    source.value = doc.text;
    source.addEventListener(BROWSER_EVENT.INPUT, () => {
      doc.edit(source.value);
      this.renderDocuments();
    });
    panel.append(heading, source);
    this.repairs.set(doc.path, panel);
    element('editors').append(panel);
  }
  /** @param path @param data */
  sendTool(path: string, data: Record<string, unknown>) {
    this.frames.get(path)?.contentWindow?.postMessage({channel: WORKSPACE_CHANNEL.HOST, ...data}, location.origin);
  }
  /** @param data */
  sendGame(data: Record<string, unknown>) {
    this.preview.send(data);
  }

  /**
   * Flush the tool's edits before any operation that can save or discard the tab. @param path */
  async capture(path: string, passive: boolean = false) {
    if(!this.ready.has(path)) {
      return true;
    }
    const request = crypto.randomUUID();
    const captured = await new Promise(resolve => {
      const finish = (ok: boolean) => {
        clearTimeout(timeout);
        this.captures.delete(request);
        resolve(ok);
      };
      const timeout = window.setTimeout(() => finish(false), 3000);
      this.captures.set(request, finish);
      this.sendTool(path, {type: WORKSPACE_MESSAGE.CAPTURE, request, passive});
    });
    if(!captured && !passive) {
      this.status('The editor did not acknowledge its current text. Try again before saving or closing.');
    }
    return captured;
  }

  /**
   * Replace the game frame with a fresh session while retaining authoring documents. */
  run(restart: boolean = false) {
    const project = this.projects.find(item => item.id === this.project);
    if(!project) {
      return;
    }
    if(this.preview.running && !restart) {
      this.focusGame(); return;
    }
    if(!this.stop()) {
      return;
    }
    const mode = ((select('play-mode').value) as PlayMode);
    let size = null;
    if(mode === PLAY_MODE.WINDOW) {
      const preset = select('play-size').value;
      if(preset === 'custom') {
        let previousSize = '1280x720';
        try { previousSize = localStorage.getItem(WORKSPACE_STORAGE_KEY.CUSTOM_SIZE) ?? previousSize; } catch { /* Optional preferences. */ }
        const answer = prompt('Game content size in CSS pixels (width × height):', previousSize);
        if(answer === null) {
          return;
        }
        const match = answer.match(/^\s*(\d+)\s*[x×]\s*(\d+)\s*$/i);
        if(!match || Number(match[1]) < 320 || Number(match[2]) < 240 || Number(match[1]) > 8192 || Number(match[2]) > 8192) {
          this.status('Enter a size from 320×240 to 8192×8192.'); return;
        }
        size = {width: Number(match[1]), height: Number(match[2])};
        try { localStorage.setItem(WORKSPACE_STORAGE_KEY.CUSTOM_SIZE, answer); } catch { /* Run still uses the entered size. */ }
      } else if(preset === '720' || preset === '1080') {
        size = preset === '720' ? {width: 1280, height: 720} : {width: 1920, height: 1080};
      }
      if(size && (size.width > screen.availWidth - 40 || size.height > screen.availHeight - 100)) {
        if(!confirm(`${size.width}×${size.height} may not fit this display. Use Fit to Screen instead?`)) {
          return;
        }
        size = null;
      }
    }
    this.previewPlaceholder.remove();
    const stage = element('game-stage');
    stage.hidden = false;
    if(mode === PLAY_MODE.EDITOR) {
      element('editors').append(stage);
    } else {
      element('preview-panel').insertBefore(stage, element('runtime-info'));
    }
    this.preview.start(project, mode, size);
    if(this.preview.running) {
      this.focusGame();
    }
  }

  /** Restore authoring after stopping, without recreating document frames. */
  stop() {
    if(!this.preview.stop()) {
      return false;
    }
    return true;
  }

  /** Focus the single owned game destination. */
  focusGame() {
    this.setWorkspaceMaximized('', false);
    if(!this.preview.running) {
      return;
    }
    if(this.preview.mode === PLAY_MODE.EDITOR) {
      this.previewTabActive = true;
      for(const [path, frame] of this.frames) { frame.hidden = true; this.sendTool(path, {type: WORKSPACE_MESSAGE.ACTIVE, active: false}); }
      for(const repair of this.repairs.values()) { repair.hidden = true; }
      element('game-stage').hidden = false;
      this.collapsePreview(true);
      this.renderDocuments();
    } else if(this.preview.mode === PLAY_MODE.DOCKED) {
      this.collapsePreview(false);
    }
    this.preview.focus();
  }

  /** @param state */
  previewChanged(state: PreviewState) {
    element('connection').textContent = state.state;
    element('connection').title = state.message;
    element('runtime-note').textContent = state.message;
    this.status(state.message);
    button('pause').disabled = !state.connected;
    button('pause').textContent = state.paused ? 'Resume' : 'Pause';
    button('pause').setAttribute('aria-label', state.paused ? 'Resume game preview' : 'Pause game preview');
    button('pause').setAttribute('aria-pressed', String(state.paused));
    button('run').setAttribute('aria-pressed', String(state.running));
    button('run').setAttribute('aria-label', state.running ? 'Focus running game preview' : 'Play game preview');
    button('stop').disabled = !state.running || this.preview.independent;
    button('restart').disabled = !state.running || this.preview.independent;
    button('preview-mode').disabled = !state.running;
    element('finish-external').hidden = !this.preview.independent;
    element('regenerate').hidden = !state.pendingWorld;
    element('regenerate-global').hidden = !state.pendingWorld;
    if(!state.running) {
      this.previewTabActive = false;
      const stage = element('game-stage');
      stage.hidden = false;
      stage.replaceChildren(this.previewPlaceholder);
      element('preview-panel').insertBefore(stage, element('runtime-info'));
      this.collapsePreview(true);
      this.activate(this.active);
    }
    this.renderDocuments();
  }

  /** Temporary expansion never overwrites the surrounding panels' preferences.
   * @param path @param maximized
   */
  setWorkspaceMaximized(path: string, maximized: boolean) {
    const previous = this.maximizedPath;
    if(previous && previous !== path) {
      this.sendTool(previous, {type: WORKSPACE_MESSAGE.WORKSPACE_MAXIMIZED, maximized: false});
    }
    this.maximizedPath = maximized ? path : '';
    element('workspace').classList.toggle('lab-maximized', maximized);
    const assetsVisible = !maximized && !element('workspace').classList.contains('assets-collapsed');
    button('assets-toggle').setAttribute('aria-expanded', String(assetsVisible));
    button('assets-toggle').classList.toggle('selected', assetsVisible);
    if(path) {
      this.sendTool(path, {type: WORKSPACE_MESSAGE.WORKSPACE_MAXIMIZED, maximized});
    }
  }

  /** @param collapsed */
  setAssets(collapsed: boolean) {
    this.setWorkspaceMaximized('', false);
    element('workspace').classList.toggle('assets-collapsed', collapsed);
    button('assets-toggle').setAttribute('aria-expanded', String(!collapsed));
    button('assets-toggle').classList.toggle('selected', !collapsed);
    this.saveAssets();
  }
  restoreAssets() {
    this.assetsWidth = 220;
    try {
      const saved = JSON.parse(localStorage.getItem(WORKSPACE_STORAGE_KEY.ASSETS) ?? '{}');
      if(Number.isFinite(saved.width)) {
        this.assetsWidth = Math.max(160, Math.min(600, saved.width));
      }
      element('workspace').classList.toggle('assets-collapsed', saved.collapsed === true);
      button('assets-toggle').setAttribute('aria-expanded', String(saved.collapsed !== true));
      button('assets-toggle').classList.toggle('selected', saved.collapsed !== true);
    } catch { /* Defaults remain usable when preferences cannot be read. */ }
    this.applyAssets();
  }
  applyAssets() {
    const width = Math.max(160, Math.min(window.innerWidth*0.3, this.assetsWidth));
    document.documentElement.style.setProperty('--assets-width', `${width}px`);
    element('assets-divider').setAttribute('aria-valuenow', String(Math.round(width)));
    element('assets-divider').setAttribute('aria-valuemin', '160');
    element('assets-divider').setAttribute('aria-valuemax', String(Math.round(window.innerWidth*0.3)));
  }
  saveAssets() {
    try { localStorage.setItem(WORKSPACE_STORAGE_KEY.ASSETS, JSON.stringify({width: this.assetsWidth, collapsed: element('workspace').classList.contains('assets-collapsed')})); }
    catch { /* Preference storage is optional. */ }
  }

  /** Store Play preferences only; never restore preview UI or a running game. */
  rememberPlay() {
    element('play-size').hidden = select('play-mode').value !== PLAY_MODE.WINDOW;
    try { localStorage.setItem(WORKSPACE_STORAGE_KEY.PLAY, JSON.stringify({mode: select('play-mode').value, size: select('play-size').value})); }
    catch { /* In-memory controls remain usable when storage is unavailable. */ }
    if(this.preview.running) {
      this.status('Play preference changed. Restart applies it and resets simulation.');
    }
  }

  restorePlay() {
    try {
      const value = JSON.parse(localStorage.getItem(WORKSPACE_STORAGE_KEY.PLAY) ?? '{}');
      if(([PLAY_MODE.DOCKED, PLAY_MODE.EDITOR, PLAY_MODE.BROWSER, PLAY_MODE.WINDOW] as readonly string[]).includes(value.mode)) {
        select('play-mode').value = value.mode;
      }
      if(['fit', '720', '1080', 'custom'].includes(value.size)) {
        select('play-size').value = value.size;
      }
    } catch { /* Invalid preferences use the initial editor-tab destination. */ }
    element('play-size').hidden = select('play-mode').value !== PLAY_MODE.WINDOW;
  }

  renderFiles() {
    const query = input('search').value.toLowerCase();
    const files = this.files.filter(file =>
      (!query || file.path.toLowerCase().includes(query)) &&
      (this.filters.size === 0 || this.filters.has(file.kind.startsWith('form') ? 'form' : file.kind))
    );
    const nodes = [];
    let folder = '';
    for(const file of files) {
      const directory = file.path.slice(0, file.path.lastIndexOf('/'));
      if(directory !== folder) {
        const heading = document.createElement('div');
        heading.className = 'folder-name';
        heading.textContent = directory;
        nodes.push(heading);
        folder = directory;
      }
      const item = document.createElement('div');
      item.className = 'file-item';
      const row = document.createElement('button');
      row.className = `file-row${file.path === this.selectedAsset ? ' selected' : ''}`;
      row.title = file.path;
      const icon = document.createElement('span');
      icon.className = `asset-icon${file.kind === 'joyfx' ? ' effect' : ''}`;
      icon.setAttribute('aria-hidden', 'true');
      icon.textContent = file.kind === 'joyfx' ? 'FX' : file.kind === 'joylevel' ? 'L' : file.kind === 'joyobject' ? 'O' : 'F';
      const name = document.createElement('span');
      name.className = 'name';
      name.textContent = file.path.split('/').at(-1) ?? file.path;
      row.append(icon, name);
      row.setAttribute('aria-label', file.path);
      item.dataset.path = file.path;
      row.addEventListener(BROWSER_EVENT.CLICK, () => {
        this.selectedAsset = file.path;
        for(const node of element('files').querySelectorAll('.file-row')) {
          node.classList.toggle('selected', node === row);
        }
        void this.open(file.path);
      });
      row.addEventListener(BROWSER_EVENT.KEYDOWN, event => {
        if(event.key === KEY_CODE.ENTER) {
          event.preventDefault();
          void this.open(file.path);
        } else if(event.key === 'F2') {
          event.preventDefault();
          void this.renameAsset(file.path);
        }
      });
      row.addEventListener(BROWSER_EVENT.CONTEXTMENU, event => {
        event.preventDefault();
        this.openAssetMenu(file.path, row);
      });
      const actions = document.createElement('div');
      actions.className = 'file-actions';
      const more = assetAction('⋯', () => this.openAssetMenu(file.path, more));
      more.setAttribute('aria-label', `Actions for ${file.path}`);
      more.setAttribute('aria-haspopup', 'menu');
      const remove = assetAction('', () => void this.deleteAsset(file.path), 'danger');
      remove.setAttribute('aria-label', `Delete ${file.path}`);
      remove.title = 'Delete asset';
      remove.innerHTML = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 7h14M9 7V4h6v3M7 7l1 13h8l1-13M10 10v7M14 10v7"/></svg>';
      actions.append(more, remove);
      item.append(row, actions);
      nodes.push(item);
    }
    const focus = document.activeElement;
    const focusedPath = focus instanceof HTMLElement ? focus.closest('.file-item')?.getAttribute('data-path') : null;
    const focusedAction = focus instanceof HTMLElement ? focus.getAttribute('aria-label') : null;
    const menuAnchorLabel = this.menuAnchor?.getAttribute('aria-label');
    element('files').replaceChildren(...nodes);
    if(this.menuPath) {
      const item = nodes.find(node => node.dataset.path === this.menuPath);
      this.menuAnchor = [...(item?.querySelectorAll('button') ?? [])].find(node => node.getAttribute('aria-label') === menuAnchorLabel) ?? null;
      if(!this.menuAnchor) {
        const menuFocused = element('asset-menu').contains(document.activeElement);
        this.closeAssetMenu(false);
        if(menuFocused) {
          input('search').focus();
        }
      }
    }
    if(focusedPath) {
      const item = nodes.find(node => node.dataset.path === focusedPath);
      const target = [...(item?.querySelectorAll('button') ?? [])].find(node => node.getAttribute('aria-label') === focusedAction);
      target?.focus({preventScroll: true});
    }
    element('file-count').textContent = `${this.files.length} ASSETS`;
    const restricted = this.filters.size > 0;
    element('filter-count').textContent = String(this.filters.size);
    element('filter-count').hidden = !restricted;
    button('filter-toggle').setAttribute('aria-label', restricted ? `Filter assets, ${this.filters.size} types selected` : 'Filter assets');
    button('filter-toggle').classList.toggle('selected', restricted);
    for(const checkbox of element('asset-filters').querySelectorAll('input')) {
      checkbox.checked = this.filters.has(checkbox.value);
    }
  }

  focusSearch() {
    this.setWorkspaceMaximized('', false);
    this.setAssets(false);
    input('search').focus();
  }

  /** @param open @param [focus] */
  setFiltersOpen(open: boolean, focus: boolean = true) {
    element('asset-filters').hidden = !open;
    button('filter-toggle').setAttribute('aria-expanded', String(open));
    if(focus) {
      (open ? button('filter-all') : button('filter-toggle')).focus();
    }
  }

  /** @param path @param anchor */
  openAssetMenu(path: string, anchor: HTMLElement) {
    this.menuPath = path;
    this.menuAnchor = anchor;
    const bounds = anchor.getBoundingClientRect();
    const menu = element('asset-menu');
    menu.hidden = false;
    menu.style.left = `${Math.min(bounds.left, window.innerWidth - menu.offsetWidth)}px`;
    menu.style.top = `${Math.min(bounds.bottom, window.innerHeight - menu.offsetHeight)}px`;
    button('rename-asset').focus();
  }

  /** @param [focus] */
  closeAssetMenu(focus: boolean = true) {
    if(!element('asset-menu').hidden) {
      element('asset-menu').hidden = true;
      if(focus) {
        this.menuAnchor?.focus();
      }
    }
    this.menuAnchor = null;
    this.menuPath = '';
  }

  renderDocuments() {
    const tabs = [];
    for(const [path, doc] of this.documents) {
      const tab = document.createElement('div');
      tab.className = `tab${path === this.active && !this.previewTabActive ? ' active' : ''}`;
      const open = document.createElement('button');
      open.textContent = `${doc.conflict ? '! ' : doc.dirty ? '● ' : ''}${path.split('/').at(-1)}`;
      open.title = `${doc.project} / ${path}`;
      open.setAttribute('aria-label', `${path}${doc.dirty ? ', unsaved changes' : ', saved'}`);
      open.addEventListener(BROWSER_EVENT.FOCUS, () => {
        const tooltip = element('path-tooltip');
        tooltip.textContent = `${doc.project} / ${path}`;
        tooltip.hidden = false;
        const bounds = open.getBoundingClientRect();
        tooltip.style.left = `${Math.min(bounds.left, window.innerWidth - tooltip.offsetWidth - 12)}px`;
        tooltip.style.top = `${bounds.bottom}px`;
      });
      open.addEventListener(BROWSER_EVENT.BLUR, () => { element('path-tooltip').hidden = true; });
      open.setAttribute('role', 'tab');
      open.setAttribute('aria-selected', String(path === this.active && !this.previewTabActive));
      open.addEventListener(BROWSER_EVENT.CLICK, () => this.activate(path));
      const close = document.createElement('button');
      close.className = 'close';
      close.textContent = '×';
      close.setAttribute('aria-label', `Close ${path}`);
      close.addEventListener(BROWSER_EVENT.CLICK, () => void this.close(path));
      tab.append(open, close);
      tabs.push(tab);
    }
    if(this.preview.running && this.preview.mode === PLAY_MODE.EDITOR) {
      const tab = document.createElement('div');
      tab.className = `tab${this.previewTabActive ? ' active' : ''}`;
      const open = document.createElement('button');
      open.textContent = 'Game Preview'; open.setAttribute('role', 'tab'); open.setAttribute('aria-selected', String(this.previewTabActive));
      open.addEventListener(BROWSER_EVENT.CLICK, () => this.focusGame());
      const close = document.createElement('button'); close.textContent = '×'; close.className = 'close'; close.setAttribute('aria-label', 'Close Game Preview');
      close.addEventListener(BROWSER_EVENT.CLICK, () => this.stop());
      tab.append(open, close); tabs.push(tab);
    }
    const focused = document.activeElement;
    const focusKey = focused instanceof HTMLButtonElement && element('tabs').contains(focused) ? focused.title || focused.getAttribute('aria-label') : null;
    element('tabs').replaceChildren(...tabs);
    if(focusKey) {
      const restored = [...element('tabs').querySelectorAll('button')].find(node => (node.title || node.getAttribute('aria-label')) === focusKey);
      restored?.focus({preventScroll: true});
    }
    const doc = this.documents.get(this.active);
    element('empty').hidden = Boolean(doc) || this.previewTabActive;
    element('document-state').title = doc ? `${this.project} / ${doc.path}` : '';
    element('return-owner').parentElement?.toggleAttribute('hidden', Boolean(element('return-owner').hidden));
    element('document-state').textContent = this.previewTabActive ? 'RUNNING SESSION' : doc?.saving ? 'SAVING…' : doc?.conflict ? 'DISK CONFLICT' : doc?.dirty ? 'UNSAVED CHANGES' : 'SAVED TO PROJECT';
    element('conflict').hidden = !doc?.conflict;
    button('save').disabled = !doc?.dirty || doc.saving || Boolean(doc.conflict);
  }

  /**
   * Store the preview pane width in CSS pixels, bounded to the desktop viewport.
   * @param width
   */
  resizePreview(width: number) {
    const clamped = Math.max(280, Math.min(window.innerWidth*0.52, width));
    this.session.width = clamped;
    this.applyPreviewWidth();
    this.remember();
  }
  /** Clamp displayed width while retaining the user's wider-screen preference. */
  applyPreviewWidth() {
    const preferred = Number.isFinite(this.session.width) && this.session.width > 0 ? this.session.width : 480;
    document.documentElement.style.setProperty('--preview-width', `${Math.max(280, Math.min(window.innerWidth*0.52, preferred))}px`);
  }
  /** @param collapsed */
  collapsePreview(collapsed: boolean) {
    element('workspace').classList.toggle('preview-collapsed', collapsed);
    element('show-preview').hidden = !collapsed || !this.preview.running || this.preview.mode !== PLAY_MODE.DOCKED;
  }
  /** @param message */
  status(message: string) {
    element('status').textContent = message;
  }
  /** @param error */
  error(error: unknown) {
    this.status(error instanceof Error ? error.message : String(error));
  }
  /**
   * Persist navigation and layout only; unsaved source remains in memory. */
  remember() {
    this.session.project = this.project;
    this.session.tabs[this.project] = [...this.documents.keys()];
    this.session.active[this.project] = this.active;
    try {
      localStorage.setItem(WORKSPACE_STORAGE_KEY.LAYOUT, JSON.stringify(this.session));
    } catch {
      // Disk content remains authoritative when UI preferences cannot be stored.
    }
  }
  restore() {

    const empty: {
    project: string;
    tabs: Record<string, string[]>;
    active: Record<string, string>;
    width: number;
} = {project: '', tabs: {}, active: {}, width: 0};
    try {
      const saved = JSON.parse(localStorage.getItem(WORKSPACE_STORAGE_KEY.LAYOUT) ?? 'null');
      if(saved?.tabs && saved?.active) {
        saved.width = typeof saved.width === 'number' && Number.isFinite(saved.width) && saved.width > 0 ? saved.width : 0;
        return  ((saved) as typeof empty);
      }
    } catch {
      // Ignore invalid UI preferences; document source is loaded from disk.
    }
    return empty;
  }
  /**
   * Stop polling, settle pending captures and unload all owned frames. */
  destroy() {
    this.disposed = true;
    this.listeners.abort();
    clearInterval(this.pollTimer);
    clearInterval(this.refreshTimer);
    for(const finish of this.captures.values()) {
      finish(false);
    }
    for(const frame of this.frames.values()) {
      frame.remove();
    }
    for(const repair of this.repairs.values()) {
      repair.remove();
    }
    this.preview.changed = () => {};
    this.preview.destroy();
  }
}

/** @param route @param [query] @param [body] @param [method] */
async function api(route: string, query: Record<string, string> = {}, body?: unknown, method: 'GET' | 'PUT' | 'POST' | 'PATCH' | 'DELETE' = 'PUT') {
  const response = await fetch(`/__joy_editor/${route}?${new URLSearchParams(query)}`, {
    method: body === undefined && method === 'PUT' ? 'GET' : method,
    headers: {'X-Joy-Editor': 'workspace', 'Content-Type': 'application/json'},
    body: body ? JSON.stringify(body) : undefined
  });
  const result = await response.json();
  if(!response.ok) {
    throw Object.assign(new Error(result.error ?? 'Project service unavailable.'), {current: result.current});
  }
  return result;
}
/** @param id */
function element(id: string) {
  const value = document.getElementById(id);
  if(!value) {
    throw new Error(`Missing workspace element ${id}`);
  }
  return value;
}
/** @param id */
function button(id: string) {
  return  ((element(id)) as HTMLButtonElement);
}
/** @param id */
function input(id: string) {
  return  ((element(id)) as HTMLInputElement);
}
/** @param id */
function select(id: string) {
  return  ((element(id)) as HTMLSelectElement);
}

/**
 * Create one compact action for an asset hierarchy entry.
 * @param label
 * @param action
 * @param [className]
 */
function assetAction(label: string, action: () => void, className: string = '') {
  const control = document.createElement('button');
  control.type = 'button';
  control.className = className;
  control.textContent = label;
  control.addEventListener(BROWSER_EVENT.CLICK, event => {
    event.stopPropagation();
    action();
  });
  return control;
}
/** @param id */
function anchor(id: string) {
  return  ((element(id)) as HTMLAnchorElement);
}

const workspace = new JoyWorkspace();
void workspace.start();
window.addEventListener(BROWSER_EVENT.PAGEHIDE, event => {
  if(!event.persisted) {
    workspace.destroy();
  }
});
if(import.meta.hot) {
  import.meta.hot.dispose(() => workspace.destroy());
}
