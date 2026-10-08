// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { BROWSER_EVENT, FORM_MATERIAL, MATERIAL_ALPHA_MODE } from 'joy-engine/constants';
import type { Material } from 'joy-engine/form';
import { defaultMaterial, readMaterials } from 'joy-engine/form';
import { exportMaterialX, importMaterialX } from './materialx.ts';
import { download } from './document.ts';
import { DOCUMENT_LIMITS } from 'joy-engine/form';
export type { Material };

/** Owns editable document materials and its DOM subscriptions; never mutates compiled geometry. */
export class MaterialControls {
  declare host: HTMLElement;
  declare onChange: (materials: Material[]) => void;
  declare materials: Material[];
  declare selected: string;
  declare revision: number;
  declare disposed: boolean;
  declare listeners: AbortController;
  declare select: HTMLSelectElement;
  declare fields: HTMLDivElement;
  declare message: HTMLParagraphElement;
  declare deleteButton: HTMLButtonElement;

  /** @param host @param onChange */
  constructor(host: HTMLElement, onChange: (materials: Material[]) => void) {
    this.host = host;
    this.onChange = onChange;
    this.materials = readMaterials(undefined);
    this.selected = FORM_MATERIAL.DEFAULT;
    this.revision = 0;
    this.disposed = false;
    this.listeners = new AbortController();
    const signal = this.listeners.signal;
    this.select = document.createElement('select');
    this.select.id = 'material-select';
    this.select.setAttribute('aria-label', 'Material');
    this.fields = document.createElement('div');
    this.fields.className = 'material-fields';
    this.message = document.createElement('p');
    this.message.id = 'material-message';
    this.message.setAttribute('role', 'status');
    const open = button('Import .mtlx');
    const save = button('Export .mtlx', 'material-export');
    const add = button('Add Material', 'material-add');
    this.deleteButton = button('Delete Material', 'material-delete');
    const file = document.createElement('input');
    file.id = 'material-file';
    file.type = 'file';
    file.accept = '.mtlx,.xml';
    file.hidden = true;
    const actions = document.createElement('div');
    actions.className = 'material-actions material-actions--files';
    actions.append(open, save, file);
    const materialActions = document.createElement('div');
    materialActions.className = 'material-actions';
    materialActions.append(add, this.deleteButton);
    host.append(this.select, this.fields, materialActions, actions, this.message);
    this.select.addEventListener(BROWSER_EVENT.CHANGE, () => {
      this.selected = this.select.value;
      this.renderFields();
    }, { signal });
    this.deleteButton.addEventListener(BROWSER_EVENT.CLICK, () => this.deleteSelected(), { signal });
    add.addEventListener(BROWSER_EVENT.CLICK, () => {
      const entered = window.prompt('Material name (letters, numbers, and underscores)', 'NewMaterial');
      if(entered === null) {
        return;
      }
      this.attempt(() => {
        const material = { ...defaultMaterial(), name: entered.trim() };
        const next = readMaterials([...this.materials, material]);
        this.selected = material.name;
        this.setMaterials(next);
        this.changed();
      });
    }, { signal });
    open.addEventListener(BROWSER_EVENT.CLICK, () => file.click(), { signal });
    save.addEventListener(BROWSER_EVENT.CLICK, () => this.attempt(() => {
      download('form-lab-materials.mtlx', exportMaterialX(this.materials), 'application/xml');
    }), { signal });
    file.addEventListener(BROWSER_EVENT.CHANGE, async () => this.importFile(file), { signal });
    this.setMaterials(this.materials);
  }

  /** @param file */
  async importFile(file: HTMLInputElement) {
    const selected = file.files?.[0];
    const revision = this.revision;
    if(!selected) {
      return;
    }
    try {
      if(selected.size > 1000000) {
        throw new Error('MaterialX files must be smaller than 1 MB.');
      }
      const imported = importMaterialX(await selected.text());
      if(this.disposed || revision !== this.revision) {
        throw new Error('The document changed while reading MaterialX. Please import again.');
      }
      const merged = new Map(this.materials.map(material => [material.name, material]));
      for(const material of imported) {
        const existing = merged.get(material.name);
        // Constant-only MaterialX updates factors without discarding document-owned maps.
        merged.set(material.name, existing ? { ...material, textures: existing.textures } : material);
      }
      this.setMaterials(readMaterials([...merged.values()]));
      this.changed();
      this.message.textContent = `Imported ${imported.length} materials. Matching names were updated; existing textures were preserved.`;
    } catch (error) {
      this.message.textContent = error instanceof Error ? error.message : String(error);
    } finally {
      file.value = '';
    }
  }

