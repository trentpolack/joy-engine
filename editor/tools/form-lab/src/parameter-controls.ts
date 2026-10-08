// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { BROWSER_EVENT, FORM_PARAMETER_TYPE } from 'joy-engine/constants';
import type { Parameter, ParameterValue, NumericParameter } from 'joy-engine/form';
import { createResourceGroup, updateResourceGroup } from './resource-parameter-controls.ts';
export type { Parameter };
export type { ParameterValue };

/** Owns script-derived controls and delegated listeners; preserves focus across generation. */
export class ParameterControls {
  declare container: HTMLElement;
  declare onChange: (name: string, value: ParameterValue | undefined) => void;
  declare signature: string;
  declare resourceListeners: AbortController;
  declare parameters: Parameter[];
  declare listeners: AbortController;

  /** @param container @param onChange */
  constructor(container: HTMLElement, onChange: (name: string, value: ParameterValue | undefined) => void) {
    this.container = container;
    this.onChange = onChange;
    this.signature = '';
    this.resourceListeners = new AbortController();

    this.parameters = [];
    this.listeners = new AbortController();
    container.addEventListener(
      BROWSER_EVENT.INPUT,
      (event) => this.change(event, 'range'),
      { signal: this.listeners.signal },
    );
    container.addEventListener(
      BROWSER_EVENT.CHANGE,
      (event) => this.change(event, 'number'),
      { signal: this.listeners.signal },
    );
  }

  /** Refresh generated values without replacing focused controls unless declarations changed.
   * @param parameters Borrowed declarations from the last successful generation.
   */
  update(parameters: Parameter[]) {
    const signature = JSON.stringify(
      parameters.map(parameter => !('min' in parameter)
        ? {name: parameter.name, type: parameter.type}
        : (({value, ...declaration}) => declaration)(parameter)),
    );
    this.parameters = parameters;
    if(signature !== this.signature) {
      this.signature = signature;
      this.resourceListeners.abort();
      this.resourceListeners = new AbortController();
      this.container.replaceChildren();
      for(const parameter of parameters) {
        this.container.append(!('min' in parameter)
          ? createResourceGroup(parameter, this.onChange, this.resourceListeners.signal)
          : createGroup(parameter));
      }
      if(!parameters.length) {
        const note = document.createElement('p');
        note.className = 'inspector-intro';
        note.textContent =
          'Add a param declaration to give your script a control.';
        this.container.append(note);
      }
    }
    for(const parameter of parameters) {
      if(!('min' in parameter)) {
        for(const group of this.container.querySelectorAll('.resource-parameter')) {
          if(group instanceof HTMLElement && group.dataset.resourceParameter === parameter.name) {
            updateResourceGroup(group, parameter);
          }
        }
        continue;
      }
      const values = Array.isArray(parameter.value)
        ? parameter.value
        : [parameter.value];
      for(const node of this.container.querySelectorAll('input')) {
        if(node.dataset.parameter !== parameter.name) {
          continue;
        }
        if(node !== document.activeElement && node.getAttribute('aria-invalid') !== 'true') {
          node.value = String(values[Number(node.dataset.component)]);
        }
        updateRangeFill(node);
      }
      this.updateSwatch(parameter.name, values);
    }
  }

  /** @private @param name @param values */
  private updateSwatch(name: string, values: number[]) {
    for(const swatch of this.container.querySelectorAll('.color-swatch')) {
      if(
        !(swatch instanceof HTMLElement) ||
        swatch.dataset.parameter !== name
      ) {
        continue;
      }
      const fill = swatch.querySelector('i');
      if(fill) {
        const [red, green, blue, alpha] = values;
        fill.style.backgroundColor = `rgba(${red * 255}, ${green * 255}, ${blue * 255}, ${alpha})`;
        swatch.title = `RGBA (${values.join(', ')})`;
      }
    }
  }

  /** Invalidate pending resource reads before changing documents. */
  clear() {
    this.resourceListeners.abort();
    this.signature = '';
    this.parameters = [];
    this.container.replaceChildren();
  }

  /** Release listeners before the containing application is disposed. */
  destroy() {
    this.listeners.abort();
    this.resourceListeners.abort();
  }

  /** @private @param event @param inputType */
  private change(event: Event, inputType: string) {
    this.changeInput(event.target, inputType);
  }

