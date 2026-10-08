// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { PREVIEW_CHANNEL, PREVIEW_COMMAND, PREVIEW_STATE } from '../runtime/preview-constants.ts';
import { BROWSER_EVENT } from '../browser/constants.ts';

/**
 * Connect a development game session to Joy Editor's same-origin preview host.
 * Games keep ownership of pause and content application policy; this helper
 * only validates the transport, reports state and releases its listener.
 * @param options
 */
export function connectEditorPreview({message, paused, onPause, onAsset, onRegenerate}: {
    message: string;
    paused: () => boolean;
    onPause: () => void;
    onAsset?: (path: string, text: string) => void;
    onRegenerate?: () => void;
}) {
  const listeners = new AbortController();

  /** @param state @param detail */
  function report(state: string, detail: string) {
    window.parent.postMessage({
      channel: PREVIEW_CHANNEL.RUNTIME,
      state,
      message: detail,
      paused: paused(),
      pendingWorld: false
    }, location.origin);
  }

  /** @param event */
  function receive(event: MessageEvent<unknown>) {
    if(event.source !== window.parent || event.origin !== location.origin || !event.data || typeof event.data !== 'object') {
      return;
    }
    const data = ((event.data) as Record<string, unknown>);
    if(data.channel !== PREVIEW_CHANNEL.COMMAND) {
      return;
    }
    try {
      if(data.type === PREVIEW_COMMAND.PAUSE) {
        onPause();
        report(PREVIEW_STATE.READY, message);
      } else if(data.type === PREVIEW_COMMAND.REGENERATE) {
        onRegenerate?.();
        report(PREVIEW_STATE.READY, message);
      } else if(data.type === PREVIEW_COMMAND.ASSET && typeof data.path === 'string' && typeof data.text === 'string') {
        onAsset?.(data.path, data.text);
        report(PREVIEW_STATE.READY, message);
      }
    } catch(error) {
      report(PREVIEW_STATE.CONTENT_ERROR, `${error instanceof Error ? error.message : String(error)} Previous game content retained.`);
    }
  }

  window.addEventListener(BROWSER_EVENT.MESSAGE, receive, {signal: listeners.signal});
  report(PREVIEW_STATE.READY, message);
  return () => listeners.abort();
}