  /** @param materials */
  setMaterials(materials: Material[] | undefined) {
    this.materials = readMaterials(materials);
    this.revision++;
    if(!this.materials.some(material => material.name === this.selected)) {
      this.selected = FORM_MATERIAL.DEFAULT;
    }
    this.select.replaceChildren(...this.materials.map(material => {
      const option = document.createElement('option');
      option.value = material.name;
      option.textContent = material.name;
      return option;
    }));
    this.select.value = this.selected;
    this.message.textContent = '';
    this.renderFields();
  }

  renderFields() {
    this.fields.replaceChildren();
    this.deleteButton.disabled = this.selected === FORM_MATERIAL.DEFAULT;
    const material = this.materials.find(item => item.name === this.selected);
    if(!material) {
      return;
    }
    const name = document.createElement('input');
    name.className = 'material-name-field';
    name.value = material.name;
    name.readOnly = material.name === FORM_MATERIAL.DEFAULT;
    name.maxLength = 64;
    name.setAttribute('aria-label', 'Material name');
    name.addEventListener(BROWSER_EVENT.CHANGE, () => this.rename(material, name.value.trim()));
    this.fields.append(name);
    this.vectorProperty(material, 'baseColor', 'Base Color', ['R', 'G', 'B', 'A']);
    this.vectorProperty(material, 'emissive', 'Emissive', ['R', 'G', 'B']);
    this.scalarProperty(material, 'metallic', 'Metallic');
    this.scalarProperty(material, 'roughness', 'Roughness');
    this.textureProperty(material, 'normal', 'Normal');
    this.textureProperty(material, 'ao', 'Ambient Occlusion');
    const property = section('Alpha Mode');
    const mode = document.createElement('select');
    mode.id = 'material-alphaMode';
    for(const value of [MATERIAL_ALPHA_MODE.OPAQUE, MATERIAL_ALPHA_MODE.MASK, MATERIAL_ALPHA_MODE.BLEND]) {
      const option = document.createElement('option');
      option.value = value;
      option.textContent = value;
      mode.append(option);
    }
    mode.value = material.alphaMode;
    mode.addEventListener(BROWSER_EVENT.CHANGE, () => {
      material.alphaMode = ((mode.value) as Material['alphaMode']);
      this.changed();
    });
    property.append(mode);
    this.fields.append(property);
  }

  /** @param material @param field @param title @param channels */
  vectorProperty(material: Material, field: 'baseColor' | 'emissive', title: string, channels: string[]) {
    const property = section(title);
    const swatch = document.createElement('span');
    swatch.className = 'color-swatch material-color-swatch';
    swatch.setAttribute('role', 'img');
    swatch.setAttribute('aria-label', `${title} color preview`);
    const fill = document.createElement('i');
    swatch.append(fill);
    const updateSwatch = () => {
      const [red, green, blue, alpha = 1] = material[field];
      fill.style.backgroundColor = `rgba(${red*255}, ${green*255}, ${blue*255}, ${alpha})`;
      swatch.title = `${title} (${material[field].join(', ')})`;
    };
    updateSwatch();
    property.firstElementChild?.append(swatch);
    const texture = this.textureControl(material, field);
    property.append(texture);
    if(material.textures[field]) {
      const hint = document.createElement('span');
      hint.className = 'material-factor-hint';
      hint.textContent = 'Color factor × texture';
      property.append(hint);
    }
    const components = document.createElement('div');
    components.className = 'material-components';
    material[field].forEach((value, channel) => components.append(this.number(`${field}-${channel}`, channels[channel], value, next => {
      material[field][channel] = next;
      updateSwatch();
    })));
    property.append(components);
    this.fields.append(property);
  }

  /** @param material @param field @param title */
  scalarProperty(material: Material, field: 'metallic' | 'roughness', title: string) {
    const property = section(title);
    property.append(this.textureControl(material, field));
    const label = material.textures[field] ? 'Factor × texture' : 'Value';
    property.append(this.number(field, label, material[field], next => {
      material[field] = next;
    }));
    this.fields.append(property);
  }

  /** @param material @param field @param title */
  textureProperty(material: Material, field: 'normal' | 'ao', title: string) {
    const property = section(title);
    property.append(this.textureControl(material, field));
    this.fields.append(property);
  }

