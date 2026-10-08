// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { BROWSER_EVENT, FORM_PARAMETER_TYPE, FORM_VALUE_KIND, MATERIAL_ALPHA_MODE } from 'joy-engine/constants';
import type { ResourceParameter, ParameterValue } from 'joy-engine/form';
import { parseGlbMesh } from 'joy-engine';
import { meshBox, meshSphere, readMesh, readMaterialValue } from 'joy-engine/form';
export type { ResourceParameter };
export type { ParameterValue };

const synchronizers: WeakMap<HTMLElement, (parameter: ResourceParameter) => void> = new WeakMap();

/** Resource controls own no runtime state; events publish explicit parameter overrides.
 * @param parameter
 * @param onChange
 * @param signal
 */
export function createResourceGroup(parameter: ResourceParameter, onChange: (name: string, value: ParameterValue | undefined) => void, signal: AbortSignal) {
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
  reset.textContent = 'Use script default';
  reset.addEventListener(BROWSER_EVENT.CLICK, () => {
    importRequest++;
    importMessage = '';
    onChange(parameter.name, undefined);
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
    for(const [name, component] of [['Red', 0], ['Green', 1], ['Blue', 2], ['Alpha', 3]]) {
      const index = Number(component);
      addNumber(String(name), value.material.baseColor[index], number => {
        value.material.baseColor[index] = number;
        value.material.alphaMode = value.material.baseColor[3] < 1 ? MATERIAL_ALPHA_MODE.BLEND : MATERIAL_ALPHA_MODE.OPAQUE;
        onChange(parameter.name, readMaterialValue(value));
      });
    }
    for(const key of  ((['metallic', 'roughness']) as const)) {
      addNumber(key, value.material[key], number => {
        value.material[key] = number;
        onChange(parameter.name, readMaterialValue(value));
      });
    }
    status.textContent = 'Linear RGBA · independent material override';
    synchronizers.set(group, current => {
      value = readMaterialValue(current.value);
      const numbers = [...value.material.baseColor, value.material.metallic, value.material.roughness];
      group.querySelectorAll('input[type=number]').forEach((input, index) => {
        if(input instanceof HTMLInputElement && input !== document.activeElement) {
          input.value = String(numbers[index]);
        }
      });
    });
  }
  group.append(status, reset);
  return group;

  /** @param name @param value @param change */
  function addNumber(name: string, value: number, change: (value: number) => void) {
    const label = document.createElement('label');
    label.className = 'parameter-top';
    label.textContent = name;
    const input = document.createElement('input');
    input.type = 'number';
    input.min = '0';
    input.max = '1';
    input.step = '0.01';
    input.value = String(value);
    input.setAttribute('aria-label', `${parameter.name} ${name}`);
    input.addEventListener(BROWSER_EVENT.CHANGE, () => {
      const number = input.value === '' ? NaN : Number(input.value);
      if(!Number.isFinite(number)) {
        input.value = String(value);
        return;
      }
      const clamped = Math.max(0, Math.min(1, number));
      input.value = String(clamped);
      change(clamped);
    }, {signal});
    label.append(input);
    group.append(label);
  }
}

/** Refresh resource values while preserving focused controls and in-flight file selection.
 * @param group @param parameter
 */
export function updateResourceGroup(group: HTMLElement, parameter: ResourceParameter) {
  synchronizers.get(group)?.(parameter);
}
