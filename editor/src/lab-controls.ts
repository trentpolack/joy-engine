// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

interface NumericControlOptions {
  label: string;
  component?: string;
  value: number;
  min?: number;
  max?: number;
  step?: number;
  slider?: boolean;
  sliderStep?: number | 'any';
  id?: string;
  attributes?: Record<string, string>;
}

/**
 * Shared lab markup borrows authored bounds and values; it owns no document state.
 * Exact inputs use any precision. The authored step remains the keyboard/slider increment.
 */
export function numericControlMarkup(options: NumericControlOptions): string {
  const {label, component = '', value, min, max, step = 0.01, id, attributes = {}} = options;
  const metadata = Object.entries(attributes).map(([name, value]) => `${name}="${escapeAttribute(value)}"`).join(' ');
  const bounds = `${min === undefined ? '' : `min="${min}"`} ${max === undefined ? '' : `max="${max}"`}`;
  const slider = options.slider !== false && Number.isFinite(min) && Number.isFinite(max) && Number(max) > Number(min);
  return `<div class="lab-number ${component ? 'lab-number--component' : ''}">
    ${component ? `<span class="lab-component-label" aria-hidden="true">${escapeAttribute(component)}</span>` : ''}
    <input class="lab-number-value" type="number" aria-label="${escapeAttribute(label)}" ${id ? `id="${escapeAttribute(id)}-value"` : ''} ${metadata} ${bounds} step="any" data-numeric-step="${step}" value="${value}">
    ${slider ? `<input class="lab-range" type="range" aria-label="${escapeAttribute(label.replace(/ value$/, ''))} slider" ${id ? `id="${escapeAttribute(id)}"` : ''} ${metadata} ${bounds} step="${options.sliderStep ?? step}" value="${value}" style="--range-fill:${rangePercent(value, Number(min), Number(max))}%">` : ''}
  </div>`;
}

/** Create the same numeric markup for DOM-owned inspectors. */
export function createNumericControl(options: NumericControlOptions): HTMLDivElement {
  const template = document.createElement('template');
  template.innerHTML = numericControlMarkup(options);
  return template.content.firstElementChild as HTMLDivElement;
}

/** Native picker is a display-space affordance; precise channels remain linear and unrounded. */
export function colorPickerMarkup(name: string, values: readonly number[], attributes: Record<string, string> = {}): string {
  const metadata = Object.entries(attributes).map(([key, value]) => `${key}="${escapeAttribute(value)}"`).join(' ');
  return `<label class="color-swatch lab-color" aria-label="${escapeAttribute(name)} color preview">
    <i style="background:${colorStyle(values)}"></i>
    <input class="lab-color-picker" type="color" aria-label="${escapeAttribute(name)} color picker" title="Choose sRGB color · precise channels are linear · alpha is preserved" value="${colorHex(values)}" data-accepted-color="${colorHex(values)}" ${metadata}>
  </label>`;
}

/** Refresh display only; clamping or quantization here never writes authored channel values. */
export function syncColorControl(root: ParentNode, values: readonly number[]): void {
  const fill = root.querySelector<HTMLElement>('.color-swatch i');
  if(fill) {
    fill.style.background = colorStyle(values);
    fill.parentElement!.title = `Linear RGBA (${values.join(', ')})`;
  }
  const picker = root.querySelector<HTMLInputElement>('.lab-color-picker');
  if(picker) {
    picker.value = colorHex(values);
    picker.dataset.acceptedColor = picker.value;
  }
}

/**
 * An explicit picker selection replaces RGB only, within the owner's existing bounds.
 * Opening/refreshing the picker never invokes this conversion, preserving HDR and exact drafts.
 */
export function pickedColor(hex: string, previous: readonly number[], minimums: readonly number[], maximums: readonly number[]): number[] {
  const next = [...previous];
  for(let channel = 0; channel < 3; channel++) {
    const display = parseInt(hex.slice(1 + channel*2, 3 + channel*2), 16)/255;
    const linear = display <= 0.04045 ? display/12.92 : ((display + 0.055)/1.055)**2.4;
    next[channel] = Math.max(minimums[channel], Math.min(maximums[channel], linear));
  }
  return next;
}

export function refreshRangeFill(input: HTMLInputElement): void {
  if(input.type === 'range') {
    input.style.setProperty('--range-fill', `${rangePercent(input.valueAsNumber, Number(input.min), Number(input.max))}%`);
  }
}

export function colorStyle(values: readonly number[]): string {
  return `rgba(${values.slice(0, 3).map(value => linearToDisplay(value)*255).join(', ')}, ${values[3] ?? 1})`;
}

function colorHex(values: readonly number[]): string {
  return `#${values.slice(0, 3).map(value => Math.round(linearToDisplay(value)*255).toString(16).padStart(2, '0')).join('')}`;
}

function linearToDisplay(value: number): number {
  const bounded = Math.max(0, Math.min(1, value));
  return bounded <= 0.0031308 ? bounded*12.92 : 1.055*(bounded**(1/2.4)) - 0.055;
}

function rangePercent(value: number, min: number, max: number): number {
  return max > min ? Math.max(0, Math.min(100, (value - min)/(max - min)*100)) : 0;
}

export function escapeAttribute(value: string): string {
  return value.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;').replaceAll("'", '&#39;');
}