  /** @param material @param field */
  textureControl(material: Material, field: keyof Material['textures']) {
    const row = document.createElement('div');
    row.className = 'material-texture';
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = 'image/png,image/jpeg,image/webp';
    const choose = button(material.textures[field] ? 'Replace Texture' : 'Choose Texture');
    const clear = button('Clear');
    const texture = material.textures[field];
    const status = texture ? document.createElement('img') : document.createElement('span');
    if(status instanceof HTMLImageElement && texture) {
      status.src = texture;
      status.alt = `${field} texture preview`;
      status.className = 'material-texture-preview';
    } else {
      status.textContent = 'Not Set';
    }
    clear.disabled = !material.textures[field];
    choose.addEventListener(BROWSER_EVENT.CLICK, () => input.click());
    clear.addEventListener(BROWSER_EVENT.CLICK, () => {
      material.textures[field] = null;
      this.changed();
      this.renderFields();
    });
    input.addEventListener(BROWSER_EVENT.CHANGE, async () => {
      const image = input.files?.[0];
      if(!image) {
        return;
      }
      if(image.size > DOCUMENT_LIMITS.textureBytes) {
        this.message.textContent = 'Textures must be 4 MB or smaller.';
        return;
      }
      const revision = this.revision;
      try {
        const dataUrl = await readDataUrl(image);
        if(this.disposed || revision !== this.revision) {
          return;
        }
        const updated = this.materials.map(item => item === material
          ? { ...item, textures: { ...item.textures, [field]: dataUrl } }
          : item);
        // Validate the complete budget before replacing any current material data.
        this.setMaterials(readMaterials(updated));
        this.changed();
      } catch (error) {
        if(!this.disposed && revision === this.revision) {
          this.message.textContent = error instanceof Error ? error.message : String(error);
        }
      }
    });
    row.append(input, choose, clear, status);
    return row;
  }

  /** Delete a named material; scripts keep their explicit references and report missing names. */
  deleteSelected() {
    if(this.selected === FORM_MATERIAL.DEFAULT) {
      return;
    }
    const removed = this.selected;
    this.setMaterials(this.materials.filter(material => material.name !== removed));
    this.changed();
    this.message.textContent = `Deleted ${removed}. Replace material(${removed}) in the script with material() or another material name.`;
  }

  /** @param material @param next */
  rename(material: Material, next: string) {
    this.attempt(() => {
      if(material.name === FORM_MATERIAL.DEFAULT) {
        throw new Error('The Default material cannot be renamed.');
      }
      const previous = material.name;
      material.name = next;
      const checked = readMaterials(this.materials);
      this.selected = next;
      this.setMaterials(checked);
      this.changed();
      this.message.textContent = `Renamed ${previous} to ${next}. Update material(${previous}) in the script.`;
    });
  }

  /** @param id @param title @param value @param update */
  number(id: string, title: string, value: number, update: (value: number) => void) {
    const label = document.createElement('label');
    label.textContent = title;
    const control = document.createElement('input');
    control.id = `material-${id}`;
    control.type = 'number';
    control.min = '0';
    control.max = '1';
    control.step = 'any';
    control.required = true;
    control.value = String(value);
    control.addEventListener(BROWSER_EVENT.CHANGE, () => {
      if(control.reportValidity()) {
        update(control.valueAsNumber);
        this.changed();
      }
    });
    label.append(control);
    return label;
  }

  changed() {
    this.revision++;
    this.message.textContent = '';
    this.onChange(readMaterials(this.materials));
  }
  /** @param action */
  attempt(action: () => void) {
    try {
      action();
    } catch (error) {
      this.message.textContent = error instanceof Error ? error.message : String(error);
      this.renderFields();
    }
  }
  destroy() {
    this.disposed = true;
    this.listeners.abort();
    this.host.replaceChildren();
  }
}

/** @param title */
function section(title: string) {
  const node = document.createElement('section');
  node.className = 'material-property';
  const heading = document.createElement('span');
  heading.className = 'material-property-title';
  heading.textContent = title;
  node.append(heading);
  return node;
}

/** @param text @param [id] */
function button(text: string, id?: string) {
  const node = document.createElement('button');
  node.type = 'button';
  node.textContent = text;
  if(id) {
    node.id = id;
  }
  return node;
}

/** @param file */
function readDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error ?? new Error('Texture could not be read.'));
    reader.readAsDataURL(file);
  });
}

