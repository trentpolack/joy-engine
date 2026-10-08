// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { PREVIEW_STATE } from 'joy-engine/constants';
import { PREVIEW_HOST_MESSAGE } from './constants.ts';
import {PLAY_MODE, PREVIEW_CHANNEL} from './preview-protocol.ts';
export type PlayMode = 'docked' | 'editor' | 'browser' | 'window';
export interface PreviewState {
  state: string;
  message: string;
  running: boolean;
  connected: boolean;
  paused?: boolean;
  pendingWorld?: boolean;
  mode: PlayMode;
}

/**
 * Owns one game frame or external preview host. The workspace owns this session.
 * Only committed assets cross this boundary; runtime adapters retain game policy. */
export class GamePreviewSession {
  declare stage: HTMLElement;
  declare changed: (state: PreviewState) => void;
  declare frame: HTMLIFrameElement | null;
  declare external: Window | null;
  declare mode: PlayMode;
  declare id: string;
  declare running: boolean;
  declare connected: boolean;
  declare independent: boolean;
  declare timeout: number;
  declare lastHostMessage: number;
  declare monitor: number;

  /** @param stage @param changed */
  constructor(stage: HTMLElement, changed: (state: PreviewState) => void) {
    this.stage = stage;
    this.changed = changed;

    this.frame = null;

    this.external = null;

    this.mode = PLAY_MODE.EDITOR;
    this.id = '';
    this.running = false;
    this.connected = false;
    this.independent = false;
    this.timeout = 0;
    this.lastHostMessage = 0;
    this.monitor = window.setInterval(() => {
      if(this.external?.closed) {
        this.stop();
      } else if(this.external && this.connected && Date.now() - this.lastHostMessage > 4000) {
        this.connected = false;
        this.report(PREVIEW_STATE.DISCONNECTED, 'Game host connection was lost. Close or Restart the preview before applying saved changes.');
      }
    }, 500);
  }

  /**
   * Start only from a user gesture; repeated Play focuses the existing endpoint.
   * @param project @param mode
   * @param size Requested CSS content dimensions. */
  start(project: {
    name: string;
    preview: string | null;
}, mode: PlayMode, size: {
    width: number;
    height: number;
} | null) {
    if(this.running) {
      this.focus();
      return;
    }
    if(!project.preview || !/^\/games\/[a-z0-9-]+\/$/.test(project.preview)) {
      this.report('UNAVAILABLE', 'This project has no supported local game preview.');
      return;
    }
    this.mode = mode;
    this.id = crypto.randomUUID();
    this.connected = false;
    this.independent = false;
    this.lastHostMessage = Date.now();
    if(mode === PLAY_MODE.DOCKED || mode === PLAY_MODE.EDITOR) {
      this.frame = document.createElement('iframe');
      this.frame.title = `${project.name} game preview`;
      this.frame.src = `${project.preview}?joyEditor=1`;
      this.stage.append(this.frame);
    } else {
      const url = new URL('preview.html', document.baseURI);
      url.search = new URLSearchParams({game: project.preview, session: this.id, mode}).toString();
      if(mode === PLAY_MODE.BROWSER && window.joyDesktop?.openBrowser) {
        this.independent = true;
        this.running = true;
        const launchId = this.id;
        void window.joyDesktop.openBrowser(url.href).then(() => {
          if(this.independent && this.id === launchId) {
            this.report('INDEPENDENT BROWSER', 'Close the external browser tab, then use Finish External Preview. Editor Pause/Stop and live saves are unavailable in this mode.');
          }
        }).catch(error => {
          if(this.id !== launchId) { return; }
          this.independent = false;
          this.stop();
          this.report('FAILED', String(error));
        });
        return;
      }
      const requested = size ?? {width: Math.min(1280, screen.availWidth - 80), height: Math.min(800, screen.availHeight - 120)};
      const width = Math.max(320, Math.min(requested.width, screen.availWidth - 40));
      const height = Math.max(240, Math.min(requested.height, screen.availHeight - 100));
      if(mode === PLAY_MODE.WINDOW) {
        url.searchParams.set('width', String(requested.width));
        url.searchParams.set('height', String(requested.height));
      }
      this.external = window.open(url.href, '_blank', mode === PLAY_MODE.WINDOW ? `popup,width=${width},height=${height}` : undefined);
      if(!this.external) {
        this.report('BLOCKED', 'Game window was blocked. Allow popups and retry Play, or select Joy Editor Tab / Docked.');
        return;
      }
    }
    this.running = true;
    this.report('CONNECTING…', 'Waiting for the game. Preview uses saved content.');
    this.timeout = window.setTimeout(() => {
      if(!this.connected) {
        this.report('RUNNING · UNCONNECTED', 'No runtime acknowledgement. Pause and live application are unavailable; Restart uses saved content.');
      }
    }, 60000);
  }

