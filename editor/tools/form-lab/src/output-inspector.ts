// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { BROWSER_EVENT, FORM_ATTRIBUTE_DOMAIN, FORM_VALUE_KIND } from 'joy-engine/constants';
import type { GeometryData, GeometryValue, AttributeDomain as Domain } from 'joy-engine/form';
import type { Viewport } from './rendering/viewport.ts';

export type { GeometryData };
export type { GeometryValue };
export type { Domain };
const PAGE_SIZE = 48;

/** Owns result selection and a bounded, read-only attribute table. The script owns edits. */
export class OutputInspector {
  declare viewport: Viewport;
  declare onAccepted: () => void;
  declare revision: number;
  declare acceptedData: GeometryData | null;
  declare acceptedName: string;
  declare panel: HTMLElement;
  declare listeners: AbortController;
  declare data: GeometryData | null;
  declare geometry: GeometryValue | null;
  declare offset: number;
  declare selected: number;
  declare locations: number[][];
  declare outputs: HTMLSelectElement;
  declare domains: HTMLSelectElement;
  declare views: HTMLSelectElement;
  declare table: HTMLElement;

  /** @param toolbar @param panel @param viewport @param onAccepted */
  constructor(toolbar: HTMLElement, panel: HTMLElement, viewport: Viewport, onAccepted: () => void) {
    this.viewport = viewport;
    this.onAccepted = onAccepted;
    this.revision = 0;

    this.acceptedData = null;
    this.acceptedName = '';
    this.panel = panel;
    this.listeners = new AbortController();

    this.data = null;

    this.geometry = null;
    this.offset = 0;
    this.selected = -1;

    this.locations = [];
    toolbar.innerHTML = `<label>RESULT <select id="output-select" aria-label="Preview result"><option value="">All outputs</option></select></label><span id="output-kind">Generated geometry</span>`;
    panel.innerHTML = `<summary>ATTRIBUTES <span id="attribute-summary">Build geometry to inspect</span></summary>
      <div class="attribute-controls"><select id="attribute-domain" aria-label="Attribute domain"></select>
      <label>VIEW <select id="attribute-view" aria-label="Visualize attribute"><option value="">Material</option></select></label>
      <button id="attribute-previous" aria-label="Previous attribute page">←</button><button id="attribute-next" aria-label="Next attribute page">→</button></div>
      <div class="attribute-readout"><span id="selection-info" role="status">Select a row or click a point in the viewport</span><span id="attribute-range"></span></div>
      <div class="attribute-scroll"><table id="attribute-table" aria-label="Generated attributes"></table></div>`;
    this.outputs = select(toolbar, '#output-select');
    this.domains = select(panel, '#attribute-domain');
    this.views = select(panel, '#attribute-view');
    this.table = required(panel, '#attribute-table');
    const signal = this.listeners.signal;
    this.outputs.addEventListener(BROWSER_EVENT.CHANGE, () => this.showResult(true), {signal});
    this.domains.addEventListener(BROWSER_EVENT.CHANGE, () => this.showDomain(), {signal});
    this.views.addEventListener(BROWSER_EVENT.CHANGE, () => this.visualize(), {signal});
    required(panel, '#attribute-previous').addEventListener(BROWSER_EVENT.CLICK, () => {
      this.offset = Math.max(0, this.offset - PAGE_SIZE);
      this.renderTable();
    }, {signal});
    required(panel, '#attribute-next').addEventListener(BROWSER_EVENT.CLICK, () => {
      this.offset+= PAGE_SIZE;
      this.renderTable();
    }, {signal});
    this.table.addEventListener(BROWSER_EVENT.CLICK, event => {
      const button = event.target instanceof Element ? event.target.closest('button[data-index]') : null;
      if(button instanceof HTMLButtonElement) {
        this.selectRow(Number(button.dataset.index));
      }
    }, {signal});

  }
  /** @param data @param frame */
  update(data: GeometryData, frame: boolean) {
    this.data = data;
    const previous = this.outputs.value;
    this.outputs.replaceChildren(option('', 'All outputs'));
    for(const output of data.outputs ?? []) {
      this.outputs.append(option(output.name, `${output.name}${output.visible ? '' : ' · inspect'}`));
    }
    this.outputs.value = (data.outputs ?? []).some(output => output.name === previous) ? previous : '';
    this.showResult(frame);
  }
  reset() {
    this.revision+= 1;
    this.data = null;
    this.acceptedData = null;
    this.acceptedName = '';
    this.geometry = null;
    this.locations = [];
    this.selected = -1;
    this.offset = 0;
    this.outputs.replaceChildren(option('', 'Previous build'));
    this.domains.replaceChildren(option(FORM_ATTRIBUTE_DOMAIN.POINTS, 'POINTS'));
    this.views.replaceChildren(option('', 'Material'));
    this.viewport.setInspection([], null);
    this.renderTable();
    this.pending(true);
    required(this.panel, '#attribute-range').textContent = '';
    const kind = document.getElementById('output-kind');
    if(kind) {
      kind.textContent = 'Awaiting valid build';
    }
  }
  /** Pair the inspector with the mesh actually accepted by the GPU resource owner.
   * @param frame
   */
  async showResult(frame: boolean) {
    const data = this.data;
    if(!data) {
      return;
    }
    const revision = ++this.revision;
    const requestedName = this.outputs.value;
    const output = data.outputs?.find(output => output.name === requestedName);
    this.pending(true);
    const accepted = await this.viewport.setData(output?.geometry ?? data, frame);
    if(revision !== this.revision) {
      return;
    }
    this.pending(false);
    if(!accepted) {
      this.data = this.acceptedData;
      this.outputs.replaceChildren(option('', this.data ? 'All outputs' : 'Previous build'));
      for(const previous of this.acceptedData?.outputs ?? []) {
        this.outputs.append(option(previous.name, `${previous.name}${previous.visible ? '' : ' · inspect'}`));
      }
      this.outputs.value = this.acceptedName;
      this.outputs.disabled = !this.data;
      return;
    }
    this.acceptedData = data;
    this.acceptedName = requestedName;
    this.onAccepted();
    const instances = output?.value.kind === FORM_VALUE_KIND.INSTANCES;
    this.geometry = output ? output.value.kind === FORM_VALUE_KIND.INSTANCES ? output.value.sites : output.value : generatedGeometry(data);
    const previous = this.domains.value;
    this.domains.replaceChildren(...(instances ? [FORM_ATTRIBUTE_DOMAIN.INSTANCES] : [FORM_ATTRIBUTE_DOMAIN.POINTS, FORM_ATTRIBUTE_DOMAIN.FACES, FORM_ATTRIBUTE_DOMAIN.CORNERS]).map(value => option(value, value.toUpperCase())));
    this.domains.value = instances ? FORM_ATTRIBUTE_DOMAIN.INSTANCES : ([FORM_ATTRIBUTE_DOMAIN.POINTS, FORM_ATTRIBUTE_DOMAIN.FACES, FORM_ATTRIBUTE_DOMAIN.CORNERS] as readonly string[]).includes(previous) ? previous : FORM_ATTRIBUTE_DOMAIN.POINTS;
    const kind = document.getElementById('output-kind');
    if(kind) {
      kind.textContent = output ? instances ? 'Retained instances' : output.visible ? 'Output geometry' : 'Inspection snapshot' : 'Full build · export source';
    }
    this.showDomain();
  }
  /** @param pending */
  pending(pending: boolean) {
    this.outputs.disabled = pending;
    this.domains.disabled = pending;
    this.views.disabled = pending;
    this.table.inert = pending;
    this.viewport.onPick = pending ? null : index => {
      if(index >= 0) {
        this.offset = Math.floor(index/PAGE_SIZE)*PAGE_SIZE;
         ((this.panel) as HTMLDetailsElement).open = true;
      }
      this.selectRow(index);
    };
  }
  showDomain() {
    this.offset = 0;
    this.selected = -1;
    const previous = this.views.value;
    const attributes = this.attributes();
    this.views.replaceChildren(option('', 'Material'));
    for(const [name, attribute] of Object.entries(attributes)) {
      if(attribute.size === 1 || name === 'color') {
        this.views.append(option(name, name));
      }
    }
    this.views.value = [...this.views.options].some(option => option.value === previous) ? previous : '';
    this.locations = this.getLocations();
    this.renderTable();
    this.visualize();
  }

