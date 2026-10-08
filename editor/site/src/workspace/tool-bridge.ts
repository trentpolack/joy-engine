// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { WORKSPACE_CHANNEL, WORKSPACE_MESSAGE } from './constants.ts';
import { BROWSER_EVENT } from 'joy-engine/constants';
import '../../css/workspace/hosted.css';

/** Same-origin workspace operations; asset paths are project-relative. */
export interface WorkspaceRequestResults {
  catalog: Array<{path: string; kind: string}>;
  read: string;
  create: {text: string; revision: string};
  open: boolean;
  'focus-search': boolean;
}

/** The tool owns this connection and releases it when its document frame closes. */
export interface WorkspaceToolConnection {
  request<Operation extends keyof WorkspaceRequestResults>(operation: Operation, data?: Record<string, unknown>): Promise<WorkspaceRequestResults[Operation]>;
  destroy(): void;
}
export const hosted = window.parent !== window && new URLSearchParams(location.search).has('workspace');
let active = true;
/**
 * Whether this tool frame should submit preview work; standalone tools stay active. */
export function workspaceActive() {
  return active;
}
if(hosted) {
  document.documentElement.classList.add('joy-hosted');
}

/** Request temporary layout expansion from the parent; it validates the owning frame.
 * @param maximized
 */
export function requestWorkspaceMaximize(maximized: boolean) {
  if(hosted) {
    window.parent.postMessage({channel: WORKSPACE_CHANNEL.TOOL, type: WORKSPACE_MESSAGE.MAXIMIZE_WORKSPACE, maximized}, location.origin);
  }
}

/**
 * Adapt an existing document editor to a workspace without sharing its mutable state.
 * The caller owns disposal. Only the same-origin parent can load or save documents.
 * @param adapter
 */
export function connectWorkspaceTool(adapter: {
    load: (path: string, text: string) => void;
    serialize: () => string;
    assetsChanged?: () => void;
    deferFocusedInputs?: boolean;
    maximized?: (value: boolean) => void;
}): WorkspaceToolConnection {
  if(!hosted) {
    return {destroy() {}, request: async (_operation: string,  _data: Record<string, unknown> = {}) => { throw new Error('Open this document in the Joy Editor workspace to browse project assets.'); }};
  }
  const listeners = new AbortController();

  const requests: Map<string, {
    resolve: (value: unknown) => void;
    reject: (error: Error) => void;
    timer: number;
}> = new Map();
  let path = '';
  let snapshot = '';
  let loaded = false;
  const send = (type: string,  data: Record<string, unknown> = {}) => {
    window.parent.postMessage({channel: WORKSPACE_CHANNEL.TOOL, type, path, ...data}, location.origin);
  };
  const capture = (passive = false) => {
    const focus = document.activeElement;
    if(passive && adapter.deferFocusedInputs && (focus instanceof HTMLInputElement || focus instanceof HTMLTextAreaElement)) {
      return false;
    }
    if(!loaded) {
      return false;
    }
    try {
      const text = adapter.serialize();
      if(text !== snapshot) {
        snapshot = text;
        send(WORKSPACE_MESSAGE.CHANGED, {text});
      }
      return true;
    } catch(error) {
      if(!passive) {
        send(WORKSPACE_MESSAGE.ERROR, {message: error instanceof Error ? error.message : String(error)});
      }
      return false;
    }
  };
  window.addEventListener(BROWSER_EVENT.MESSAGE, event => {
    if(event.source !== window.parent || event.origin !== location.origin || event.data?.channel !== WORKSPACE_CHANNEL.HOST) {
      return;
    }
    const message = event.data;
    if(message.type === WORKSPACE_MESSAGE.RESPONSE) {
      const request = requests.get(message.request);
      if(request) {
        clearTimeout(request.timer);
        requests.delete(message.request);
        if(message.error) {
          request.reject(new Error(message.error));
        } else { request.resolve(message.result); }
      }
    } else if(message.type === WORKSPACE_MESSAGE.WORKSPACE_MAXIMIZED) {
      adapter.maximized?.(message.maximized === true);
    } else if(message.type === WORKSPACE_MESSAGE.ASSETS_CHANGED) {
      adapter.assetsChanged?.();
    } else if(message.type === WORKSPACE_MESSAGE.LOAD && typeof message.path === 'string' && typeof message.text === 'string') {
      loaded = false;
      path = message.path;
      try {
        adapter.load(path, message.text);
        snapshot = adapter.serialize();
        loaded = true;
        send(WORKSPACE_MESSAGE.LOADED);
      } catch(error) {
        send(WORKSPACE_MESSAGE.ERROR, {phase: 'load', message: error instanceof Error ? error.message : String(error)});
      }
    } else if(message.type === WORKSPACE_MESSAGE.ACTIVE) {
      active = Boolean(message.active);
    } else if(message.type === WORKSPACE_MESSAGE.CAPTURE) {
      const ok = capture(message.passive === true);
      send(WORKSPACE_MESSAGE.CAPTURED, {request: message.request, ok});
    }
  }, {signal: listeners.signal});
  // Capture before navigation or save; polling also catches inspector-only edits.
  const timer = window.setInterval(() => capture(true), 200);
  document.addEventListener(BROWSER_EVENT.INPUT, () => capture(true), {capture: false, signal: listeners.signal});
  window.addEventListener(BROWSER_EVENT.KEYDOWN, event => {
    if((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'p') {
      event.preventDefault();
      event.stopImmediatePropagation();
      send(WORKSPACE_MESSAGE.FOCUS_SEARCH);
    }
    if((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 's') {
      event.preventDefault();
      event.stopImmediatePropagation();
      capture();
      send(WORKSPACE_MESSAGE.SAVE);
    }
  }, {capture: true, signal: listeners.signal});
  send(WORKSPACE_MESSAGE.READY);
  return {
    /**
     * Request a bounded operation from the owning workspace, never another frame.
     * @param operation @param [data] */
    request<Operation extends keyof WorkspaceRequestResults>(operation: Operation, data: Record<string, unknown> = {}): Promise<WorkspaceRequestResults[Operation]> {
      return new Promise((resolve, reject) => {
        const request = crypto.randomUUID();
        const timer = window.setTimeout(() => {
          requests.delete(request);
          reject(new Error('Workspace request timed out.'));
        }, 15000);
        // The parent owns operation execution; this frame has already checked its origin.
        requests.set(request, {resolve: resolve as (value: unknown) => void, reject, timer});
        send(WORKSPACE_MESSAGE.REQUEST, {operation, request, ...data});
      });
    },
    destroy() {
      for(const request of requests.values()) {
        clearTimeout(request.timer);
        request.reject(new Error('Editor closed.'));
      }
      requests.clear();
      clearInterval(timer);
      listeners.abort();
    }
  };
}
