// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { FORM_VIEW_MODE } from '../../src/constants.ts';
import { BROWSER_EVENT, GPU_BACKEND } from 'joy-engine/constants';
import type { GeometryData, Material, ParameterValue } from 'joy-engine/form';
import type { FormDocument } from '../../src/document.ts';
import type { EditorView } from '@codemirror/view';
import {PanelLayout} from '../../../../site/src/interaction/panel-layout.ts';
import {NumericControls} from '../../../../site/src/interaction/numeric-controls.ts';
import {guardNumberScroll} from '../../../../site/src/interaction/number-scroll.ts';
import '../css/style.css';
import { hosted, connectWorkspaceTool, requestWorkspaceMaximize } from '../../../../site/src/workspace/tool-bridge.ts';
import { version } from '../../package.json';
import { mountEditorNavigation } from '../../../../site/src/navigation.ts';
import '@fontsource/archivo/500.css';
import '@fontsource/archivo/700.css';
import '@fontsource/archivo-black/400.css';
import { VariantControls } from '../../src/variant-controls.ts';
import { WorkspaceLayout } from '../../src/workspace-layout.ts';
import { connectScriptGuide } from '../../src/script-guide.ts';
import { OutputInspector } from '../../src/output-inspector.ts';
import { MaterialControls } from '../../src/material-controls.ts';
import { readMaterials } from 'joy-engine/form';
import { exportGlb } from '../../src/export-glb.ts';
import { projectNameChange } from '../../src/project-metadata.ts';
import {COMPILING_PHASE,STARTUP_TIMEOUT_MILLISECONDS,EXECUTION_TIMEOUT_MILLISECONDS} from '../../src/compiler-protocol.ts';
import { ParameterControls, updateRangeFill } from '../../src/parameter-controls.ts';
import { createEditor } from '../../src/editor.ts';
import { Viewport } from '../../src/rendering/viewport.ts';
import { EXAMPLES } from '../../src/examples.ts';
import { readDocument, serializeDocument, download, fileStem } from '../../src/document.ts';
import { DOCUMENT_LIMITS } from 'joy-engine/form';
import { exportObj, exportPly } from '../../src/export.ts';
export type { GeometryData };

/** Composition root: owns document, editor, worker and viewport lifetimes. */
class FormLab {
  declare name: string;
  declare materials: Material[];
  declare overrides: Record<string, ParameterValue>;
  declare data: GeometryData | null;
  declare worker: Worker | null;
  declare debounce: number;
  declare timeout: number;
  declare shouldFrame: boolean;
  declare disposed: boolean;
  declare replacing: boolean;
  declare hasEdits: boolean;
  declare parameters: ParameterControls;
  declare listeners: AbortController;
  declare panelLayout: PanelLayout;
  declare materialControls: MaterialControls;
  declare editor: EditorView;
  declare viewport: Viewport;
  declare inspection: OutputInspector;
  declare variants: VariantControls;
  declare numeric: NumericControls;
  declare layout: WorkspaceLayout;

