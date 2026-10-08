// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { BROWSER_EVENT, ENTITY_TYPE, KEY_CODE, PRIMITIVE_SHAPE, TRANSFORM_PROPERTY } from 'joy-engine/constants';
import { WORKSPACE_OPERATION } from '../workspace/constants.ts';
import { LEVEL_TOOL } from './constants.ts';
import type { JsonValue, EntityDefinition, JsonRecord } from 'joy-engine';
import type { FieldMetadata } from './property-editor.ts';
import type { Parameter } from 'joy-engine/form';
import type { ObjectData } from 'joy-engine/objects';
import {PanelLayout} from '../interaction/panel-layout.ts';
import {NumericControls} from '../interaction/numeric-controls.ts';
import {guardNumberScroll} from '../interaction/number-scroll.ts';
import '../../css/level.css';
import '@fontsource/archivo/400.css';
import '@fontsource/archivo/600.css';
import '@fontsource/archivo/900.css';
import {
  LevelDocument,
  parseLevel,
  serializeLevel,
  parseObject,
  serializeObject,
  createObjectInstance,
  resolveObjectInstance
} from 'joy-engine';
import { PropertyEditor } from './property-editor.ts';
import { connectWorkspaceTool } from '../workspace/tool-bridge.ts';
import { LevelViewport } from './viewport.ts';

/**
 * Owns the authored document and UI; the workspace owns file persistence and conflicts. */
class LevelEditor {
  declare document: LevelDocument;
  declare listeners: AbortController;
  declare panelLayout: PanelLayout;
  declare disposed: boolean;
  declare componentSnapshot: string;
  declare path: string;
  declare objectMode: boolean;
  declare assetGeneration: number;
  declare assets: { path: string; kind: string; }[];
  declare objects: Map<string, ObjectData>;
  declare propertyEditor: PropertyEditor;
  declare viewport: LevelViewport;
  declare numeric: NumericControls;
  declare bridge: ReturnType<typeof connectWorkspaceTool>;

  constructor() {
    this.document = new LevelDocument({version: 1, name: 'Untitled level', entities: []});
    this.listeners = new AbortController();
    guardNumberScroll(document, this.listeners.signal);
    this.panelLayout = new PanelLayout(((document.querySelector('.level-editor')) as HTMLElement), {left:  ((document.querySelector('.outliner')) as HTMLElement), right:  ((document.querySelector('.inspector')) as HTMLElement), center:  ((document.querySelector('.viewport')) as HTMLElement), key: 'joy-editor-panels-level-v2', leftWidth: 160, rightWidth: 300});
    this.disposed = false;
    this.componentSnapshot = '';
    this.path = '';
    this.objectMode = false;
    this.assetGeneration = 0;

    this.assets = [];

    this.objects = new Map();
    this.propertyEditor = new PropertyEditor(element('component-fields'), {
      change: (path, value) => this.changeProperty(path, value),
      error: message => this.status(message),
      open: path => { void this.openSource(path); },
      assets: () => this.assets,
      read: path => {
        const entity = this.selected();
        let value = ((entity ? this.resolve(entity).components : undefined) as JsonValue | undefined);
        for(const part of path) {
          value = value && typeof value === 'object' ? (Array.isArray(value) ? value[Number(part)] : value[part]) : undefined;
        }
        return value;
      }
    });
    this.viewport = new LevelViewport(((element('viewport')) as HTMLCanvasElement), {
      select: id => this.attempt(() => {
        this.applyComponents();
        this.document.select(id);
        this.refresh();
      }),
      move: point => this.editSelected(entity => {
        entity.transform.position[0] = point[0];
        entity.transform.position[2] = point[2];
      }),
      status: message => this.status(message),
      readText: path => this.readAsset(path)
    });
    this.createTransforms();
    this.numeric = new NumericControls(element('properties'), this.listeners.signal, {commit: input => this.attempt(() => {
      if(input.closest('#component-fields')) {
        this.propertyEditor.commitInput(input);
      } else {
        this.captureInspector();
      }
    })});
    input('scene-filter').addEventListener(BROWSER_EVENT.INPUT, () => this.refreshEntities(), {signal:this.listeners.signal});
    this.bind();
    this.refresh();
    this.bridge = connectWorkspaceTool({
      deferFocusedInputs: true,
      load: (path, text) => {
        this.assetGeneration++;
        this.objects.clear();
        this.path = path;
        this.objectMode = path.endsWith('.joyobject');
        const object = this.objectMode ? parseObject(text) : null;
        this.document = new LevelDocument(object ? {version: 1, name: object.name, entities: [object.entity]} : parseLevel(text));
        if(object) {
          this.document.select(object.entity.id);
        }
        element('authoring-kind').textContent = this.objectMode ? 'OBJECT' : 'LEVEL';
        for(const id of ['add-box', 'add-sphere', 'instance-picker', 'add-instance', 'duplicate', 'delete', 'save-object']) {
          element(id).hidden = this.objectMode;
        }
        void this.refreshAssets();
        this.refresh();
        this.viewport.focus();
      },
      serialize: () => {
        // Invalid in-progress JSON must block saving instead of silently discarding source.
        this.numeric.validate();
        this.propertyEditor.capture();
        this.applyComponents();
        this.captureInspector();
        const level = this.document.snapshot();
        return this.objectMode ? serializeObject({version: 1, name: level.name, entity: level.entities[0]}) : serializeLevel(level);
      },
      assetsChanged: () => { this.assetGeneration++; this.objects.clear(); void this.refreshAssets(); }
    });
  }

