// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { BROWSER_EVENT, FORM_PARAMETER_TYPE } from 'joy-engine/constants';
import type { Parameter, ParameterValue, NumericParameter } from 'joy-engine/form';
import {colorPickerMarkup, createNumericControl, pickedColor, syncColorControl, refreshRangeFill} from '../../../src/lab-controls.ts';
import { createResourceGroup, updateResourceGroup, commitResourceInput } from './resource-parameter-controls.ts';
export type { Parameter };
export type { ParameterValue };

/** Owns script-derived controls and delegated listeners; preserves focus across generation. */
export class ParameterControls {
  declare container: HTMLElement;
  declare onChange: (name: string, value: ParameterValue | undefined) => void;
  declare signature: string;
  declare onReset: (name: string) => void;
  declare resourceListeners: AbortController;
  declare parameters: Parameter[];
  declare listeners: AbortController;
  declare edit: (action: () => void) => void;

  /** @param container @param onChange */
  constructor(container: HTMLElement, onChange: (name: string, value: ParameterValue | undefined) => void, onReset: (name: string) => void = name => onChange(name, undefined), edit: (action: () => void) => void = action => action()) {
    this.container = container;
    this.onChange = onChange;
    this.signature = '';
    this.onReset = onReset;
    this.edit = edit;
    this.resourceListeners = new AbortController();

    this.parameters = [];
    this.listeners = new AbortController();
    container.addEventListener(BROWSER_EVENT.CLICK, event => {
      const target = event.target;
      if(target instanceof HTMLButtonElement && target.dataset.resetParameter) {
        this.onReset(target.dataset.resetParameter);
      }
    }, {signal: this.listeners.signal});
    container.addEventListener(
      BROWSER_EVENT.INPUT,
      (event) => this.change(event, 'range'),
      { signal: this.listeners.signal },
    );
    container.addEventListener(
      BROWSER_EVENT.CHANGE,
      event => {
        if(event.target instanceof HTMLInputElement && event.target.type === 'color' && event.target.dataset.colorParameter) {
          this.changeColor(event.target);
        } else {
          this.change(event, 'number');
        }
      },
      { signal: this.listeners.signal },
    );
  }