  constructor() {
    element('tool-version').textContent = version;
    this.name = 'Wave study';
    this.materials = readMaterials(undefined);

    this.overrides = Object.create(null);

    this.data = null;

    this.worker = null;
    this.debounce = 0;
    this.timeout = 0;
    this.shouldFrame = true;
    this.disposed = false;
    this.replacing = false;
    this.hasEdits = false;
    this.parameters = new ParameterControls(
      element('parameters'),
      (name, value) => {
        if(value === undefined) {
          delete this.overrides[name];
        } else {
          this.overrides[name] = value;
        }
        this.hasEdits = true;
        this.persist();
        this.schedule();
      },
    );
    this.listeners = new AbortController();
    guardNumberScroll(document, this.listeners.signal);
    this.panelLayout = new PanelLayout(((document.querySelector('.workbench')) as HTMLElement), {left:  ((document.querySelector('.editor-panel')) as HTMLElement), right:  ((document.querySelector('.inspector')) as HTMLElement), center:  ((document.querySelector('.preview-panel')) as HTMLElement), key: 'joy-editor-panels-form-v2', leftWidth: 320, rightWidth: 260, maximizeMode: 'workspace', onMaximize: requestWorkspaceMaximize});
    const saved = this.restore();
    if(saved) {
      this.name = saved.name;
      this.overrides = Object.assign(Object.create(null), saved.overrides);
      // Autosave may be the only copy; retain replacement protection on reload.
      this.hasEdits = true;
    }
    this.materials = readMaterials(saved?.materials);
    this.materialControls = new MaterialControls(element('materials'), materials => {
      this.materials = materials;
      this.changed();
    });
    this.materialControls.setMaterials(this.materials);
    this.editor = createEditor(
      element('editor'),
      saved?.source ?? EXAMPLES[0].source,
      () => this.changed(),
      () => this.compile(),
      (line, column) => {
        element('cursor-position').textContent = `Ln ${line}, Col ${column}`;
      },
    );
    this.viewport = new Viewport(
       ((element('viewport')) as HTMLCanvasElement),
      (message) => this.gpuError(message),
    );
    this.inspection = new OutputInspector(element('output-toolbar'), element('attribute-panel'), this.viewport, () => {
      if(this.viewport.renderer) {
        element('gpu-error').hidden = true;
        element('backend').textContent = `JOY ENGINE / ${this.viewport.renderer.backend === GPU_BACKEND.WEBGPU ? 'WEBGPU' : 'WEBGL 2'}`;
      }
    });
    this.variants = new VariantControls(element('variants-section'), () => this.overrides, values => {
      serializeDocument({...this.document(), overrides: values});
      this.overrides = Object.assign(Object.create(null), values);
      this.parameters.clear();
      this.changed();
      this.compile();
    }, () => {
      this.hasEdits = true;
      this.persist();
    }, variants => {
      serializeDocument({...this.document(), variants});
    });
    this.variants.setVariants(saved?.variants ?? []);
    this.numeric = new NumericControls(element('parameters'), this.listeners.signal, {
      read: () => JSON.stringify(Object.fromEntries(Object.entries(this.overrides).filter(([,value]) => typeof value === 'number' || Array.isArray(value)))),
      commit: input => this.parameters.changeInput(input, 'number'),
      restore: text => {
        // Property history owns numeric overrides only; imported resources stay current.
        for(const [name,value] of Object.entries(this.overrides)) {
          if(typeof value === 'number' || Array.isArray(value)) {
            delete this.overrides[name];
          }
        }
        Object.assign(this.overrides, JSON.parse(text));
        const declarations = this.parameters.parameters.map(parameter => {
          if(!('min' in parameter)) {
            return parameter;
          }
          const value = this.overrides[parameter.name];
          return {...parameter, value:typeof value === 'number' || Array.isArray(value) ? value : parameter.defaultValue};
        });
        this.parameters.clear();
        this.parameters.update(declarations);
        this.hasEdits = true;
        this.persist();
        this.schedule();
      },
      changed: () => {
        button('property-undo').disabled = !this.numeric?.past.length;
        button('property-redo').disabled = !this.numeric?.future.length;
      }
    });
    button('property-undo').addEventListener(BROWSER_EVENT.CLICK, () => this.numeric.undo(), {signal:this.listeners.signal});
    button('property-redo').addEventListener(BROWSER_EVENT.CLICK, () => this.numeric.redo(), {signal:this.listeners.signal});
    this.layout = new WorkspaceLayout(element('app'), {resize: false});
    this.connectControls();
    updateRangeFill(input('point-size'));
    this.updateName();
  }
  async start() {
    this.compile();
    try {
      const backend = await this.viewport.initialize();
      if(this.disposed) {
        return;
      }
      element('backend').textContent =
        `JOY ENGINE / ${backend === GPU_BACKEND.WEBGPU ? 'WEBGPU' : 'WEBGL 2'}`;
      if(import.meta.env.DEV) {
        console.info(`[Form Lab] Joy Engine ready: ${backend}`);
      }
    } catch (error) {
      this.gpuError(
        `A WebGPU or WebGL 2 browser is required for the preview. ${error instanceof Error ? error.message : String(error)} You can still edit and export your geometry.`,
      );
    }
  }
  changed() {
    if(this.replacing || this.disposed) {
      return;
    }
    this.numeric?.clear();
    this.hasEdits = true;
    this.persist();
    this.schedule();
  }
  schedule() {
    this.setPreviewPending(true);
    this.cancelCompilation();
    if(input('live').checked) {
      this.status('Waiting for your next idea…', 'busy');
      this.debounce = window.setTimeout(() => this.compile(), 240);
    } else {
      this.status('Edited · Run to update the preview.', 'busy');
    }
    this.updateExportNote(true);
  }
  compile() {
    if(this.disposed) {
      return;
    }
    this.cancelCompilation();
    this.setPreviewPending(true);
    this.updateExportNote(true);
    this.status('Starting compiler…', 'busy');
    const worker = new Worker(
      new URL('../../src/compiler-worker.ts', import.meta.url),
      { type: 'module' },
    );
    this.worker = worker;
    const expire = (message: string) => {
      if(this.worker !== worker) {
        return;
      }
      this.cancelCompilation();
      this.previewFailed();
      this.status(message, 'error');
    };
    const executionFailure = 'Build exceeded 2 seconds. Reduce loops or resolution. Previous preview retained.';
    this.timeout = window.setTimeout(() => expire('Compiler startup exceeded 10 seconds. Check module loading or restart the tool. Previous preview retained.'), STARTUP_TIMEOUT_MILLISECONDS);
    worker.onmessage = (event) => {
      if(this.worker !== worker || this.disposed) {
        return;
      }
      const result = event.data;
      if(result.phase === COMPILING_PHASE) {
        clearTimeout(this.timeout);
        this.timeout = window.setTimeout(() => expire(executionFailure), EXECUTION_TIMEOUT_MILLISECONDS);
        this.status('Building geometry…', 'busy');
        return;
      }
      this.cancelCompilation();
      // Result delivery may wait behind GPU/main-thread startup; worker time owns this budget.
      if(result.ok && result.milliseconds > EXECUTION_TIMEOUT_MILLISECONDS) {
        this.previewFailed();
        this.status(executionFailure, 'error');
        return;
      }
      if(!result.ok) {
        this.previewFailed();
        this.status(
          `Line ${result.line}, column ${result.column}: ${result.message} Previous preview retained.`,
          'error',
        );
        button('error-location').hidden = false;
        button('error-location').dataset.line = String(result.line);
        button('error-location').dataset.column = String(result.column);
        return;
      }
      this.data = result.data;
      const project = ((result.data) as GeometryData).project;
      if(project) {
        this.name = project.name;
      }
      this.updateName();
      element('scene-title').textContent =
        project?.title ?? this.name.toUpperCase();
      element('example-kind').textContent = project?.info ?? 'PROCEDURAL STUDY';
      this.inspection.update(result.data, this.shouldFrame);
      this.shouldFrame = false;
      this.parameters.update(result.data.parameters);
      this.filterParameters();
      element('vertex-count').textContent = (
        result.data.positions.length / 3
      ).toLocaleString();
      element('triangle-count').textContent = (
        result.data.triangles.length / 3
      ).toLocaleString();
      element('point-count').textContent =
        result.data.points.length.toLocaleString();
      element('build-time').textContent =
        `${result.milliseconds.toFixed(1)} ms`;
      // Numeric payload only: JS array/object overhead and GPU preview buffers are excluded.
      const data = ((result.data) as GeometryData);
      const bytes =
        (data.positions.length +
          data.normals.length +
          data.uvs.length +
          data.triangleMaterials.length +
          data.pointMaterials.length +
          data.colors.length +
          data.alphas.length +
          data.triangles.length +
          data.points.length) *
        Float64Array.BYTES_PER_ELEMENT;
      element('data-size').textContent = `${(bytes / 1024).toFixed(1)} KiB`;
      this.setPreviewPending(false);
      button('export').disabled = false;
      this.status(
        `Built successfully · ${(result.data.positions.length / 3).toLocaleString()} vertices`,
      );
      this.updateExportNote(false);
      this.persist();
    };
    worker.onerror = (event) => {
      if(this.worker !== worker) {
        return;
      }
      this.cancelCompilation();
      this.previewFailed();
      this.status(
        `Compiler could not start: ${event.message}. Previous preview retained.`,
        'error',
      );
    };
    worker.postMessage({
      source: this.editor.state.doc.toString(),
      overrides: this.overrides,
      materials: this.materials,
    });
  }
  filterParameters() {
    const query = input('parameter-search').value.trim().toLowerCase();
    let matches = 0;
    for(const group of element('parameters').children) {
      if(group instanceof HTMLElement) {
        const match = !query || (group.querySelector('h3')?.textContent ?? '').toLowerCase().includes(query.replaceAll('_', ' '));
        group.hidden = !match;
        matches+= Number(match);
      }
    }
    element('parameter-empty').hidden = !query || matches > 0;
  }
  cancelCompilation() {
    clearTimeout(this.debounce);
    clearTimeout(this.timeout);
    this.worker?.terminate();
    this.worker = null;
  }
  connectControls() {
    const signal = this.listeners.signal;
    /** @param id @param handler */
    const click = (id: string, handler: () => void) =>
      element(id).addEventListener(BROWSER_EVENT.CLICK, handler, { signal });
    input('parameter-search').addEventListener(BROWSER_EVENT.INPUT, () => this.filterParameters(), {signal});
    click('error-location', () => {
      const line = Math.max(1, Math.min(this.editor.state.doc.lines, Number(button('error-location').dataset.line) || 1));
      const sourceLine = this.editor.state.doc.line(line);
      const column = Math.max(0, (Number(button('error-location').dataset.column) || 1) - 1);
      this.editor.dispatch({selection: {anchor: Math.min(sourceLine.to, sourceLine.from + column)}, scrollIntoView: true});
      this.editor.focus();
    });
    click('run', () => this.compile());
    click('frame', () => this.viewport.frame());
    click('reset-camera', () => this.viewport.resetCamera());
    click('reset', () => {
      this.overrides = Object.create(null);
      this.hasEdits = true;
      this.compile();
    });
    click('shaded', () => this.setMode(FORM_VIEW_MODE.SHADED));
    click('points-mode', () => this.setMode(FORM_VIEW_MODE.POINTS));
    input('grid').addEventListener(
      BROWSER_EVENT.CHANGE,
      () => {
        this.viewport.showGrid = input('grid').checked;
        this.viewport.invalidate();
      },
      { signal },
    );
    input('project-info').addEventListener(
      BROWSER_EVENT.CHANGE,
      () => {
        const title = element('scene-title').parentElement;
        if(title) {
          title.hidden = !input('project-info').checked;
        }
      },
      { signal },
    );
    input('point-size').addEventListener(
      BROWSER_EVENT.INPUT,
      () => {
        this.viewport.pointSize = Number(input('point-size').value);
        updateRangeFill(input('point-size'));
        this.viewport.invalidate();
      },
      { signal },
    );
    input('live').addEventListener(
      BROWSER_EVENT.CHANGE,
      () => {
        if(input('live').checked) {
          this.compile();
        } else {
          this.cancelCompilation();
          this.status('Live preview paused. Run to update.', 'busy');
        }
      },
      { signal },
    );
    input('document-name').addEventListener(
      BROWSER_EVENT.CHANGE,
      () => {
        const nextName = input('document-name').value.trim();
        input('document-name').value = nextName || this.name;
        if(!nextName || nextName === this.name) {
          return;
        }
        try {
          const changes = projectNameChange(
            this.editor.state.doc.toString(),
            nextName,
          );
          this.name = nextName;
          this.updateName();
          this.editor.dispatch({ changes });
        } catch (error) {
          input('document-name').value = this.name;
          this.status(
            `Fix the script before renaming its project metadata: ${error instanceof Error ? error.message : String(error)}`,
            'error',
          );
        }
      },
      { signal },
    );
    const select = ((element('examples')) as HTMLSelectElement);
    EXAMPLES.forEach((example, index) => {
      const option = document.createElement('option');
      option.value = String(index);
      option.textContent = example.name;
      select.append(option);
    });
    select.addEventListener(
      BROWSER_EVENT.CHANGE,
      () => {
        if(select.value === '') {
          return;
        }
        const index = Number(select.value),
          example = EXAMPLES[index];
        if(
          this.hasEdits &&
          !confirm(
            'Replace this study with an example? Save project first if you want to keep your edits.',
          )
        ) {
          select.value = '';
          return;
        }
        this.load({
          version: 1,
          name: example.name,
          source: example.source,
          overrides: {},
        });
        select.value = '';
      },
      { signal },
    );
    click('save', () => {
      try {
        this.numeric.capture();
        const text = serializeDocument(this.document());
        download(`${fileStem(this.name)}.formlab`, text, 'application/json');
        this.hasEdits = false;
      } catch (error) {
        this.status(`Save failed: ${error instanceof Error ? error.message : String(error)}`, 'error');
      }
    });
    click('open', () => input('file-input').click());
    input('file-input').addEventListener(
      BROWSER_EVENT.CHANGE,
      async () => {
        const file = input('file-input').files?.[0];
        if(!file) {
          return;
        }
        try {
          if(file.size > DOCUMENT_LIMITS.fileBytes) {
            throw new Error('Files must be 64 MB or smaller.');
          }
          const text = await file.text();
          if(this.disposed) {
            return;
          }
          const doc = /\.(form|txt)$/i.test(file.name)
            ? {
                version:  ((1) as 1),
                name: file.name.replace(/\.[^.]+$/, ''),
                source: text,
                overrides: {},
              }
            : readDocument(text);
          if(doc.source.length > 200000) {
            throw new Error('Script size limit is 200,000 characters.');
          }
          if(
            this.hasEdits &&
            !confirm(
              'Replace this study with the opened file? Save project first to keep your edits.',
            )
          ) {
            return;
          }
          this.load(doc);
        } catch (error) {
          this.status(
            error instanceof Error ? error.message : String(error),
            'error',
          );
        } finally {
          input('file-input').value = '';
        }
      },
      { signal },
    );
    click('export', () => this.export());
    const dialog = ((
      element('reference-dialog')) as HTMLDialogElement
    );
    connectScriptGuide(element('procedural-recipes'), this.editor, dialog, signal);
    click('help', () => dialog.showModal());
    click('recipes', () => {
      dialog.showModal();
      element('procedural-recipes').scrollIntoView();
    });
    click('reference-link', () => dialog.showModal());
    click('close-help', () => dialog.close());
    window.addEventListener(
      BROWSER_EVENT.KEYDOWN,
      (event) => {
        if(
          (event.ctrlKey || event.metaKey) &&
          event.key.toLowerCase() === 's'
        ) {
          event.preventDefault();
          button('save').click();
        }
      },
      { signal },
    );
  }
  /** @param mode */
  setMode(mode: string) {
    this.viewport.mode = mode;
    for(const [id, value] of [
      ['shaded', FORM_VIEW_MODE.SHADED],
      ['points-mode', FORM_VIEW_MODE.POINTS],
    ]) {
      button(id).classList.toggle('selected', mode === value);
      button(id).setAttribute('aria-pressed', String(mode === value));
    }
    this.viewport.invalidate();
    if(
      mode === FORM_VIEW_MODE.POINTS &&
      this.data &&
      this.data.positions.length / 3 > 40000
    ) {
      element('footer-status').textContent =
        'POINT PREVIEW SAMPLED · EXPORT CONTAINS ALL DATA';
    } else {
      element('footer-status').textContent =
        'LOCAL WORKSPACE · NO CLOUD REQUIRED';
    }
  }
  /** @param doc */
  load(doc: FormDocument) {
    this.numeric.clear();
    this.cancelCompilation();
    this.parameters.clear();
    input('parameter-search').value = '';
    element('parameter-empty').hidden = true;
    this.inspection.reset();
    this.name = doc.name;
    this.variants.setVariants(doc.variants ?? []);
    this.materials = readMaterials(doc.materials);
    this.materialControls.setMaterials(this.materials);
    this.overrides = Object.assign(Object.create(null), doc.overrides);
    this.shouldFrame = true;
    this.replacing = true;
    this.editor.dispatch({
      changes: {
        from: 0,
        to: this.editor.state.doc.length,
        insert: doc.source,
      },
      selection: { anchor: 0 },
    });
    this.replacing = false;
    this.hasEdits = false;
    this.updateName();
    this.persist();
    this.compile();
  }
  updateName() {
    input('document-name').value = this.name;
    element('editor-filename').textContent = `${fileStem(this.name)}.form`;
  }