  /**
   * Initialize the owned viewport and keep startup failures visible in the editor. */
  async start() {
    try {
      await this.viewport.initialize();
      element('backend').textContent = this.viewport.renderer.backend?.toUpperCase() ?? '';
    } catch(error) {
      this.status(`Viewport unavailable: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

  /**
   * Add an authored primitive at the snapped camera target, in world meters.
   * @param shape
   */
  add(shape: typeof PRIMITIVE_SHAPE.BOX | typeof PRIMITIVE_SHAPE.SPHERE) {
    this.attempt(() => {
      this.applyComponents();
      const id = crypto.randomUUID();
      const target = this.viewport.camera.target;
      const snap = this.viewport.snap;
      const position = target.map(value => snap > 0 ? Math.round(value/snap)*snap : value);
      this.document.transact(draft => {
        draft.entities.push({
          id,
          name: shape === PRIMITIVE_SHAPE.BOX ? 'Box' : 'Sphere',
          type: ENTITY_TYPE.ENTITY,
          tags: [],
          transform: {
            position: [position[0], 0.5, position[2]],
            rotation: [0, 0, 0],
            scale: [1, 1, 1]
          },
          components: {
            render: {shape, color: [0.95, 0.36, 0.12]},
            physics: shape === PRIMITIVE_SHAPE.BOX
              ? {shape, mass: 0, halfExtents: [0.5, 0.5, 0.5]}
              : {shape, mass: 0, radius: 0.5}
          }
        });
      });
      this.document.select(id);
      this.refresh();
      this.status('Primitive added. Use MOVE and click the ground to place it.');
    });
  }

  /**
   * Duplicate the selection as a separate entity and undoable document edit. */
  duplicate() {
    this.attempt(() => {
      this.applyComponents();
      const selected = this.selected();
      if(!selected) {
        return;
      }
      const copy = structuredClone(selected);
      copy.id = crypto.randomUUID();
      copy.name+= ' copy';
      copy.transform.position[0]+= this.viewport.snap || 1;
      this.document.transact(draft => {
        draft.entities.push(copy);
      });
      this.document.select(copy.id);
      this.refresh();
    });
  }

  /**
   * Commit pending component edits before removing the selected entity. */
  remove() {
    this.attempt(() => {
      this.applyComponents();
      this.document.transact(draft => {
        draft.entities = draft.entities.filter(entity => entity.id !== this.document.selectedId);
      });
      this.refresh();
    });
  }

  /**
   * Apply a mutation to the selected draft; document validation owns commit or rollback.
   * @param edit
   * @param [refreshProperties]
   */
  editSelected(edit: (entity: EntityDefinition) => void, refreshProperties: boolean = true) {
    this.attempt(() => {
      this.applyComponents();
      this.document.transact(draft => {
        const entity = draft.entities.find(item => item.id === this.document.selectedId);
        if(entity) {
          edit(entity);
        }
      });
      this.refresh(refreshProperties);
      this.status('Property updated.');
    });
  }

  /**
   * Pending component source commits only after successful whole-document validation. */
  applyComponents() {
    this.numeric.validate();
    this.propertyEditor.capture();
    const text = textarea('components').value;
    if(!this.document.selectedId || text === this.componentSnapshot) {
      return;
    }
    const components = JSON.parse(text);
    this.document.transact(draft => {
      const entity = draft.entities.find(item => item.id === this.document.selectedId);
      if(entity) {
        if(entity.template) {
          entity.overrides ??= {};
          entity.overrides.components = components;
        } else {
          entity.components = components;
        }
      }
    });
    this.componentSnapshot = text;
    void this.syncScene();
  }

  /**
   * Flush focused fields as well as blur-committed edits before workspace saves or navigation. */
  captureInspector() {
    const changed = this.document.transact(draft => {
      draft.name = input('level-name').value;
      const entity = draft.entities.find(item => item.id === this.document.selectedId);
      if(!entity) {
        return;
      }
      entity.name = input('entity-name').value;
      const type = input('entity-type').value;
      const tags = input('entity-tags').value.split(',').map(value => value.trim()).filter(Boolean);
      if(entity.template) {
        const resolved = this.resolve(entity);
        entity.overrides ??= {};
        if(type !== resolved.type) {
          entity.overrides.type = type;
        }
        if(JSON.stringify(tags) !== JSON.stringify(resolved.tags)) {
          entity.overrides.tags = tags;
        }
      } else { entity.type = type; entity.tags = tags; }
      for(const field of  (([TRANSFORM_PROPERTY.POSITION, TRANSFORM_PROPERTY.ROTATION, TRANSFORM_PROPERTY.SCALE]) as const)) {
        for(let axis = 0; axis < 3; axis++) {
          const value = input(`${field}-${axis}`).valueAsNumber*(field === TRANSFORM_PROPERTY.ROTATION ? Math.PI/180 : 1);
          // Display rounding must not change untouched authored precision.
          const displayed = Math.round(entity.transform[field][axis]*(field === TRANSFORM_PROPERTY.ROTATION ? 180/Math.PI : 1)*10000)/10000;
          if(input(`${field}-${axis}`).valueAsNumber !== displayed) {
            entity.transform[field][axis] = value;
          }
        }
      }
    });
    if(changed) {
      void this.syncScene();
      button('undo').disabled = !this.document.canUndo;
      button('redo').disabled = !this.document.canRedo;
    }
  }

  /**
   * Return a detached selection snapshot; edits must go through a transaction. */
  selected() {
    return this.document.snapshot().entities.find(entity => entity.id === this.document.selectedId);
  }

  /**
   * Filter existing scene rows without touching document or inspector state. */
  refreshEntities() {
    const level = this.document.snapshot();
    const query = input('scene-filter').value.trim().toLocaleLowerCase();
    const entities = level.entities.filter(entity => {
      const resolved = this.resolve(entity);
      return [resolved.name, resolved.type, ...resolved.tags].join(' ').toLocaleLowerCase().includes(query);
    });
    element('entity-count').textContent = query ? `${entities.length} / ${level.entities.length}` : `${level.entities.length} ENTITIES`;
    const rows = [];
    for(const entity of entities) {
      const row = document.createElement('button');
      row.className = `entity-row${entity.id === this.document.selectedId ? ' selected' : ''}`;
      row.setAttribute('role', 'option');
      row.setAttribute('aria-selected', String(entity.id === this.document.selectedId));
      row.dataset.id = entity.id;
      row.textContent = entity.name;
      const type = document.createElement('small');
      type.textContent = entity.type;
      row.append(type);
      row.addEventListener(BROWSER_EVENT.CLICK, () => this.attempt(() => {
        this.numeric.validate();
        this.applyComponents();
        this.document.select(entity.id);
        this.refresh();
      }));
      rows.push(row);
    }
    element('entities').replaceChildren(...rows);
  }

  /** Rebuild accepted document properties, retaining the view-only filter. */
  refresh(refreshProperties: boolean = true) {
    const level = this.document.snapshot();
    input('level-name').value = level.name;
    this.refreshEntities();
    const authored = this.selected();
    const selected = authored ? this.resolve(authored) : null;
    element('properties').hidden = !selected;
    element('no-selection').hidden = Boolean(selected);
    button('duplicate').disabled = !selected;
    button('delete').disabled = !selected;
    button('undo').disabled = !this.document.canUndo;
    button('redo').disabled = !this.document.canRedo;
    if(selected) {
      input('entity-name').value = selected.name;
      input('entity-type').value = selected.type;
      input('entity-tags').value = selected.tags.join(', ');
      for(const field of  (([TRANSFORM_PROPERTY.POSITION, TRANSFORM_PROPERTY.ROTATION, TRANSFORM_PROPERTY.SCALE]) as const)) {
        for(let axis = 0; axis < 3; axis++) {
          const value = selected.transform[field][axis]*(field === TRANSFORM_PROPERTY.ROTATION ? 180/Math.PI : 1);
          input(`${field}-${axis}`).value = String(Math.round(value*10000)/10000);
        }
      }
      this.componentSnapshot = JSON.stringify(selected.components, null, 2);
      textarea('components').value = this.componentSnapshot;
      if(refreshProperties) {
        this.renderProperties(selected);
      }
      element('instance-info').hidden = !authored?.template;
      element('instance-source').textContent = authored?.template ?? '';
      element('instance-overrides').textContent = authored?.template ? `${Object.keys(authored.overrides ?? {}).length} override groups` : '';
    }
    void this.syncScene();
  }

  /**
   * Resolve current linked values for the inspector without modifying authored overrides.
   * @param entity
   */
  resolve(entity: EntityDefinition) {
    const source = entity.template ? this.objects.get(entity.template) : null;
    return source ? resolveObjectInstance(entity, source) : entity;
  }

  /**
   * Show compiler-declared defaults without silently storing them as instance overrides.
   * @param entity
   */
  renderProperties(entity: EntityDefinition) {
    const components = structuredClone(entity.components);
    const scene = this.viewport.levelScene;

    const metadata: Record<string, FieldMetadata> = {};
    const mesh = scene.meshes.get(entity.id);
    const formParameters = ((mesh?.geometry.metadata?.parameters ?? []) as Parameter[]);
    const effectParameters = scene.effects.get(entity.id)?.effect.definition.asset.parameters ?? {};
    for(const [component, parameters] of  (([['render', formParameters], ['effect', Object.entries(effectParameters).map(([name, value]) => ({name, ...value}))]]) as const)) {
      const target = components[component];
      if(!target || !target.asset || !parameters.length) {
        continue;
      }
      const values = ((target.parameters ?? {}) as JsonRecord);
      target.parameters = values;
      for(const parameter of parameters) {
        if(!('min' in parameter) || !('max' in parameter)) {
          continue;
        }
        if(!Object.hasOwn(values, parameter.name)) {
          values[parameter.name] = structuredClone(parameter.value);
        }
        metadata[`${component}.parameters.${parameter.name}`] = {min: typeof parameter.min === 'number' ? parameter.min : undefined, max: typeof parameter.max === 'number' ? parameter.max : undefined};
      }
    }
    this.propertyEditor.actions.metadata = metadata;
    this.propertyEditor.render(components);
  }

  /**
   * Read committed project content through the owning workspace. @param path */
  async readAsset(path: string) {
    const generation = this.assetGeneration;
    const text = ((await this.bridge.request(WORKSPACE_OPERATION.READ, {path})) as string);
    if(generation === this.assetGeneration && !this.disposed && path.endsWith('.joyobject')) {
      this.objects.set(path, parseObject(text));
    }
    return text;
  }

  /**
   * Refresh catalog and linked content while retaining document history. */
  async refreshAssets() {
    try {
      this.assets = await this.bridge.request(WORKSPACE_OPERATION.CATALOG);
      const picker = ((element('instance-picker')) as HTMLSelectElement);
      const selected = picker.value;
      picker.replaceChildren();
      for(const asset of this.assets.filter(asset => asset.kind === 'joyobject')) {
        const option = document.createElement('option');
        option.value = asset.path;
        option.textContent = asset.path.replace('assets/', '');
        picker.append(option);
      }
      if([...picker.options].some(option => option.value === selected)) {
        picker.value = selected;
      }
      button('add-instance').disabled = !picker.options.length;
      await this.syncScene();
      const entity = this.selected();
      if(entity && !this.propertyEditor.pending.size && !element('component-fields').contains(document.activeElement)) {
        this.renderProperties(this.resolve(entity));
      }
    } catch(error) { this.status(error instanceof Error ? error.message : String(error)); }
  }

  /**
   * Stage visual updates; failures retain the previous valid scene. */
  async syncScene() {
    const generation = ++this.assetGeneration;
    try {
      await this.viewport.sync(this.document.snapshot().entities, this.document.selectedId);
      if(generation !== this.assetGeneration || this.disposed) {
        return;
      }
      const entity = this.selected();
      if(entity && !this.propertyEditor.pending.size && !element('component-fields').contains(document.activeElement) && textarea('components').value === this.componentSnapshot) {
        this.renderProperties(this.resolve(entity));
        const selected = this.resolve(entity);
        input('entity-type').value = selected.type;
        input('entity-tags').value = selected.tags.join(', ');
        this.componentSnapshot = JSON.stringify(selected.components, null, 2);
        textarea('components').value = this.componentSnapshot;
      }
    } catch(error) { if(generation === this.assetGeneration) { this.status(`Content: ${error instanceof Error ? error.message : String(error)}`); } }
  }

  /**
   * Commit one nested property edit, preserving linked source defaults elsewhere.
   * @param path @param value
   */
  changeProperty(path: string[], value: JsonValue | undefined) {
    const id = this.document.selectedId;
    const previous = this.selected();
    let oldValue = ((previous ? this.resolve(previous).components : null) as JsonValue);
    for(const part of path) {
      oldValue = oldValue && typeof oldValue === 'object' ? (Array.isArray(oldValue) ? oldValue[Number(part)] : oldValue[part]) : null;
    }
    this.document.transact(level => {
      const entity = level.entities.find(item => item.id === id);
      if(!entity) {
        return;
      }
      if(entity.template) {
        entity.overrides ??= {};
        entity.overrides.components ??= {};
        if(value === undefined && path.length > 1) {
          throw new Error('Edit the object source or detach this instance to remove an inherited property.');
        }
        // Arrays override as units. Nested numeric indices must not become record keys.
        const resolved = this.resolve(entity).components;
        let current = ((resolved) as JsonValue);
        let arrayIndex = -1;
        for(let index = 0; index < path.length; index++) {
          if(Array.isArray(current)) {
            arrayIndex = index; break;
          }
          current = current && typeof current === 'object' ? current[path[index]] : null;
        }
        if(arrayIndex >= 0 && Array.isArray(current)) {
          const copy = structuredClone(current);
          setPath(((((copy) as unknown)) as JsonRecord), path.slice(arrayIndex), value);
          setPath(((entity.overrides.components) as JsonRecord), path.slice(0, arrayIndex), copy);
        } else {
          setPath(((entity.overrides.components) as JsonRecord), path, value === undefined ? null : value);
        }
      } else {
        setPath(entity.components, path, value);
      }
    });
    const structural = value === undefined || (value !== null && typeof value === 'object' && (!Array.isArray(value) || !Array.isArray(oldValue) || value.length !== oldValue.length));
    this.refresh(structural);
    this.status('Property updated.');
  }

  /**
   * Add a useful component record using typed controls. */
  addComponent() {
    this.applyComponents();
    const kind = ((element('component-kind')) as HTMLSelectElement).value;
    const mesh = this.assets.find(asset => ['form', 'formlab'].includes(asset.kind));
    const effect = this.assets.find(asset => asset.kind === 'joyfx');
    const name = kind === 'custom' ? prompt('Component name:', 'gameplay') : kind === 'mesh' ? 'render' : kind;
    if(!name) {
      return;
    }
    const selected = this.selected();
    if(selected && Object.hasOwn(this.resolve(selected).components, name)) {
      this.status(`This object already has a ${name} component. Remove it first to replace it.`); return;
    }

    const value: JsonRecord = kind === 'mesh' ? {asset: mesh?.path ?? '', parameters: {}} :
      kind === 'render' ? {shape: PRIMITIVE_SHAPE.BOX, size: [1, 1, 1], color: [0.93, 0.42, 0.22]} :
      kind === 'effect' ? {asset: effect?.path ?? '', enabled: true, loop: true, seed: 7777, parameters: {}} :
      kind === 'physics' ? {shape: PRIMITIVE_SHAPE.BOX, mass: 0, halfExtents: [0.5, 0.5, 0.5]} : {};
    this.changeProperty([name], value);
  }

  /**
   * Save a reusable definition and link the selected instance without moving it. */
  async saveObject() {
    try {
      this.propertyEditor.capture();
      this.applyComponents();
      this.captureInspector();
      const owner = this.document;
      const entity = this.selected();
      if(!entity) {
        return;
      }
      const filename = prompt('Reusable object path beneath assets/:', `${entity.name.toLowerCase().replace(/[^a-z0-9]+/g, '-')}.joyobject`);
      if(!filename) {
        return;
      }
      const path = filename.startsWith('assets/') ? filename : `assets/${filename}`;
      const definition = structuredClone(this.resolve(entity));
      definition.id = 'object';
      definition.transform.position = [0, 0, 0];
      delete definition.template; delete definition.overrides;
      const object = {version:  ((1) as 1), name: entity.name, entity: definition};
      await this.bridge.request(WORKSPACE_OPERATION.CREATE, {path, text: serializeObject(object)});
      if(this.disposed || this.document !== owner) {
        return;
      }
      this.objects.set(path, object);
      this.document.transact(level => {
        const current = level.entities.find(item => item.id === entity.id);
        if(current) { current.template = path; current.overrides = {}; current.components = {}; }
      });
      this.refresh();
      await this.refreshAssets();
      this.status(`Created ${path}. This instance now follows its source.`);
    } catch(error) { this.status(error instanceof Error ? error.message : String(error)); }
  }

  /**
   * Add a linked instance at the viewport target. */
  async addInstance() {
    try {
      this.applyComponents();
      const path = ((element('instance-picker')) as HTMLSelectElement).value;
      if(!path) {
        return;
      }
      const owner = this.document;
      const object = parseObject(await this.readAsset(path));
      if(this.disposed || this.document !== owner) {
        return;
      }
      const entity = createObjectInstance(object, path, crypto.randomUUID());
      entity.transform.position = (([...this.viewport.camera.target]) as [
    number,
    number,
    number
]);
      this.document.transact(level => { level.entities.push(entity); });
      this.document.select(entity.id);
      this.refresh();
    } catch(error) { this.status(error instanceof Error ? error.message : String(error)); }
  }

  /**
   * Open a referenced asset while the workspace keeps this document alive. @param path */
  async openSource(path: string) {
    try { await this.bridge.request(WORKSPACE_OPERATION.OPEN, {path}); }
    catch(error) { this.status(error instanceof Error ? error.message : String(error)); }
  }

  /** @param message */
  status(message: string) {
    element('status').textContent = message;
  }
  /**
   * Surface validation errors without replacing in-progress inspector input.
   * @param action
   */
  attempt(action: () => void) {
    try {
      action();
    } catch(error) {
      this.status(error instanceof Error ? error.message : String(error));
    }
  }

  /**
   * Release the workspace bridge, input listeners and viewport once. */
  destroy() {
    if(this.disposed) {
      return;
    }
    this.disposed = true;
    this.bridge.destroy();
    this.panelLayout?.destroy();
    this.listeners.abort();
    this.viewport.destroy();
  }

  /**
   * Build controls in meters, degrees and scale; stored rotations remain radians. @private */
  private createTransforms() {
    for(const field of  (([TRANSFORM_PROPERTY.POSITION, TRANSFORM_PROPERTY.ROTATION, TRANSFORM_PROPERTY.SCALE]) as const)) {
      const row = document.createElement('div');
      row.className = 'transform-row';
      const heading = document.createElement('legend');
      heading.textContent = field === TRANSFORM_PROPERTY.ROTATION ? 'ROTATION · DEGREES' : field.toUpperCase();
      row.append(heading);
      for(let axis = 0; axis < 3; axis++) {
        const label = document.createElement('label');
        label.textContent = ['X', 'Y', 'Z'][axis];
        const value = document.createElement('input');
        value.id = `${field}-${axis}`;
        value.type = 'number';
        value.step = field === TRANSFORM_PROPERTY.ROTATION ? '5' : '0.25';
        value.setAttribute('aria-label', `${field} ${label.textContent}`);
        value.addEventListener(BROWSER_EVENT.CHANGE, () => this.editSelected(entity => {
          entity.transform[field][axis] = value.valueAsNumber*(field === TRANSFORM_PROPERTY.ROTATION ? Math.PI/180 : 1);
        }), {signal: this.listeners.signal});
        label.append(value);
        row.append(label);
      }
      element('transforms').append(row);
    }
  }

  /** @private */
  private bind() {
    const signal = this.listeners.signal;
    /** @param id @param action */
    const click = (id: string, action: () => void) => element(id).addEventListener(BROWSER_EVENT.CLICK, action, {signal});
    click('add-component', () => this.attempt(() => this.addComponent()));
    click('save-object', () => { void this.saveObject(); });
    click('add-instance', () => { void this.addInstance(); });
    click('edit-object', () => { const entity = this.selected(); if(entity?.template) {
      void this.openSource(entity.template);
    } });
    click('reset-overrides', () => this.editSelected(entity => { entity.overrides = {}; }));
    click('detach-object', () => this.editSelected(entity => {
      const resolved = this.resolve(entity);
      delete entity.template; delete entity.overrides;
      entity.type = resolved.type; entity.tags = resolved.tags; entity.components = resolved.components;
    }));
    click('add-box', () => this.add(PRIMITIVE_SHAPE.BOX));
    click('add-sphere', () => this.add(PRIMITIVE_SHAPE.SPHERE));
    click('duplicate', () => this.duplicate());
    click('delete', () => this.remove());
    click('undo', () => this.history(false));
    click('redo', () => this.history(true));
    click('focus', () => this.viewport.focus());
    click('reset-camera', () => this.viewport.resetCamera());
    click('apply-components', () => this.attempt(() => {
      this.applyComponents();
      this.refresh();
      this.status('Components applied.');
    }));
    for(const mode of [LEVEL_TOOL.SELECT, LEVEL_TOOL.MOVE]) {
      click(`${mode}-mode`, () => {
        this.viewport.mode = mode;
        button('select-mode').classList.toggle('active', mode === LEVEL_TOOL.SELECT);
        button('move-mode').classList.toggle('active', mode === LEVEL_TOOL.MOVE);
        this.status(mode === LEVEL_TOOL.MOVE ? 'Click the ground to move the selected entity. Height is preserved.' : 'Click an entity to select it. Drag to orbit.');
      });
    }
    input('grid-toggle').addEventListener(BROWSER_EVENT.CHANGE, () => {
      this.viewport.showGrid = input('grid-toggle').checked;
    }, {signal});
    input('snap').addEventListener(BROWSER_EVENT.CHANGE, () => {
      const value = input('snap').valueAsNumber;
      this.viewport.snap = Number.isFinite(value) ? Math.max(0, value) : 1;
      input('snap').value = String(this.viewport.snap);
    }, {signal});
    input('level-name').addEventListener(BROWSER_EVENT.CHANGE, () => this.attempt(() => {
      this.applyComponents();
      this.document.transact(draft => {
        draft.name = input('level-name').value;
      });
      this.refresh();
    }), {signal});
    input('entity-name').addEventListener(BROWSER_EVENT.CHANGE, () => this.editSelected(entity => {
      entity.name = input('entity-name').value;
    }), {signal});
    input('entity-type').addEventListener(BROWSER_EVENT.CHANGE, () => this.editSelected(entity => {
      if(entity.template) { entity.overrides ??= {}; entity.overrides.type = input('entity-type').value; }
      else { entity.type = input('entity-type').value; }
    }), {signal});
    input('entity-tags').addEventListener(BROWSER_EVENT.CHANGE, () => this.editSelected(entity => {
      const tags = input('entity-tags').value.split(',').map(value => value.trim()).filter(Boolean);
      if(entity.template) { entity.overrides ??= {}; entity.overrides.tags = tags; } else { entity.tags = tags; }
    }), {signal});
    window.addEventListener(BROWSER_EVENT.BEFOREUNLOAD, event => {
      if(this.propertyEditor.pending.size || (this.document.selectedId && textarea('components').value !== this.componentSnapshot)) {
        event.preventDefault();
      }
    }, {signal});
    window.addEventListener(BROWSER_EVENT.KEYDOWN, event => {
      const editing = event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement || event.target instanceof HTMLSelectElement;
      if((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'z' && !editing) {
        event.preventDefault();
        this.history(event.shiftKey);

      } else if(!editing && (event.key === KEY_CODE.DELETE || event.key === KEY_CODE.BACKSPACE)) {
        event.preventDefault();
        this.remove();
      }
    }, {signal});
  }

  /** @private @param redo */
  private history(redo: boolean) {
    this.attempt(() => {
      this.applyComponents();
      if(redo) {
        this.document.redo();
      } else {
        this.document.undo();
      }
      this.refresh();
    });
  }
}

/**
 * Set or remove an own nested property without invoking prototype setters.
 * @param root @param path @param value
 */
function setPath(root: JsonRecord, path: string[], value: JsonValue | undefined) {
  let target = root;
  for(const part of path.slice(0, -1)) {
    if(!Object.hasOwn(target, part) || target[part] === null || typeof target[part] !== 'object') {
      Object.defineProperty(target, part, {value: {}, enumerable: true, writable: true, configurable: true});
    }
    target = ((target[part]) as JsonRecord);
  }
  const key = path.at(-1);
  if(key === undefined) {
    return;
  }
  if(value === undefined) {
    if(Array.isArray(target)) {
      target.splice(Number(key), 1);
    } else { delete target[key]; }
  } else {
    Object.defineProperty(target, key, {value: structuredClone(value), enumerable: true, writable: true, configurable: true});
  }
}

/** @param id */
function element(id: string) {
  const node = document.getElementById(id);
  if(!node) {
    throw new Error(`Missing level editor element ${id}.`);
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
/** @param id */
function textarea(id: string) {
  return  ((element(id)) as HTMLTextAreaElement);
}

const editor = new LevelEditor();
void editor.start();
window.addEventListener(BROWSER_EVENT.PAGEHIDE, event => {
  if(!event.persisted) {
    editor.destroy();
  }
});
if(import.meta.hot) {
  import.meta.hot.dispose(() => editor.destroy());
}
