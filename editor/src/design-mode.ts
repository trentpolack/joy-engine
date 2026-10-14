// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import './design-mode.css';
import {applyJoyComponentClasses} from './components.ts';

const DESIGN_ENDPOINT = '/__joy-design/css';
const TOKEN_PREFIX = '--joy-';
const DESIGN_CONTROLS = '.joy-design-panel, .joy-design-toggle';
const COMPARISON_QUERY = 'joy-compare-tools';
const COMPARISON_FRAME_QUERY = 'joy-comparison-frame';
const SUPPRESS_HOVER_ATTRIBUTE = 'data-joy-hover-suppressed';

export interface DesignModeOptions {
  /** Borrowed header container; the launcher participates in its normal layout. */
  toggleHost: HTMLElement;
  compareUrls?: string[];
}

/**
 * Owns the development-only inspector, launcher, and listeners until destroy().
 */
export class JoyDesignMode {
  readonly listeners = new AbortController();
  readonly panel = document.createElement('aside');
  readonly outline = document.createElement('div');
  readonly toggle = document.createElement('button');
  readonly content = document.createElement('div');
  readonly modeLabel = document.createElement('div');
  comparison: HTMLDivElement | null = null;
  selected: HTMLElement | null = null;

  constructor(private readonly options: DesignModeOptions) {
    applyJoyComponentClasses();
    this.panel.className = 'joy-design-panel';
    this.panel.hidden = true;
    this.panel.setAttribute('aria-label', 'Joy design mode');
    this.outline.className = 'joy-design-outline';
    this.outline.hidden = true;

    const heading = document.createElement('div');
    heading.className = 'joy-design-heading';
    const title = document.createElement('h2');
    title.textContent = 'Design mode';
    const close = document.createElement('button');
    close.type = 'button';
    close.className = 'joy-button joy-button--quiet';
    close.textContent = 'Close';
    close.addEventListener('click', () => this.setOpen(false), {signal: this.listeners.signal});
    heading.append(title, close);

    this.modeLabel.className = 'joy-design-mode-label';
    this.modeLabel.textContent = 'DESIGN MODE – Escape to Exit';
    this.modeLabel.hidden = true;
    const compare = document.createElement('button');
    compare.type = 'button';
    compare.className = 'joy-button joy-button--quiet';
    compare.textContent = 'COMPARE TOOLS';
    compare.hidden = !this.options.compareUrls?.length || !new URLSearchParams(window.location.search).has(COMPARISON_QUERY);
    compare.addEventListener('click', () => this.showComparison(), {signal: this.listeners.signal});
    this.panel.append(heading, compare, this.content);

    this.toggle.className = 'joy-button joy-button--primary joy-design-toggle';
    this.toggle.type = 'button';
    this.toggle.textContent = 'DESIGN';
    this.toggle.setAttribute('aria-expanded', 'false');
    this.toggle.addEventListener('click', () => {
      this.toggle.removeAttribute(SUPPRESS_HOVER_ATTRIBUTE);
      this.setOpen(this.toggle.getAttribute('aria-expanded') !== 'true');
    }, {signal: this.listeners.signal});
    const restoreHover = () => this.toggle.removeAttribute(SUPPRESS_HOVER_ATTRIBUTE);
    this.toggle.addEventListener('pointerleave', restoreHover, {signal: this.listeners.signal});
    this.toggle.addEventListener('pointerdown', restoreHover, {signal: this.listeners.signal});
    this.options.toggleHost.append(this.toggle);
    document.body.append(this.outline, this.panel, this.modeLabel);
    document.addEventListener('keydown', event => {
      if(event.key === 'Escape' && this.toggle.getAttribute('aria-expanded') === 'true') {
        event.preventDefault();
        event.stopPropagation();
        this.setOpen(false);
        // Escape exits the active presentation even when the pointer is still on the launcher.
        this.toggle.toggleAttribute(SUPPRESS_HOVER_ATTRIBUTE, this.toggle.matches(':hover'));
        this.toggle.focus();
      }
    }, {capture: true, signal: this.listeners.signal});
    window.addEventListener('resize', () => {
      if(this.toggle.getAttribute('aria-expanded') === 'true') {
        this.positionPanel();
        this.updateOutline();
      }
    }, {signal: this.listeners.signal});
    document.addEventListener('pointerdown', event => this.select(event), {capture: true, signal: this.listeners.signal});
    // Cancel the subsequent click as well, so inspecting a link or button cannot
    // navigate away or invoke document actions after pointerdown is intercepted.
    document.addEventListener('click', event => {
      if(this.toggle.getAttribute('aria-expanded') === 'true' && event.target instanceof HTMLElement && !event.target.closest(DESIGN_CONTROLS)) {
        event.preventDefault();
        event.stopPropagation();
      }
    }, {capture: true, signal: this.listeners.signal});
  }

  /**
   * Remove this instance's interface and release its input subscriptions.
   */
  destroy(): void {
    this.listeners.abort();
    this.panel.remove();
    this.outline.remove();
    this.modeLabel.remove();
    this.comparison?.remove();
    this.toggle.remove();
  }

  private setOpen(open: boolean): void {
    this.panel.hidden = true;
    this.modeLabel.hidden = !open;
    document.documentElement.classList.toggle('joy-design-mode-active', open);
    this.toggle.setAttribute('aria-expanded', String(open));
    if(open) {
      return;
    }
    this.selected = null;
    this.outline.hidden = true;
    this.content.replaceChildren();
  }