  document(): FormDocument {
    return {
      version: 1,
      name: this.name,
      source: this.editor.state.doc.toString(),
      overrides: this.overrides,
      materials: this.materials,
      variants: this.variants.variants
    };
  }
  persist() {
    if(hosted) {
      return;
    }
    try {
      localStorage.setItem('joy-form-lab-v1', JSON.stringify(this.document()));
      element('save-state').hidden = true;
    } catch {
      element('save-state').hidden = false;
      element('save-state').textContent =
        'LOCAL SAVE UNAVAILABLE · USE SAVE PROJECT';
    }
  }
  restore() {
    if(hosted) {
      return null;
    }
    try {
      const value = localStorage.getItem('joy-form-lab-v1');
      return value ? readDocument(value) : null;
    } catch {
      return null;
    }
  }
  export() {
    if(!this.data) {
      return;
    }
    const format = ((element('export-format')) as HTMLSelectElement)
      .value;
    if(format === 'glb') {
      try {
        download(`${fileStem(this.name)}.glb`, exportGlb(this.data), 'model/gltf-binary');
      } catch (error) {
        this.status(error instanceof Error ? error.message : String(error), 'error');
      }
      return;
    }
    const text =
      format === 'obj'
        ? exportObj(this.data)
        : format === 'ply'
          ? exportPly(this.data)
          : JSON.stringify(
              {
                format: 'form-lab-geometry',
                version: 1,
                upAxis: 'Y',
                units: 'script',
                ...this.data,
              },
              null,
              2,
            );
    download(
      `${fileStem(this.name)}.${format}`,
      text,
      format === 'json' ? 'application/json' : 'text/plain',
    );
  }
  /** Keep the badge tied to the geometry on screen, including failed and cancelled builds.
   * @param pending
   */
  setPreviewPending(pending: boolean) {
    element('preview-status').textContent = pending
      ? (this.data ? 'Preview pending · last accepted geometry' : 'Preview pending · no accepted geometry')
      : 'Preview accepted';
    element('preview-status').dataset.state = pending ? 'pending' : 'generated';
  }
  previewFailed() {
    element('preview-status').textContent = this.data ? 'Preview failed · last accepted geometry' : 'Preview failed · no accepted geometry';
    element('preview-status').dataset.state = 'failed';
  }
  /** @param stale */
  updateExportNote(stale: boolean) {
    element('export-note').textContent = stale
      ? 'Script changed. Export uses the previous successful build.'
      : 'Exports the last successful build. GLB includes materials, colors and points; OBJ omits materials.';
  }
  /** @param message @param [state] */
  status(message: string, state: string = '') {
    button('error-location').hidden = true;
    element('diagnostic').className = `diagnostic ${state}`;
    element('diagnostic-text').textContent = message;
  }
  /** @param message */
  gpuError(message: string) {
    element('gpu-error').hidden = false;
    element('gpu-error').textContent = message;
    element('backend').textContent = 'PREVIEW UNAVAILABLE';
  }
  destroy() {
    if(this.disposed) {
      return;
    }
    this.disposed = true;
    this.cancelCompilation();
    this.panelLayout?.destroy();
    this.listeners.abort();
    this.parameters.destroy();
    this.materialControls.destroy();
    this.layout.destroy();
    this.variants.destroy();
    this.inspection.destroy();
    this.viewport.destroy();
    this.editor.destroy();
  }
}
/** @param id */
function element(id: string) {
  const node = document.getElementById(id);
  if(!node) {
    throw new Error(`Missing element #${id}`);
  }
  return node;
}
/** @param id */
function input(id: string) {
  return  ((element(id)) as HTMLInputElement);
}
/** @param id */
function button(id: string) {
  return  ((element(id)) as HTMLButtonElement);
}

