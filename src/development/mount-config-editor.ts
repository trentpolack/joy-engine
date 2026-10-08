// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { BROWSER_EVENT } from '../browser/constants.ts';
import type { ConfigField } from '../core/config-values.ts';
import type { InputController } from '../browser/input-controller.ts';
import { ConfigSession } from '../browser/config/config-session.ts';
import { ConfigPanel } from '../browser/config/config-panel.ts';
import { createConfigValues } from '../core/config-values.ts';

/**
 * Mount one development editor and its project-specific persistence connection.
 * The caller dynamically imports this module only in development, after game
 * startup. Returned destroy releases the view, subscriptions, and input hook.
 * @param options
 */
export async function mountConfigEditor({ title, fields, endpoint, onChange, input, onRestart, restartLabel }: {
    title: string;
    fields: readonly ConfigField[];
    endpoint: string;
    onChange: (values: Record<string, number>) => void;
    input?: InputController;
    onRestart?: () => void;
    restartLabel?: string;
}) {
  const initial = await requestConfig(endpoint);
  const values = createConfigValues(fields, initial.values);
  let revision = initial.revision;
  onChange(values);
  const session = new ConfigSession({
    fields,
    values,
    onChange,
    async save(nextValues: Record<string, number>) {
      const result = await requestConfig(endpoint, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json', 'X-Joy-Config': '1' },
        body: JSON.stringify({ revision, values: nextValues }),
      });
      revision = result.revision;
    },
  });
  const panel = new ConfigPanel({ title, session, onRestart, restartLabel });

  /**
   * Drop pre-existing held keys when focus enters the editor. Keyup remains
   * free to bubble; the panel stops its keydowns from reaching gameplay.
   * @param event
   */
  function clearGameplayInput(event: FocusEvent) {
    if(event.target instanceof HTMLElement && event.target.closest('.joy-config-tools')) {
      input?.keys.clear();
      if(input) {
        input.pointer.down = false;
      }
    }
  }
  document.addEventListener(BROWSER_EVENT.FOCUSIN, clearGameplayInput);
  return {
    session,
    panel,
    /** Release the mounted panel before its editing session. */
    destroy() {
      document.removeEventListener(BROWSER_EVENT.FOCUSIN, clearGameplayInput);
      panel.destroy();
      session.destroy();
    },
  };
}

/**
 * Fetch an uncached snapshot; server failures and malformed responses reject startup or saving.
 * @param endpoint
 * @param [options]
 */
async function requestConfig(endpoint: string, options?: RequestInit) {
  const response = await fetch(endpoint, { ...options, cache: 'no-store' });
  const result = await response.json();
  if(!response.ok) {
    throw new Error(result.error ?? `Config request failed (${response.status}).`);
  }
  if(typeof result.revision !== 'string' || !result.values || typeof result.values !== 'object') {
    throw new Error('The development server returned an invalid config snapshot.');
  }
  return  ((result) as {
    revision: string;
    values: Record<string, number>;
});
}