  /** Commit a direct input using the same owner path as native events.
   * @param node @param inputType
   */
  changeInput(node: EventTarget | null, inputType: string) {
    if(!(node instanceof HTMLInputElement) || node.type !== inputType) {
      return;
    }
    const parameter = this.parameters.find(
      (item) => item.name === node.dataset.parameter,
    );
    if(!parameter || !('min' in parameter)) {
      return;
    }
    const component = Number(node.dataset.component);
    const number = node.valueAsNumber;
    if(!Number.isFinite(number) || number < parameter.min || number > parameter.max) {
      node.setCustomValidity(`Enter a number from ${parameter.min} to ${parameter.max}.`);
      return;
    }
    node.setCustomValidity('');
    const value = number;
    for(const sibling of this.container.querySelectorAll('input')) {
      if(
        sibling.dataset.parameter === parameter.name &&
        sibling.dataset.component === node.dataset.component
      ) {
        sibling.setCustomValidity('');
        sibling.value = String(value);
        updateRangeFill(sibling);
      }
    }
    // Read the visible controls: a second edit can arrive before the worker finishes the first.
    const values = [];
    for(const input of this.container.querySelectorAll('input')) {
      if(
        input.type === 'number' &&
        input.dataset.parameter === parameter.name
      ) {
        values.push(Number(input.value));
      }
    }
    this.updateSwatch(parameter.name, values);
    this.onChange(
      parameter.name,
      parameter.type === FORM_PARAMETER_TYPE.SCALAR ? values[0] : values,
    );
  }
}

/** @param parameter */
function createGroup(parameter: NumericParameter) {
  const group = document.createElement('section');
  group.className = 'parameter';
  const heading = document.createElement('div');
  heading.className = 'parameter-heading';
  const title = document.createElement('h3');
  title.textContent = parameter.name.replaceAll('_', ' ').toUpperCase();
  const type = document.createElement('span');
  type.className = 'parameter-type';
  type.textContent = parameter.type === FORM_PARAMETER_TYPE.SCALAR ? 'S' : parameter.type === FORM_PARAMETER_TYPE.VECTOR ? 'V' : 'C';
  type.title = parameter.type;
  type.setAttribute('aria-label', `${parameter.type} parameter`);
  heading.append(type, title);
  if(parameter.type === FORM_PARAMETER_TYPE.COLOR) {
    const swatch = document.createElement('span');
    swatch.className = 'color-swatch';
    swatch.dataset.parameter = parameter.name;
    swatch.setAttribute('role', 'img');
    swatch.setAttribute('aria-label', `${parameter.name} color preview`);
    swatch.append(document.createElement('i'));
    heading.append(swatch);
  }
  group.append(heading);
  const values = document.createElement('div');
  values.className = 'parameter-values';
  group.append(values);
  const components =
    parameter.type === FORM_PARAMETER_TYPE.SCALAR
      ? ['Value']
      : parameter.type === FORM_PARAMETER_TYPE.VECTOR
        ? ['X', 'Y', 'Z']
        : ['R', 'G', 'B', 'A'];
  components.forEach((name, index) => {
    const component = document.createElement('div');
    component.className = 'parameter-component';
    const top = document.createElement('div');
    top.className = 'parameter-top';
    const label = document.createElement('label');
    label.textContent = name;
    const id =
      parameter.type === FORM_PARAMETER_TYPE.SCALAR
        ? `param-${parameter.name}`
        : `param-${parameter.name}-${name.toLowerCase()}`;
    label.htmlFor = id;
    const number = document.createElement('input');
    number.type = 'number';
    const range = document.createElement('input');
    range.type = 'range';
    range.id = id;
    const accessibleName =
      parameter.type === FORM_PARAMETER_TYPE.SCALAR
        ? parameter.name
        : `${parameter.name} ${name}`;
    number.setAttribute('aria-label', `${accessibleName} value`);
    range.setAttribute('aria-label', accessibleName);
    for(const node of [number, range]) {
      node.min = String(parameter.min);
      node.max = String(parameter.max);
      node.step = String(parameter.step);
      node.dataset.parameter = parameter.name;
      node.dataset.component = String(index);
    }
    top.append(label, number);
    component.append(top, range);
    values.append(component);
  });
  return group;
}

/** Synchronize the slider fill after edits, clamping, and Reset. @param node */
export function updateRangeFill(node: HTMLInputElement) {
  if(node.type !== 'range') {
    return;
  }
  const span = Number(node.max) - Number(node.min);
  const fill =
    span > 0 ? ((Number(node.value) - Number(node.min)) / span) * 100 : 0;
  node.style.setProperty(
    '--range-fill',
    `${Math.max(0, Math.min(100, fill))}%`,
  );
}

