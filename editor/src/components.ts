// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import './components.css';

const BUTTON_CLASS = 'joy-button';
const FIELD_CLASS = 'joy-field';
const NUMBER_STEPPER_CLASS = 'joy-number-stepper';
const PANEL_HEADER_CLASS = 'joy-panel-header';
const TOOLBAR_CLASS = 'joy-toolbar';
const STATUS_BAR_CLASS = 'joy-status-bar';

/** Applies the shared component vocabulary to existing editor markup. */
export function applyJoyComponentClasses(root: ParentNode = document): void {
  root.querySelectorAll('button').forEach(element => element.classList.add(BUTTON_CLASS));
  root.querySelectorAll('input, select, textarea').forEach(element => element.classList.add(FIELD_CLASS));
  root.querySelectorAll("input[type='number']").forEach(element => element.classList.add(NUMBER_STEPPER_CLASS));
  root.querySelectorAll('.panel-heading, .panel-header').forEach(element => element.classList.add(PANEL_HEADER_CLASS));
  root.querySelectorAll('.toolbar, .stage-toolbar, [role="toolbar"]').forEach(element => element.classList.add(TOOLBAR_CLASS));
  root.querySelectorAll('.status-bar, footer').forEach(element => element.classList.add(STATUS_BAR_CLASS));
}

/** Owns shared component classification for static and dynamically-authored controls. */
export class JoyComponentRegistry {
  readonly observer: MutationObserver;

  constructor(private readonly root: Document | HTMLElement = document) {
    applyJoyComponentClasses(root);
    this.observer = new MutationObserver(records => {
      for(const record of records) {
        for(const node of record.addedNodes) {
          if(node instanceof HTMLElement) {
            classifyElement(node);
          }
        }
      }
    });
    this.observer.observe(root, {childList: true, subtree: true});
  }

  destroy(): void {
    this.observer.disconnect();
  }
}

function classifyElement(element: HTMLElement): void {
  if(element.matches('button')) {
    element.classList.add(BUTTON_CLASS);
  }
  if(element.matches('input, select, textarea')) {
    element.classList.add(FIELD_CLASS);
  }
  if(element.matches("input[type='number']")) {
    element.classList.add(NUMBER_STEPPER_CLASS);
  }
  if(element.matches('.panel-heading, .panel-header')) {
    element.classList.add(PANEL_HEADER_CLASS);
  }
  if(element.matches('.toolbar, .stage-toolbar, [role="toolbar"]')) {
    element.classList.add(TOOLBAR_CLASS);
  }
  if(element.matches('.status-bar, footer')) {
    element.classList.add(STATUS_BAR_CLASS);
  }
  applyJoyComponentClasses(element);
}