  /** Refresh generated values without replacing focused controls unless declarations changed.
   * @param parameters Borrowed declarations from the last successful generation.
   */
  update(parameters: Parameter[], overrides: Record<string, ParameterValue> = {}) {
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
          ? createResourceGroup(parameter, this.onChange, this.resourceListeners.signal, this.edit, this.onReset)
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
            updateResourceGroup(group, parameter, Object.hasOwn(overrides, parameter.name));
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
        refreshRangeFill(node);
      }
      this.updateSwatch(parameter.name, values);
      this.updateDefaultState(parameter, values, Object.hasOwn(overrides, parameter.name));
    }
  }

  /** Compare effective values with script defaults without changing the document.
   * @param parameter @param values @param overridden
   */
  private updateDefaultState(parameter: NumericParameter, values: number[], overridden: boolean) {
    const defaults = Array.isArray(parameter.defaultValue) ? parameter.defaultValue : [parameter.defaultValue];
    const modified = values.some((value, index) => value !== defaults[index]);
    for(const group of this.container.querySelectorAll<HTMLElement>('[data-numeric-parameter]')) {
      if(group.dataset.numericParameter !== parameter.name) {
        continue;
      }
      group.classList.toggle('modified', modified);
      const state = group.querySelector<HTMLElement>('.parameter-default');
      if(state) {
        state.textContent = modified ? 'Modified' : overridden ? 'Override' : 'Default';
        state.title = `Script default: ${defaults.join(', ')}`;
        state.setAttribute('aria-label', `${state.textContent}; ${state.title}`);
      }
      const reset = group.querySelector<HTMLButtonElement>('[data-reset-parameter]');
      if(reset) {
        reset.disabled = !overridden;
      }
    }
  }

  /** @private @param name @param values */
  private updateSwatch(name: string, values: number[]) {
    for(const group of this.container.querySelectorAll<HTMLElement>('[data-numeric-parameter]')) {
      if(group.dataset.numericParameter === name) {
        syncColorControl(group, values);
      }
    }
  }

  private changeColor(input: HTMLInputElement) {
    const parameter = this.parameters.find(item => item.name === input.dataset.colorParameter);
    if(!parameter || !('min' in parameter) || !Array.isArray(parameter.value)) {
      return;
    }
    const hex = input.value;
    try {
      this.edit(() => {
        const previous = [...this.container.querySelectorAll<HTMLInputElement>('input[type=number][data-parameter]')]
          .filter(node => node.dataset.parameter === parameter.name).map(node => node.valueAsNumber);
        const values = pickedColor(hex, previous, Array(3).fill(parameter.min), Array(3).fill(parameter.max));
        for(const node of this.container.querySelectorAll<HTMLInputElement>('input[data-parameter]')) {
          if(node.dataset.parameter === parameter.name) {
            node.value = String(values[Number(node.dataset.component)]);
            refreshRangeFill(node);
          }
        }
        this.updateSwatch(parameter.name, values);
        this.updateDefaultState(parameter, values, true);
        this.onChange(parameter.name, values);
      });
    } catch(error) {
      // Live may be off: accepted edits can be newer than compiled declarations.
      input.value = input.dataset.acceptedColor ?? input.defaultValue;
      input.title = error instanceof Error ? error.message : String(error);
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
    if(commitResourceInput(node)) {
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
        refreshRangeFill(sibling);
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
    this.updateDefaultState(parameter, values, true);
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
  group.dataset.numericParameter = parameter.name;
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
    heading.insertAdjacentHTML('beforeend', colorPickerMarkup(parameter.name, parameter.value as number[], {'data-color-parameter': parameter.name}));
  }
  const reset = document.createElement('button');
  reset.type = 'button';
  reset.className = 'parameter-reset';
  reset.dataset.resetParameter = parameter.name;
  reset.textContent = '↺';
  reset.title = `Reset ${parameter.name} to script default`;
  reset.setAttribute('aria-label', reset.title);
  heading.append(reset);
  group.append(heading);
  const defaultState = document.createElement('p');
  defaultState.className = 'parameter-default';
  heading.insertBefore(defaultState, reset);
  const values = document.createElement('div');
  values.className = 'parameter-values';
  group.append(values);
  const components =
    parameter.type === FORM_PARAMETER_TYPE.SCALAR
      ? ['Value']
      : parameter.type === FORM_PARAMETER_TYPE.VECTOR
        ? ['X', 'Y', 'Z']
        : ['R', 'G', 'B', 'A'];
  const initial = Array.isArray(parameter.value) ? parameter.value : [parameter.value];
  if(components.length > 1) {
    values.classList.add('lab-components');
    values.style.setProperty('--lab-components', String(components.length));
  }
  components.forEach((name, index) => {
    const id = parameter.type === FORM_PARAMETER_TYPE.SCALAR ? `param-${parameter.name}` : `param-${parameter.name}-${name.toLowerCase()}`;
    const accessibleName = parameter.type === FORM_PARAMETER_TYPE.SCALAR ? parameter.name : `${parameter.name} ${name}`;
    const control = createNumericControl({
      label: `${accessibleName} value`, component: components.length > 1 ? name : '',
      value: initial[index], min: parameter.min, max: parameter.max, step: parameter.step, id,
      attributes: {'data-parameter': parameter.name, 'data-component': String(index)}
    });
    // Retain existing accessible range names for script parameter controls.
    control.querySelector('input[type=range]')?.setAttribute('aria-label', accessibleName);
    values.append(control);
  });
  return group;
}

export {refreshRangeFill as updateRangeFill};
