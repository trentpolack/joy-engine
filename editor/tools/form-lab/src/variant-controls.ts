// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { BROWSER_EVENT } from 'joy-engine/constants';
import type { ParameterValue } from 'joy-engine/form';
import type { ParameterVariant } from './document.ts';
import { readOverrides, readVariants } from './document.ts';

/** Document-owned snapshots of explicit overrides. Script defaults remain live. */
export class VariantControls {
  declare listeners: AbortController;
  declare variants: ParameterVariant[];
  declare name: HTMLInputElement;
  declare select: HTMLSelectElement;
  declare status: HTMLElement;
  declare buttons: Element[];

  /** @param parent @param capture
   * @param apply @param changed
   * @param validate
   */
  constructor(parent: HTMLElement, capture: () => Record<string, ParameterValue>, apply: (values: Record<string, ParameterValue>) => void, changed: () => void, validate: (variants: ParameterVariant[]) => void) {
    this.listeners = new AbortController();

    this.variants = [];
    parent.innerHTML = `<p class="variant-intro">Keep different takes on the same script. Variants store parameter overrides, including mesh and material inputs; script defaults stay live.</p>
      <label class="variant-label" for="variant-name">NAME THIS TAKE</label>
      <input id="variant-name" maxlength="64" placeholder="Weathered, tall, winter…" />
      <button id="variant-save">Save new variant +</button>
      <label class="variant-label" for="variant-select">SAVED VARIANTS</label>
      <select id="variant-select" aria-label="Saved variants"></select>
      <div class="variant-actions"><button id="variant-apply">Apply</button><button id="variant-rename">Rename</button><button id="variant-delete">Delete</button></div>
      <p id="variant-status" role="status"></p>
      <p class="variant-intro">Apply replaces current overrides. Rename uses the name above. Your script and global material palette are shared by every variant. Save project to keep them together.</p>`;
    this.name = ((parent.querySelector('#variant-name')) as HTMLInputElement);
    this.select = ((parent.querySelector('#variant-select')) as HTMLSelectElement);
    this.status = ((parent.querySelector('#variant-status')) as HTMLElement);
    this.buttons = [...parent.querySelectorAll('.variant-actions button')];
    const signal = this.listeners.signal;
    /** @param id @param action */
    const connect = (id: string, action: () => void) => parent.querySelector(`#${id}`)?.addEventListener(BROWSER_EVENT.CLICK, () => {
      try {
        action();
      } catch (error) {
        this.status.textContent = error instanceof Error ? error.message : String(error);
      }
    }, {signal});
    connect('variant-save', () => {
      const next = readVariants([...this.variants, {name: this.name.value, overrides: capture()}]);
      validate(next);
      this.variants = next;
      this.render(next.length - 1);
      changed();
      this.status.textContent = 'Variant saved. Changes to current controls will not alter it.';
    });
    connect('variant-apply', () => {
      const variant = this.variants[Number(this.select.value)];
      if(variant) {
        apply(readOverrides(variant.overrides));
        this.status.textContent = `Applied ${variant.name}. Parameters removed from the script are ignored.`;
      }
    });
    connect('variant-rename', () => {
      const index = Number(this.select.value);
      const next = readVariants(this.variants.map((variant, i) => i === index ? {...variant, name: this.name.value} : variant));
      validate(next);
      this.variants = next;
      this.render(index);
      changed();
      this.status.textContent = 'Variant renamed.';
    });
    connect('variant-delete', () => {
      this.variants.splice(Number(this.select.value), 1);
      this.render();
      changed();
      this.status.textContent = 'Variant deleted. Current parameters are unchanged.';
    });
    this.render();
  }
  /** @param variants */
  setVariants(variants: ParameterVariant[]) {
    this.variants = readVariants(variants);
    this.name.value = '';
    this.status.textContent = '';
    this.render();
  }
  /** @param [selected] */
  render(selected: number = 0) {
    this.select.replaceChildren();
    for(const [index, variant] of this.variants.entries()) {
      this.select.add(new Option(variant.name, String(index)));
    }
    if(!this.variants.length) {
      this.select.add(new Option('No variants saved yet', ''));
    }
    this.select.selectedIndex = Math.min(selected, this.select.options.length - 1);
    this.select.disabled = !this.variants.length;
    for(const button of this.buttons) {
       ((button) as HTMLButtonElement).disabled = !this.variants.length;
    }
  }
  destroy() {
    this.listeners.abort();
  }
}
