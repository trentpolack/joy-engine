// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

export const EDITOR_PANEL_OPEN_EVENT = 'joy:editor-panel-open';

/**
 * Notify sibling development tools that one editor is opening.
 * @param owner Root element owned by the opening editor.
 */
export function announceEditorPanelOpen(owner: HTMLElement) {
  document.dispatchEvent(new CustomEvent(EDITOR_PANEL_OPEN_EVENT, { detail: owner }));
}

/**
 * Identify notifications from sibling editors so the current panel can close.
 * @param event
 * @param owner Borrowed root of the editor handling the notification.
 */
export function isAnotherEditorOpening(event: Event, owner: HTMLElement) {
  return event instanceof CustomEvent && event.detail !== owner;
}