  domain(): Domain {
    return  ((this.domains.value === FORM_ATTRIBUTE_DOMAIN.INSTANCES ? FORM_ATTRIBUTE_DOMAIN.POINTS : this.domains.value) as Domain);
  }
  attributes() {
    return this.geometry?.attributes[this.domain()] ?? {};
  }
  getLocations() {
    const geometry = this.geometry;
    if(!geometry) {
      return [];
    }
    const positions = ((geometry.attributes.points.position.values) as number[][]);
    if(this.domain() === FORM_ATTRIBUTE_DOMAIN.POINTS) {
      return positions;
    }
    if(this.domain() === FORM_ATTRIBUTE_DOMAIN.CORNERS) {
      return geometry.faces.flatMap(face => face.map(index => positions[index]));
    }
    return geometry.faces.map(face => [0, 1, 2].map(axis => face.reduce((sum, index) => sum + positions[index][axis], 0)/3));
  }
  renderTable() {
    const names = Object.keys(this.attributes());
    const records = this.domains.value === FORM_ATTRIBUTE_DOMAIN.POINTS || this.domains.value === FORM_ATTRIBUTE_DOMAIN.INSTANCES ? this.geometry?.userData : undefined;
    const total = this.locations.length;
    this.offset = Math.min(this.offset, Math.max(0, Math.floor((total - 1)/PAGE_SIZE)*PAGE_SIZE));
    const end = Math.min(total, this.offset + PAGE_SIZE);
    const head = document.createElement('thead'), headings = document.createElement('tr');
    for(const name of ['INDEX', ...names, ...(records ? ['USER DATA'] : [])]) {
      const cell = document.createElement('th');
      cell.scope = 'col';
      cell.textContent = name;
      headings.append(cell);
    }
    head.append(headings);
    const body = document.createElement('tbody');
    for(let index = this.offset; index < end; index++) {
      const row = document.createElement('tr');
      row.classList.toggle('selected', index === this.selected);
      const cell = document.createElement('td'), button = document.createElement('button');
      button.dataset.index = String(index);
      button.textContent = String(index);
      button.setAttribute('aria-label', `Select ${this.domains.value} ${index}`);
      button.setAttribute('aria-pressed', String(index === this.selected));
      cell.append(button);
      row.append(cell);
      for(const name of names) {
        const value = this.attributes()[name].values[index];
        const cell = document.createElement('td');
        cell.textContent = (Array.isArray(value) ? value : [value]).map(format).join(', ');
        cell.title = `${name}: ${JSON.stringify(value)}`;
        row.append(cell);
      }
      if(records) {
        const cell = document.createElement('td');
        cell.textContent = JSON.stringify(records[index]);
        cell.title = cell.textContent;
        row.append(cell);
      }
      body.append(row);
    }
    this.table.replaceChildren(head, body);
    required(this.panel, '#attribute-summary').textContent = `${total.toLocaleString()} ${this.domains.value} · ${names.length} attributes`;
    required(this.panel, '#selection-info').textContent = this.selected >= 0
      ? `${this.domains.value} ${this.selected} · ${this.locations[this.selected].map(format).join(', ')}`
      : total ? `Rows ${this.offset + 1}–${end} · Select a row or click the viewport` : 'Empty result · no elements to inspect';
     ((required(this.panel, '#attribute-previous')) as HTMLButtonElement).disabled = this.offset === 0;
     ((required(this.panel, '#attribute-next')) as HTMLButtonElement).disabled = end >= total;
  }
  /** @param index */
  selectRow(index: number) {
    this.selected = index >= 0 && index < this.locations.length ? index : -1;
    this.renderTable();
    this.viewport.selectElement(this.selected);
  }
  visualize() {
    const name = this.views.value, attribute = this.attributes()[name];

    let colors: number[][] | null = null;
    let range = '';
    if(attribute?.size === 1 && attribute.values.length) {
      const values = ((attribute.values) as number[]);
      let min = Infinity, max = -Infinity;
      for(const value of values) {
        min = Math.min(min, value);
        max = Math.max(max, value);
      }
      colors = values.map(value => {
        const t = max === min ? 0.5 : (value - min)/(max - min);
        return [0.16 + t*0.77, 0.45 - t*0.03, 0.7 - t*0.48];
      });
      range = `${name}: ${format(min)} → ${format(max)}`;
    } else if(attribute?.size === 4) {
      colors = ((attribute.values) as number[][]).map(color => color.slice(0, 3));
      range = name;
    }
    required(this.panel, '#attribute-range').textContent = range;
    this.viewport.setInspection(this.locations, colors);
    this.viewport.selectElement(this.selected);
  }
  destroy() {
    this.revision+= 1;
    this.listeners.abort();
    this.viewport.onPick = null;
  }
}
/** Convert legacy final arrays for inspection; does not change export ownership. @param data */
function generatedGeometry(data: GeometryData): GeometryValue {
  /** @param values @param size */
  const vectors = (values: number[], size: number) => Array.from({length: values.length/size}, (_, index) => values.slice(index*size, (index + 1)*size));
  return {kind: FORM_VALUE_KIND.GEOMETRY, userData: data.userData, faces: vectors(data.triangles, 3), attributes: {
    points: {
      position: {size: 3, values: vectors(data.positions, 3)},
      normal: {size: 3, values: vectors(data.normals, 3)},
      color: {size: 4, values: vectors(data.colors, 3).map((color, index) => [...color, data.alphas[index]])},
      uv: {size: 3, values: vectors(data.uvs, 2).map(uv => [...uv, 0])}
    }, faces: {}, corners: {}
  }};
}
/** @param value */
function format(value: number) {
  return Number(value.toFixed(4)).toString();
}
/** @param value @param label */
function option(value: string, label: string) {
  return new Option(label, value);
}
/** @param root @param selector */
function required(root: HTMLElement, selector: string) {
  const result = root.querySelector(selector);
  if(!(result instanceof HTMLElement)) {
    throw new Error(`Missing inspector control ${selector}.`);
  }
  return result;
}
/** @param root @param selector */
function select(root: HTMLElement, selector: string) {
  return  ((required(root, selector)) as HTMLSelectElement);
}
