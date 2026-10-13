// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import {numericControlMarkup, colorPickerMarkup, escapeAttribute} from '../../../src/lab-controls.ts';
export {refreshRangeFill, colorStyle} from '../../../src/lab-controls.ts';
import { PARTICLE_PARAMETER_TYPE } from 'joy-engine/constants';
import type { ParticleParameter } from 'joy-engine';

/** Render one property as a named group with attached component values and optional bounds.
 * Markup is owned by the lab; input events are delegated by its document owner.
 * @param name @param item
 * @param [collapsed] Hide editable values/bounds while retaining a compact current-value preview.
 */
export function parameterMarkup(name: string, item: ParticleParameter, collapsed: boolean = false) {
  const safeName = escapeAttribute(name);
  const type = item.type ?? 'scalar';
  const code = type === 'vector3' ? 'V' : type === 'color' ? 'C' : 'S';
  const components = type === 'vector3' ? ['X', 'Y', 'Z'] : type === 'color' ? ['R', 'G', 'B', 'A'] : [''];
  const values = Array.isArray(item.value) ? item.value : [item.value];
  const minimums = Array.isArray(item.min) ? item.min : [item.min];
  const maximums = Array.isArray(item.max) ? item.max : [item.max];
  const rows = values.map((value, component) => numericControlMarkup({
    label: `${name} ${components[component]}`.trim() + ' value',
    component: components[component], value, min: minimums[component], max: maximums[component], step: 0.1, sliderStep: 'any',
    attributes: {'data-param': name, 'data-component': String(component), 'data-field': 'value'}
  })).join('');
  const bounds = values.map((value, component) => {
    const label = `${safeName} ${components[component]}`.trim();
    const attributes = `data-param="${safeName}" data-component="${component}" step="any"`;
    const colorLimits = type === 'color' ? 'min="0" max="1"' : '';
    return `
      <div class="bounds-row">
        <b aria-hidden="true">${components[component]}</b>
        <label><span>MIN</span><input aria-label="${label} minimum" type="number" ${attributes} ${colorLimits} data-field="min" value="${minimums[component]}"></label>
        <label><span>MAX</span><input aria-label="${label} maximum" type="number" ${attributes} ${colorLimits} data-field="max" value="${maximums[component]}"></label>
      </div>`;
  }).join('');
  const swatch = type === 'color' ? colorPickerMarkup(name, values, {'data-param': name, 'data-field': 'color'}) : '';
  return `
    <section class="parameter" data-collapsed="${collapsed}" aria-label="${safeName} property">
      <div class="parameter-heading">
        <label class="parameter-type"><span aria-hidden="true">${code}⌄</span>
          <select aria-label="${safeName} type" data-param="${safeName}" data-field="type">
            <option value="scalar" ${type === 'scalar' ? 'selected' : ''}>Scalar</option>
            <option value="vector3" ${type === 'vector3' ? 'selected' : ''}>Vector</option>
            <option value="color" ${type === 'color' ? 'selected' : ''}>Color</option>
          </select>
        </label>
        <input aria-label="Parameter name" data-param="${safeName}" data-field="name" value="${safeName}">
        ${swatch}
        <button class="parameter-collapse" data-param="${safeName}" data-field="collapse" aria-label="${collapsed ? 'Expand' : 'Collapse'} ${safeName} values" aria-expanded="${!collapsed}" title="${collapsed ? 'Expand' : 'Collapse'} values">${collapsed ? '▾' : '▴'}</button>
        <button data-param="${safeName}" data-field="delete" aria-label="Delete ${safeName}">×</button>
      </div>
      <output class="parameter-summary" aria-label="${safeName} current value" ${!collapsed || type === 'color' ? 'hidden' : ''}>${parameterSummary(item)}</output>
      <div class="parameter-values ${values.length > 1 ? 'lab-components' : ''}" style="--lab-components:${values.length}" ${collapsed ? 'hidden' : ''}>${rows}</div>
      <details class="parameter-bounds" ${collapsed ? 'hidden' : ''}><summary>Bounds${type === 'color' ? ' · RGBA 0–1' : ''}</summary>${bounds}</details>
    </section>`;
}

/** Toggle editing visibility without replacing controls, losing focus, or changing the asset.
 * @param card @param name
 * @param item @param collapsed
 */
export function setParameterCollapsed(card: HTMLElement, name: string, item: ParticleParameter, collapsed: boolean) {
  card.dataset.collapsed = String(collapsed);
  for(const element of card.querySelectorAll('.parameter-values, .parameter-bounds')) {
    if(element instanceof HTMLElement) {
      element.hidden = collapsed;
    }
  }
  const summary = card.querySelector('.parameter-summary');
  if(summary instanceof HTMLElement) {
    summary.hidden = !collapsed || item.type === PARTICLE_PARAMETER_TYPE.COLOR;
    summary.textContent = parameterSummary(item);
  }
  const button = card.querySelector('.parameter-collapse');
  if(button instanceof HTMLButtonElement) {
    const action = collapsed ? 'Expand' : 'Collapse';
    button.textContent = collapsed ? '▾' : '▴';
    button.setAttribute('aria-expanded', String(!collapsed));
    button.setAttribute('aria-label', `${action} ${name} values`);
    button.title = `${action} values`;
  }
}

/** Read-only display rounding never changes the authored value.
 * @param item
 */
function parameterSummary(item: ParticleParameter) {
  if(item.type === PARTICLE_PARAMETER_TYPE.COLOR) {
    return '';
  }
  if(item.type === PARTICLE_PARAMETER_TYPE.VECTOR3) {
    return item.value.map((value, index) => `${['X', 'Y', 'Z'][index]} ${Number(value.toFixed(3))}`).join(' · ');
  }
  return String(Number(item.value.toFixed(3)));
}
