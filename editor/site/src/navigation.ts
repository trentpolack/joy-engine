// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { BROWSER_EVENT, KEY_CODE } from 'joy-engine/constants';

const TOOLS = [
  {
    slug: 'form-lab',
    enabled: import.meta.env.DEV || import.meta.env.JOY_FORM_LAB_ENABLED,
    shortcut: '1',
    name: 'FORM LAB',
    description: 'Geometry, point clouds, and materials.',
    formats: '.formlab · .form'
  },
  {
    slug: 'particle-lab',
    enabled: import.meta.env.DEV || import.meta.env.JOY_PARTICLE_LAB_ENABLED,
    shortcut: '2',
    name: 'PARTICLE LAB',
    description: 'Layered and scriptable particle effects.',
    formats: '.joyfx'
  }
].filter(tool => tool.enabled);

/**
 * Mount shared navigation without turning the focused tools into one monolithic app.
 * The returned owner must be destroyed when a hot-module session is replaced.
 * @param options
 */
export function mountEditorNavigation(options: {
    currentTool: string;
    openButton: HTMLButtonElement;
    saveButton: HTMLButtonElement;
}) {
  const header = document.querySelector('.app-header');
  const actions = header?.querySelector('nav[aria-label="Document actions"]');
  if(!(header instanceof HTMLElement) || !(actions instanceof HTMLElement)) {
    throw new Error('Joy Editor navigation requires the shared application header.');
  }

  const navigation = document.createElement('nav');
  navigation.className = 'joy-editor-navigation';
  navigation.setAttribute('aria-label', 'Joy Editor workspace');
  navigation.innerHTML = navigationMarkup(options.currentTool);
  header.insertBefore(navigation, actions);

  const listeners = new AbortController();
  const signal = listeners.signal;
  navigation.querySelector('[data-editor-action="open"]')?.addEventListener(BROWSER_EVENT.CLICK, () => {
    closeMenu(navigation);
    options.openButton.click();
  }, { signal });
  navigation.querySelector('[data-editor-action="save"]')?.addEventListener(BROWSER_EVENT.CLICK, () => {
    closeMenu(navigation);
    options.saveButton.click();
  }, { signal });
  document.addEventListener(BROWSER_EVENT.KEYDOWN, event => handleShortcut(event, navigation, options), { signal });
  document.addEventListener(BROWSER_EVENT.POINTERDOWN, event => {
    if(!navigation.contains(((event.target) as Node))) {
      closeMenu(navigation);
    }
  }, { signal });
  window.addEventListener(BROWSER_EVENT.PAGEHIDE, event => {
    if(!event.persisted) {
      listeners.abort();
      navigation.remove();
    }
  }, { signal });

  return {
    destroy() {
      listeners.abort();
      navigation.remove();
    }
  };
}

/** @param currentTool */
function navigationMarkup(currentTool: string) {
  const toolLinks = TOOLS.map(tool => {
    const current = tool.slug === currentTool;
    return `<a class="joy-editor-tool${current ? ' current' : ''}" href="../${tool.slug}/"${current ? ' aria-current="page"' : ''}>
      <span class="joy-editor-tool-key">${tool.shortcut}</span>
      <span><strong>${tool.name}</strong><small>${tool.description}</small><code>${tool.formats}</code></span>
    </a>`;
  }).join('');

  return `<details class="joy-editor-switcher">
    <summary><span class="joy-editor-grid" aria-hidden="true">+</span><span>TOOLS</span><span aria-hidden="true">⌄</span></summary>
    <div class="joy-editor-menu">
      <div class="joy-editor-menu-heading"><span>JOY EDITOR</span>${TOOLS.length > 1 ? '<small>ALT + 1–2 TO SWITCH</small>' : ''}</div>
      <div class="joy-editor-tools">${toolLinks}</div>
      <div class="joy-editor-file-actions">
        <button type="button" data-editor-action="open"><span>Open asset</span><kbd>⌘ O</kbd></button>
        <button type="button" data-editor-action="save"><span>Save asset</span><kbd>⌘ S</kbd></button>
      </div>
      <a class="joy-editor-home" href="../../../">All Joy Games experiments <span>↗</span></a>
    </div>
  </details>`;
}

/**
 * Keep document actions reachable from anywhere while leaving tool-specific keys alone.
 * @param event
 * @param navigation
 * @param options
 */
function handleShortcut(event: KeyboardEvent, navigation: HTMLElement, options: {
    openButton: HTMLButtonElement;
    saveButton: HTMLButtonElement;
}) {
  const key = event.key.toLowerCase();
  if((event.ctrlKey || event.metaKey) && !event.altKey && key === 'o') {
    event.preventDefault();
    options.openButton.click();
    return;
  }
  if((event.ctrlKey || event.metaKey) && !event.altKey && key === 's') {
    event.preventDefault();
    options.saveButton.click();
    return;
  }
  if(event.altKey && !event.ctrlKey && !event.metaKey) {
    const tool = TOOLS.find(candidate => candidate.shortcut === event.key);
    if(tool) {
      event.preventDefault();
      window.location.assign(`../${tool.slug}/`);
      return;
    }
  }
  if(event.key === KEY_CODE.ESCAPE) {
    closeMenu(navigation);
  }
}

/** @param navigation */
function closeMenu(navigation: HTMLElement) {
  const switcher = navigation.querySelector('details');
  if(switcher instanceof HTMLDetailsElement) {
    switcher.open = false;
  }
}
