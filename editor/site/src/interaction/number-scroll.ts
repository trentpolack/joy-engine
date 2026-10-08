// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { BROWSER_EVENT } from 'joy-engine/constants';
import {wheelPixels} from './viewport-input.ts';

/**
 * Keep panel scrolling from invoking native number-input stepping, including focused drafts.
 * The caller owns the subscription signal. Never blur, commit or rewrite a field.
 * @param root @param signal */
export function guardNumberScroll(root: Document, signal: AbortSignal) {
  root.addEventListener(BROWSER_EVENT.WHEEL, event => {
    if(!(event.target instanceof HTMLInputElement) || event.target.type !== 'number') {
      return;
    }
    event.preventDefault();
    const delta = wheelPixels(event, root.documentElement.clientHeight);
    let parent = event.target.parentElement;
    while(parent) {
      const style = getComputedStyle(parent);
      if(/auto|scroll/.test(style.overflowY) && parent.scrollHeight > parent.clientHeight &&
        (delta < 0 && parent.scrollTop > 0 || delta > 0 && parent.scrollTop + parent.clientHeight < parent.scrollHeight)) {
        parent.scrollBy({top: delta, left: event.deltaX, behavior: 'instant'});
        return;
      }
      parent = parent.parentElement;
    }
    root.scrollingElement?.scrollBy({top: delta, left: event.deltaX, behavior: 'instant'});
  }, {signal, passive: false});
}
