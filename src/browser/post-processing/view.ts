// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { PostProcessingKey } from '../../rendering/postprocessor/config.ts';

import styles from './editor.css?raw';
import launcherStyles from '../editor-launchers.css?raw';
import themeStyles from '../dev-widget-theme.css?raw';
import { POST_PROCESSING_SCHEMA } from '../../rendering/postprocessor/config.ts';
import { POST_PROCESSING_PRESETS } from '../../rendering/postprocessor/profile.ts';
import { requireElement } from '../dom.ts';

/**
 * Construct one editor's DOM. The owning editor removes the root and style.
 * @param title
 */
export function createPostProcessingView(title: string) {
  const root = document.createElement('aside');
  root.className = 'joy-post-editor';
  root.setAttribute('aria-label', 'Post-processing editor');
  root.innerHTML = `
    <button type="button" class="joy-post-toggle" aria-expanded="false">JOY-ENGINE</button>
    <section class="joy-post-panel" hidden>
      <div class="joy-post-heading"><strong>POST-PROCESSING</strong><button type="button" data-action="close" aria-label="Close post-processing">×</button></div>
      <p class="joy-post-project"></p>
      <p class="joy-post-status" role="status" aria-live="polite"></p>
      <div class="joy-post-toolbar">
        <button type="button" data-action="save">Save to Game</button>
        <button type="button" data-action="revert">Revert</button>
        <button type="button" data-action="undo">Undo</button>
        <button type="button" data-action="redo">Redo</button>
      </div>
      <div class="joy-post-conflict" hidden>
        <p>The file changed while you had local edits.</p>
        <button type="button" data-action="disk">Use Disk</button>
        <button type="button" data-action="local">Keep Local</button>
      </div>
      <div class="joy-post-preview">
        <label><input type="checkbox" data-mode="pause"> Pause Simulation</label>
        <label><input type="checkbox" data-mode="saved"> Compare Saved</label>
        <label><input type="checkbox" data-mode="bypass"> Bypass Effects</label>
      </div>
      <p class="joy-post-note">Bypass keeps a neutral display resolve. Preview modes are never saved.</p>
      <label class="joy-post-preset">Starting Preset <select aria-label="Starting Preset"></select></label>
      <p class="joy-post-note">Choosing a preset clears overrides. Undo restores your previous look.</p>
      <div class="joy-post-controls"></div>
      <div class="joy-post-footer">
        <button type="button" data-action="export">Export Preset</button>
        <button type="button" data-action="import">Import Profile</button>
        <input type="file" accept=".json,application/json" hidden>
        <p class="joy-post-note">Export saves a portable snapshot. Import previews it; Save to Game commits it.</p>
      </div>
    </section>
  `;
  requireElement(root, '.joy-post-project', HTMLElement).textContent = title;
  const preset = requireElement(root, 'select', HTMLSelectElement);
  for(const name of Object.keys(POST_PROCESSING_PRESETS)) {
    preset.add(new Option(name[0].toUpperCase() + name.slice(1), name));
  }
  const controls = requireElement(root, '.joy-post-controls', HTMLElement);

  const fields: Map<PostProcessingKey, {
    input: HTMLInputElement | HTMLSelectElement;
    slider: HTMLInputElement | null;
    source: HTMLElement;
    reset: HTMLButtonElement;
}> = new Map();
  let currentGroup = '';
  let group = controls;
  for(const control of POST_PROCESSING_SCHEMA) {
    if(control.group !== currentGroup) {
      currentGroup = control.group;
      group = document.createElement('fieldset');
      const legend = document.createElement('legend');
      legend.textContent = control.group.toUpperCase();
      group.append(legend);
      controls.append(group);
    }
    const row = document.createElement('div');
    row.className = 'joy-post-field';
    row.dataset.settingRow = control.key;
    row.title = control.description;
    const label = document.createElement('label');
    label.textContent = control.label;
    const source = document.createElement('small');
    source.className = 'joy-post-source';
    const input = control.choices ? document.createElement('select') : document.createElement('input');
    input.dataset.setting = control.key;
    input.setAttribute('aria-label', `${control.group} ${control.label}`);
    input.title = control.description;
    let slider = null;
    if(input instanceof HTMLSelectElement) {
      for(const choice of control.choices ?? []) {
        input.add(new Option(choice.toUpperCase(), choice));
      }
    } else {
      input.type = 'number';
      input.step = String(control.step);
      input.min = String(control.exclusiveMinimum ? control.step : control.minimum);
      if(control.maximum !== undefined) {
        input.max = String(control.maximum);
      }
      slider = document.createElement('input');
      slider.type = 'range';
      slider.min = input.min;
      slider.max = String(control.sliderMaximum);
      slider.step = input.step;
      slider.dataset.slider = control.key;
      slider.setAttribute('aria-label', `${control.group} ${control.label} Slider`);
    }
    label.append(input);
    const reset = document.createElement('button');
    reset.type = 'button';
    reset.textContent = 'Reset';
    reset.setAttribute('aria-label', `Reset ${control.group} ${control.label}`);
    row.append(label, source, reset);
    if(slider) {
      row.append(slider);
    }
    group.append(row);
    fields.set(control.key, { input, slider, source, reset });
  }
  const style = document.createElement('style');
  style.textContent = `${themeStyles}\n${styles}\n${launcherStyles}`;
  document.head.append(style);
  document.body.append(root);
  return {
    root, style, fields, preset,
    panel: requireElement(root, '.joy-post-panel', HTMLElement),
    toggle: requireElement(root, '.joy-post-toggle', HTMLButtonElement),
    status: requireElement(root, '.joy-post-status', HTMLElement),
    pause: requireElement(root, 'input[data-mode="pause"]', HTMLInputElement),
    conflict: requireElement(root, '.joy-post-conflict', HTMLElement),
    file: requireElement(root, 'input[type="file"]', HTMLInputElement),
    actions: new Map(Array.from(root.querySelectorAll('button[data-action]')).map(button => {
      const element = ((button) as HTMLButtonElement);
      return [element.dataset.action ?? '', element];
    })),
  };
}
