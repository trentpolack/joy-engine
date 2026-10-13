// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import {createNumericControl, colorPickerMarkup, pickedColor, syncColorControl, refreshRangeFill} from '../../../src/lab-controls.ts';
import { BROWSER_EVENT, FORM_PARAMETER_TYPE, FORM_VALUE_KIND, MATERIAL_ALPHA_MODE } from 'joy-engine/constants';
import type { ResourceParameter, ParameterValue } from 'joy-engine/form';
import { parseGlbMesh } from 'joy-engine';
import { meshBox, meshSphere, readMesh, readMaterialValue } from 'joy-engine/form';
export type { ResourceParameter };
export type { ParameterValue };

const inputCommits = new WeakMap<HTMLInputElement, () => void>();

const synchronizers: WeakMap<HTMLElement, (parameter: ResourceParameter) => void> = new WeakMap();

/** Resource controls own no runtime state; events publish explicit parameter overrides.
 * @param parameter
 * @param onChange
 * @param signal
 */
export function createResourceGroup(parameter: ResourceParameter, onChange: (name: string, value: ParameterValue | undefined) => void, signal: AbortSignal, edit: (action: () => void) => void = action => action(), onReset: (name: string) => void = name => onChange(name, undefined)) {
  let importRequest = 0;
  let importMessage = '';
  const group = document.createElement('section');
  group.className = 'parameter resource-parameter';
  group.dataset.resourceParameter = parameter.name;
  const heading = document.createElement('div');
  heading.className = 'parameter-heading';
  const badge = document.createElement('span');
  badge.className = 'parameter-type';
  badge.textContent = parameter.type === FORM_PARAMETER_TYPE.MESH ? 'MESH' : 'MAT';
  const title = document.createElement('h3');
  title.textContent = parameter.name.replaceAll('_', ' ').toUpperCase();
  heading.append(badge, title);
  group.append(heading);
  const status = document.createElement('p');
  status.className = 'resource-status';
  status.setAttribute('role', 'status');
  const reset = document.createElement('button');
  reset.type = 'button';
  reset.textContent = '↺';
  reset.className = 'parameter-reset';
  reset.setAttribute('aria-label', 'Use script default');
  reset.title = `Reset ${parameter.name} to script default`;
  heading.append(reset);
  reset.addEventListener(BROWSER_EVENT.CLICK, () => {
    importRequest++;
    importMessage = '';
    onReset(parameter.name);
  }, {signal});

  if(parameter.value.kind === FORM_VALUE_KIND.MESH) {
    const mesh = parameter.value;
    const label = document.createElement('label');
    label.textContent = 'Mesh source';
    const select = document.createElement('select');
    select.setAttribute('aria-label', `${parameter.name} mesh source`);
    for(const [value, text] of [['current', mesh.name], ['box', 'Box · unit size'], ['sphere', 'Sphere · unit radius']]) {
      select.add(new Option(text, value));
    }
    select.addEventListener(BROWSER_EVENT.CHANGE, () => {
      importRequest++;
      importMessage = '';
      if(select.value === 'current') {
        return;
      }
      onChange(parameter.name, select.value === 'box' ? meshBox(1, 1, 1) : meshSphere(1, 20));
    }, {signal});
    label.append(select);
    const importLabel = document.createElement('label');
    importLabel.className = 'resource-import';
    importLabel.textContent = 'Import GLB geometry';
    const file = document.createElement('input');
    file.type = 'file';
    file.accept = '.glb';
    file.setAttribute('aria-label', `${parameter.name} GLB geometry`);
    file.addEventListener(BROWSER_EVENT.CHANGE, async () => {
      const request = ++importRequest;
      const asset = file.files?.[0];
      if(!asset) {
        return;
      }
      try {
        if(asset.size > 16000000) {
          throw new Error('GLB input must be 16 MB or smaller.');
        }
        importMessage = 'Reading geometry…';
        status.textContent = importMessage;
        const decoded = await parseGlbMesh(await asset.arrayBuffer());
        if(signal.aborted || !group.isConnected || request !== importRequest) {
          return;
        }
        const value = readMesh({...decoded, kind: FORM_VALUE_KIND.MESH, name: asset.name.slice(0, 120),
          uvs: new Array(decoded.positions.length/3*2).fill(0)});
        importMessage = '';
        onChange(parameter.name, value);
        status.textContent = 'Geometry loaded.';
      } catch(error) {
        if(signal.aborted || !group.isConnected || request !== importRequest) {
          return;
        }
        importMessage = error instanceof Error ? error.message : String(error);
        status.textContent = importMessage;
      }
    }, {signal});
    importLabel.append(file);
    status.textContent = importMessage || `${mesh.positions.length/3} vertices · ${mesh.indices.length/3} triangles. GLB: one triangle primitive; geometry only.`;
    group.append(label, importLabel);
    synchronizers.set(group, current => {
      if(current.value.kind !== FORM_VALUE_KIND.MESH) {
        return;
      }
      const mesh = current.value;
      select.options[0].textContent = mesh.name;
      select.value = 'current';
      status.textContent = importMessage || `${mesh.positions.length/3} vertices · ${mesh.indices.length/3} triangles. GLB: one triangle primitive; geometry only.`;
    });
  } else {
    let value = readMaterialValue(parameter.value);
    const color = document.createElement('div');
    color.className = 'lab-components';
    color.style.setProperty('--lab-components', '4');
    const title = document.createElement('span');
    title.className = 'lab-field-title';
    title.textContent = 'Base color · linear RGBA';
    heading.insertAdjacentHTML('beforeend', colorPickerMarkup(parameter.name, value.material.baseColor));
    const picker = heading.querySelector<HTMLInputElement>('.lab-color-picker')!;
    picker.addEventListener(BROWSER_EVENT.CHANGE, () => {
      const hex = picker.value;
      try {
        edit(() => {
          value.material.baseColor = pickedColor(hex, value.material.baseColor, [0, 0, 0], [1, 1, 1]);
          publish();
          refresh();
        });
        status.textContent = '';
      } catch(error) {
        status.textContent = error instanceof Error ? error.message : String(error);
        syncColorControl(group, value.material.baseColor);
      }
    }, {signal});
    const controls: {input: HTMLInputElement; read: () => number}[] = [];
    for(const [index, name] of ['Red', 'Green', 'Blue', 'Alpha'].entries()) {
      addNumber(color, name, ['R', 'G', 'B', 'A'][index], () => value.material.baseColor[index], number => {
        value.material.baseColor[index] = number;
        value.material.alphaMode = value.material.baseColor[3] < 1 ? MATERIAL_ALPHA_MODE.BLEND : MATERIAL_ALPHA_MODE.OPAQUE;
      });
    }
    group.append(title, color);
    for(const key of ['metallic', 'roughness'] as const) {
      const row = document.createElement('div');
      row.className = 'lab-scalar';
      const label = document.createElement('span');
      label.className = 'lab-field-title';
      label.textContent = key === 'metallic' ? 'Metallic' : 'Roughness';
      row.append(label);
      addNumber(row, key, '', () => value.material[key], number => { value.material[key] = number; });
      group.append(row);
    }
    synchronizers.set(group, current => {
      value = readMaterialValue(current.value);
      refresh();
    });
    refresh();

    function publish() {
      onChange(parameter.name, readMaterialValue(value));
      group.classList.add('modified');
      reset.disabled = false;
      syncColorControl(group, value.material.baseColor);
    }

    function refresh() {
      for(const {input, read} of controls) {
        if(input !== document.activeElement && input.getAttribute('aria-invalid') !== 'true') {
          input.value = String(read());
        }
        refreshRangeFill(input);
      }
      syncColorControl(group, value.material.baseColor);
    }

    function addNumber(parent: HTMLElement, name: string, component: string, read: () => number, change: (value: number) => void) {
      const control = createNumericControl({label: `${parameter.name} ${name}`, component, value: read(), min: 0, max: 1, step: 0.01, slider: !component});
      for(const input of control.querySelectorAll('input')) {
        controls.push({input, read});
        const commit = () => {
          const number = input.valueAsNumber;
          if(!Number.isFinite(number) || number < 0 || number > 1) {
            input.setCustomValidity('Enter a number from 0 to 1.');
            return;
          }
          input.setCustomValidity('');
          change(number);
          for(const sibling of control.querySelectorAll('input')) {
            sibling.value = String(number);
            sibling.setCustomValidity('');
            refreshRangeFill(sibling);
          }
          publish();
        };
        inputCommits.set(input, commit);
      }
      parent.append(control);
    }
  }
  group.append(status);
  return group;

}

/** Refresh resource values while preserving focused controls and in-flight file selection.
 * @param group @param parameter
 */
export function updateResourceGroup(group: HTMLElement, parameter: ResourceParameter, overridden: boolean = false) {
  group.classList.toggle('modified', overridden);
  const reset = group.querySelector<HTMLButtonElement>('.parameter-reset');
  if(reset) {
    reset.disabled = !overridden;
  }
  synchronizers.get(group)?.(parameter);
}

/** Commit a focused resource draft through the same path as native input. */
export function commitResourceInput(input: HTMLInputElement): boolean {
  const commit = inputCommits.get(input);
  commit?.();
  return Boolean(commit);
}
