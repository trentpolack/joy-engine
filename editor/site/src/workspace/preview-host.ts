// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { BROWSER_EVENT } from 'joy-engine/constants';
import { PREVIEW_HOST_MESSAGE } from './constants.ts';
import {PLAY_MODE, PREVIEW_CHANNEL} from './preview-protocol.ts';
// This page owns the child frame only; its opener owns the managed game session.
const query = new URLSearchParams(location.search);
const game = query.get('game') ?? '';
const session = query.get('session');
const owner = window.opener;
const note = document.querySelector('#preview-note');
if(!/^\/games\/[a-z0-9-]+\/$/.test(game) || !session) {
  throw new Error('Unsupported preview destination.');
}
const frame = document.createElement('iframe');
frame.title = 'Running game';
frame.src = `${game}?joyEditor=1`;
document.body.append(frame);
const listeners = new AbortController();

let runtime: Record<string, unknown> | null = null;
window.addEventListener(BROWSER_EVENT.MESSAGE, event => {
  if(event.origin !== location.origin || !event.data) {
    return;
  }
  if(event.source === frame.contentWindow && event.data.channel === PREVIEW_CHANNEL.RUNTIME) {
    runtime = event.data;
    owner?.postMessage({channel: PREVIEW_CHANNEL.SESSION, session, type: PREVIEW_HOST_MESSAGE.RUNTIME, data: event.data}, location.origin);
  } else if(owner && event.source === owner && event.data.channel === PREVIEW_CHANNEL.SESSION && event.data.session === session && event.data.type === PREVIEW_HOST_MESSAGE.COMMAND && event.data.data?.channel === PREVIEW_CHANNEL.COMMAND) {
    frame.contentWindow?.postMessage(event.data.data, location.origin);
  }
}, {signal: listeners.signal});
if(!owner && note instanceof HTMLElement) {
  note.hidden = false;
  note.textContent = 'Independent browser preview · editor controls and live content connection unavailable.';
}
function showSize() {
  if(query.get('mode') !== PLAY_MODE.WINDOW || !(note instanceof HTMLElement)) {
    return;
  }
  const width = Number(query.get('width'));
  const height = Number(query.get('height'));
  note.hidden = Math.abs(innerWidth - width) <= 8 && Math.abs(innerHeight - height) <= 8;
  note.textContent = `Requested ${width}×${height}; current window ${innerWidth}×${innerHeight}. Browser/display constraints may limit sizing. Use Fit to Screen if needed.`;
}
window.addEventListener(BROWSER_EVENT.RESIZE, showSize, {signal: listeners.signal});
showSize();
const monitor = window.setInterval(() => {
  owner?.postMessage({channel: PREVIEW_CHANNEL.SESSION, session, type: PREVIEW_HOST_MESSAGE.ALIVE}, location.origin);
  // Re-acknowledge the child's last reported state after background timer suspension.
  if(runtime) {
    owner?.postMessage({channel: PREVIEW_CHANNEL.SESSION, session, type: PREVIEW_HOST_MESSAGE.RUNTIME, data: runtime}, location.origin);
  }
  if(owner?.closed) {
    frame.remove();
    window.close();
  }
}, 1000);
window.addEventListener(BROWSER_EVENT.PAGEHIDE, event => {
  if(!event.persisted) {
    owner?.postMessage({channel: PREVIEW_CHANNEL.SESSION, session, type: PREVIEW_HOST_MESSAGE.DISCONNECTED}, location.origin);
    listeners.abort();
    clearInterval(monitor);
    frame.remove();
  }
});