const app = new FormLab();
const navigation = mountEditorNavigation({
  currentTool: 'form-lab',
  openButton: button('open'),
  saveButton: button('save')
});
let workspacePath = '';
const workspaceBridge = connectWorkspaceTool({
  maximized: value => app.panelLayout.setMaximized(value),
  deferFocusedInputs: true,
  load(path: string, text: string) {
    workspacePath = path;
    const raw = path.endsWith('.form');
    document.documentElement.classList.toggle('joy-raw-form', raw);
    element('override-state').textContent = raw
      ? 'Parameter overrides are temporary. Save writes script source only; exports use the accepted preview.'
      : 'Parameter overrides and materials are saved in this FORM LAB document. Exports use the accepted preview.';
    app.load(raw ? {version: 1, name: path.split('/').at(-1) ?? 'FORM', source: text, overrides: {}} : readDocument(text));
  },
  serialize() {
    app.numeric.capture();
    return workspacePath.endsWith('.form') ? app.editor.state.doc.toString() : serializeDocument(app.document());
  }
});
void app.start();
window.addEventListener(BROWSER_EVENT.PAGEHIDE, (event) => {
  if(!event.persisted) {
    workspaceBridge.destroy();
    navigation.destroy();
    app.destroy();
  }
});
if(import.meta.hot) {
  import.meta.hot.dispose(() => {
    workspaceBridge.destroy();
    navigation.destroy();
    app.destroy();
  });
}
