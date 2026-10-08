// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

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
  const rows = values.map((value, component) => {
    const label = `${safeName} ${components[component]}`.trim();
    const attributes = `data-param="${safeName}" data-component="${component}"`;
    return `
      <div class="parameter-component">
        <b aria-hidden="true">${components[component]}</b>
        <input aria-label="${label} slider" type="range" ${attributes} data-field="value" min="${minimums[component]}" max="${maximums[component]}" step="any" value="${value}" style="--range-fill:${rangePercent(value, minimums[component], maximums[component])}%">
        <input aria-label="${label} value" class="component-value" type="number" ${attributes} data-field="value" min="${minimums[component]}" max="${maximums[component]}" step="any" value="${value}">
      </div>`;
  }).join('');
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
  const swatch = type === 'color' ? `<span class="color-swatch" aria-label="${safeName} color preview"><i style="background:${colorStyle(values)}"></i></span>` : '';
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
      <div class="parameter-values" ${collapsed ? 'hidden' : ''}>${rows}</div>
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

/** Keep the shared track fill aligned with native keyboard/pointer range input.
 * @param input
 */
export function refreshRangeFill(input: HTMLInputElement) {
  input.style.setProperty('--range-fill', `${rangePercent(input.valueAsNumber, Number(input.min), Number(input.max))}%`);
}

/** @param values */
export function colorStyle(values: readonly number[]) {
  return `rgba(${values[0] * 255}, ${values[1] * 255}, ${values[2] * 255}, ${values[3]})`;
}

/** @param value @param min @param max */
function rangePercent(value: number, min: number, max: number) {
  return max > min ? Math.max(0, Math.min(100, (value - min) / (max - min) * 100)) : 0;
}

/** @param value */
function escapeAttribute(value: string) {
  return value.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;').replaceAll("'", '&#39;');
}
