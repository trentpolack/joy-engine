// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { BROWSER_EVENT, KEY_CODE } from 'joy-engine/constants';

/**
 * Own a preview's fullscreen button, focus, and listeners. The panel and button
 * are borrowed. Native fullscreen is preferred; denial or unavailable APIs leave
 * a window-filling preview with the same exit controls. Call destroy() on teardown.
 */
export class PreviewFullscreen {
  declare panel: HTMLElement;
  declare button: HTMLElement;
  declare document: Document;
  declare listeners: AbortController;
  declare expanded: boolean;
  declare nativeEntered: boolean;
  declare disposed: boolean;
  declare previousOverflow: string;
  declare background: { element: HTMLElement; inert: boolean; }[];
  declare previousFocus: HTMLElement | null;

  /**
   * @param panel
   * @param button
   */
  constructor(panel: HTMLElement, button: HTMLElement) {
    this.panel = panel;
    this.button = button;
    this.document = panel.ownerDocument;
    this.listeners = new AbortController();
    this.expanded = false;
    this.nativeEntered = false;
    this.disposed = false;
    this.previousOverflow = '';

    this.background = [];

    this.previousFocus = null;

    const signal = this.listeners.signal;
    button.setAttribute('aria-controls', panel.id);
    button.addEventListener(BROWSER_EVENT.CLICK, () => void this.toggle(), {signal});
    this.document.addEventListener('fullscreenchange', () => {
      if(this.document.fullscreenElement === this.panel) {
        this.nativeEntered = true;
      } else if(this.nativeEntered) {
        this.nativeEntered = false;
        this.setExpanded(false);
      }
    }, {signal});
    this.document.addEventListener(BROWSER_EVENT.KEYDOWN, event => {
      if(event.key === KEY_CODE.ESCAPE && this.expanded) {
        event.preventDefault();
        void this.toggle();
      }
    }, {signal});
    this.updateButton();
  }

  /** Enter or exit without changing the preview's camera, document, or playback. */
  async toggle() {
    if(this.disposed) {
      return;
    }
    if(this.expanded) {
      this.setExpanded(false);
      await this.exitNative();
      return;
    }
    this.setExpanded(true);
    if(this.panel.requestFullscreen && this.document.fullscreenEnabled) {
      try {
        await this.panel.requestFullscreen();
        if(this.disposed || !this.expanded) {
          await this.exitNative();
        }
      } catch {
        // Embedded browsers can deny fullscreen; the window-filling view remains usable.
      }
    }
  }

  /** Restore borrowed DOM state and release every listener; safe to call twice. */
  destroy() {
    if(this.disposed) {
      return;
    }
    this.disposed = true;
    this.listeners.abort();
    this.setExpanded(false);
    void this.exitNative();
  }

  /**
   * Preserve focus, overflow, and background interactivity across native and fallback expansion.
   * @private
   * @param expanded
   */
  private setExpanded(expanded: boolean) {
    if(this.expanded === expanded) {
      return;
    }
    this.expanded = expanded;
    if(expanded) {
      const focused = this.document.activeElement;
      this.previousFocus = focused instanceof HTMLElement ? focused : null;
      this.previousOverflow = this.document.documentElement.style.overflow;
      this.document.documentElement.style.overflow = 'hidden';
      // Inert siblings at each ancestor keep keyboard navigation within the preview,
      // including the fallback where the browser does not isolate a fullscreen tree.
      for(let branch = this.panel; branch.parentElement; branch = branch.parentElement) {
        for(const sibling of branch.parentElement.children) {
          if(sibling !== branch && sibling instanceof HTMLElement) {
            this.background.push({element: sibling, inert: sibling.inert});
            sibling.inert = true;
          }
        }
      }
    } else {
      for(const {element, inert} of this.background) {
        element.inert = inert;
      }
      this.background = [];
      this.document.documentElement.style.overflow = this.previousOverflow;
    }
    this.panel.classList.toggle('joy-preview-fullscreen', expanded);
    this.updateButton();
    if(expanded) {
      this.button.focus({preventScroll: true});
    } else if(this.previousFocus?.isConnected) {
      this.previousFocus.focus({preventScroll: true});
    }
  }

  /** Keep the borrowed button label and accessibility state in sync. @private */
  private updateButton() {
    this.button.textContent = this.expanded ? 'Exit fullscreen' : 'Fullscreen';
    this.button.setAttribute('aria-pressed', String(this.expanded));
    this.button.title = this.expanded ? 'Exit preview fullscreen (Escape)' : 'Expand preview to fullscreen';
  }

  /** Exit only this preview’s native fullscreen session; unrelated fullscreen owners are retained. @private */
  private async exitNative() {
    if(this.document.fullscreenElement === this.panel) {
      try {
        await this.document.exitFullscreen();
      } catch {
        // The browser may already be exiting in response to Escape or page teardown.
      }
    }
  }
}