  private positionPanel(): void {
    this.panel.style.maxHeight = `${Math.max(0, window.innerHeight - 48)}px`;
  }

  private select(event: PointerEvent): void {
    if(this.toggle.getAttribute('aria-expanded') !== 'true' || !(event.target instanceof HTMLElement) || event.target.closest(DESIGN_CONTROLS)) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    this.selected = event.target;
    this.updateOutline();
    void this.renderPanel();
  }

  private updateOutline(): void {
    if(!this.selected) {
      return;
    }
    const bounds = this.selected.getBoundingClientRect();
    Object.assign(this.outline.style, {left: `${bounds.left}px`, top: `${bounds.top}px`, width: `${bounds.width}px`, height: `${bounds.height}px`});
    this.outline.hidden = false;
  }

  private async renderPanel(): Promise<void> {
    if(!this.selected) {
      return;
    }
    const selected = this.selected;
    const match = await findSourceRule(selected);
    // Closing or selecting again can invalidate an in-flight stylesheet lookup.
    if(this.listeners.signal.aborted || this.toggle.getAttribute('aria-expanded') !== 'true' || this.selected !== selected) {
      return;
    }
    const tokens = readTokens();
    this.content.replaceChildren();
    const title = document.createElement('h3');
    title.textContent = this.selected.tagName.toLowerCase();
    const details = document.createElement('dl');
    details.innerHTML = `<dt>Component</dt><dd><code>${escapeHtml([...this.selected.classList].filter(name => name.startsWith('joy-')).join(' ') || 'unclassified')}</code></dd><dt>Classes</dt><dd>${escapeHtml(this.selected.className)}</dd><dt>Source</dt><dd>${escapeHtml(match?.source ?? 'browser / inline')}</dd><dt>Rule</dt><dd><code>${escapeHtml(match?.selector ?? 'none')}</code></dd>`;
    const tokenList = document.createElement('div');
    tokenList.className = 'joy-design-tokens';
    for(const [name, value] of tokens) {
      tokenList.append(Object.assign(document.createElement('code'), {textContent: name}), Object.assign(document.createElement('span'), {textContent: value}));
    }
    this.content.append(title, details, tokenList);
    this.panel.hidden = false;
    this.positionPanel();
    this.enableDrag(match);
  }

  private enableDrag(match: SourceRule | null): void {
    if(!this.selected || !match) {
      return;
    }
    this.selected.addEventListener('pointermove', event => {
      if(event.buttons !== 1 || !this.selected) {
        return;
      }
      const bounds = this.selected.getBoundingClientRect();
      void saveCss(match, {width: `${Math.max(1, Math.round(event.clientX - bounds.left))}px`});
    }, {once: true, signal: this.listeners.signal});
  }

  private showComparison(): void {
    if(this.comparison) {
      return;
    }
    const comparison = document.createElement('div');
    comparison.className = 'joy-design-compare';
    for(const url of this.options.compareUrls ?? []) {
      const frame = document.createElement('iframe');
      const frameUrl = new URL(url, window.location.href);
      frameUrl.searchParams.set(COMPARISON_FRAME_QUERY, '1');
      frame.src = frameUrl.href;
      frame.title = url;
      comparison.append(frame);
    }
    comparison.addEventListener('dblclick', () => {
      comparison.remove();
      this.comparison = null;
    }, {once: true});
    this.comparison = comparison;
    document.body.append(comparison);
  }
}

/** Comparison frames intentionally omit design controls so comparison cannot recurse. */
export function isDesignComparisonFrame(): boolean {
  return new URLSearchParams(window.location.search).has(COMPARISON_FRAME_QUERY);
}

interface SourceRule { href: string; selector: string; source: string; }

async function findSourceRule(element: HTMLElement): Promise<SourceRule | null> {
  for(const sheet of document.styleSheets) {
    try {
      for(const rule of sheet.cssRules) {
        if(rule instanceof CSSStyleRule && element.matches(rule.selectorText)) {
          const href = sheet.href ?? '';
          return {href, selector: rule.selectorText, source: await locateRule(href, rule.selectorText)};
        }
      }
    } catch {
      continue;
    }
  }
  return null;
}

async function locateRule(href: string, selector: string): Promise<string> {
  if(!href) {
    return `${window.location.pathname}:inline`;
  }
  try {
    const source = await fetch(href).then(response => response.text());
    const offset = source.indexOf(selector);
    const line = offset < 0 ? '?' : source.slice(0, offset).split('\n').length;
    return `${new URL(href).pathname}:${line}`;
  } catch {
    return `${new URL(href).pathname}:?`;
  }
}

function readTokens(): [string, string][] {
  const styles = getComputedStyle(document.documentElement);
  return [...styles].filter(name => name.startsWith(TOKEN_PREFIX)).sort().map(name => [name, styles.getPropertyValue(name).trim()]);
}

async function saveCss(rule: SourceRule, declarations: Record<string, string>): Promise<void> {
  await fetch(DESIGN_ENDPOINT, {method: 'PATCH', headers: {'content-type': 'application/json'}, body: JSON.stringify({href: rule.href, selector: rule.selector, declarations})});
}

function escapeHtml(value: string): string {
  return value.replace(/[&<>"']/g, character => `&#${character.charCodeAt(0)};`);
}