  /**
   * Match the owned endpoint before accepting runtime state. @param event */
  receive(event: MessageEvent) {
    if(!this.running || event.origin !== location.origin || !event.data) {
      return false;
    }
    const direct = event.source === this.frame?.contentWindow && event.data.channel === PREVIEW_CHANNEL.RUNTIME;
    const host = event.source === this.external && event.data.channel === PREVIEW_CHANNEL.SESSION && event.data.session === this.id;
    if(host && (event.data.type === PREVIEW_HOST_MESSAGE.ALIVE || event.data.type === PREVIEW_HOST_MESSAGE.DISCONNECTED)) {
      this.lastHostMessage = Date.now();
      if(event.data.type === PREVIEW_HOST_MESSAGE.DISCONNECTED) {
        this.connected = false;
        this.report(PREVIEW_STATE.DISCONNECTED, 'Game host left its preview page. Restart or Stop this preview.');
      }
      return true;
    }
    const forwarded = host && event.data.type === PREVIEW_HOST_MESSAGE.RUNTIME;
    if(!direct && !forwarded) {
      return false;
    }
    const data = direct ? event.data : event.data.data;
    if(!data || typeof data !== 'object' || data.channel !== PREVIEW_CHANNEL.RUNTIME) {
      return true;
    }
    this.connected = typeof data.paused === 'boolean';
    this.lastHostMessage = Date.now();
    clearTimeout(this.timeout);
    this.changed({state: String(data.state ?? PREVIEW_STATE.CONNECTED), message: String(data.message ?? ''),
      running: this.running, connected: this.connected, mode: this.mode,
      paused: data.paused, pendingWorld: data.pendingWorld === true});
    return true;
  }

  /** @param data */
  send(data: Record<string, unknown>) {
    if(!this.running || this.independent) {
      return;
    }
    const message = {channel: PREVIEW_CHANNEL.COMMAND, ...data};
    this.frame?.contentWindow?.postMessage(message, location.origin);
    this.external?.postMessage({channel: PREVIEW_CHANNEL.SESSION, session: this.id, type: PREVIEW_HOST_MESSAGE.COMMAND, data: message}, location.origin);
  }

  /** Focus an existing destination without creating a second simulation. */
  focus() {
    if(this.independent) {
      this.report('INDEPENDENT BROWSER', 'The system browser owns this tab. Close it before finishing the external preview.');
      return;
    }
    this.external?.focus();
    this.frame?.contentWindow?.focus();
  }

  /**
   * Release a managed endpoint. Return false when closure cannot be confirmed. */
  stop() {
    if(this.independent) {
      return false;
    }
    if(this.external && !this.external.closed) {
      this.external.close();
      if(!this.external.closed) {
        this.report('STOPPING…', 'Waiting for the game window to close. Close it manually if necessary.');
        return false;
      }
    }
    clearTimeout(this.timeout);
    this.frame?.remove();
    this.frame = null;
    this.external = null;
    this.id = '';
    this.running = false;
    this.connected = false;
    this.report('PREVIEW STOPPED', 'Asset auditions remain in their editors. Play starts a fresh game from saved content.');
    return true;
  }

  /** Call only after the user confirms their independent browser tab is closed. */
  finishIndependent() {
    this.independent = false;
    return this.stop();
  }

  /** Release subscriptions; the caller handles independent-window close instructions. */
  destroy() {
    this.stop();
    clearInterval(this.monitor);
  }

  /** @private @param state @param message */
  private report(state: string, message: string) {
    this.changed({state, message, running: this.running, connected: this.connected, mode: this.mode});
  }
}
