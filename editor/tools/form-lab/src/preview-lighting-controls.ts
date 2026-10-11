// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import {BROWSER_EVENT} from 'joy-engine/constants';
import {DEFAULT_PREVIEW_LIGHTING, PREVIEW_LIGHTING_FIELDS} from './rendering/preview-lighting.ts';
import type {PreviewLightingSettings} from './rendering/preview-lighting.ts';

/** Owns editor-only light controls; changes never mutate a document or material. */
export class PreviewLightingControls {
  private readonly listeners = new AbortController();
  private readonly settings: PreviewLightingSettings;

  constructor(container: HTMLElement, settings: PreviewLightingSettings, onChange: (settings: PreviewLightingSettings) => void) {
    this.settings = {...settings};
    const signal = this.listeners.signal;
    const message = document.createElement('p');
    message.className = 'numeric-guidance';
    const commit = () => {
      try {
        onChange({...this.settings});
        message.textContent = '';
      } catch(error) {
        message.textContent = error instanceof Error ? error.message : String(error);
      }
    };
    const inputs: HTMLInputElement[] = [];
    for(const field of PREVIEW_LIGHTING_FIELDS) {
      const label = document.createElement('label');
      label.className = 'parameter-row';
      const title = document.createElement('span');
      title.textContent = field.title;
      const input = document.createElement('input');
      input.type = 'number';
      input.id = `preview-${field.key}`;
      input.min = String(field.min);
      input.max = String(field.max);
      input.step = String(field.step);
      input.value = String(settings[field.key]);
      input.addEventListener(BROWSER_EVENT.CHANGE, () => {
        this.settings[field.key] = input.valueAsNumber;
        commit();
      }, {signal});
      inputs.push(input);
      label.append(title, input);
      container.append(label);
    }
    const casting = document.createElement('input');
    casting.id = 'preview-castsShadow';
    casting.type = 'checkbox';
    casting.checked = settings.castsShadow;
    const label = document.createElement('label');
    label.className = 'parameter-row';
    label.append('Cast shadows', casting);
    casting.addEventListener(BROWSER_EVENT.CHANGE, () => {
      this.settings.castsShadow = casting.checked;
      commit();
    }, {signal});
    const quality = document.createElement('select');
    quality.id = 'preview-quality';
    for(const value of ['low', 'balanced', 'cinematic'] as const) {
      const option = document.createElement('option');
      option.value = value;
      option.textContent = value;
      quality.append(option);
    }
    quality.value = settings.quality;
    quality.setAttribute('aria-label', 'Shadow quality');
    quality.addEventListener(BROWSER_EVENT.CHANGE, () => {
      this.settings.quality = quality.value as PreviewLightingSettings['quality'];
      commit();
    }, {signal});
    const reset = document.createElement('button');
    reset.id = 'preview-reset-lighting';
    reset.textContent = 'Reset preview lighting';
    reset.addEventListener(BROWSER_EVENT.CLICK, () => {
      Object.assign(this.settings, DEFAULT_PREVIEW_LIGHTING);
      inputs.forEach((input, index) => { input.value = String(this.settings[PREVIEW_LIGHTING_FIELDS[index].key]); });
      casting.checked = this.settings.castsShadow;
      quality.value = this.settings.quality;
      commit();
    }, {signal});
    container.append(label, quality, reset, message);
  }

  destroy() {
    this.listeners.abort();
  }
}
