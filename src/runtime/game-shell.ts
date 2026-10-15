// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { BROWSER_EVENT } from '../browser/constants.ts';
import { DevTools } from '../browser/diagnostics/dev-tools.ts';

export interface GameShellResource {
  destroy(): void;
}

export interface GameShellFrame<TConfiguration, TRenderer, TInput> {
  configuration: TConfiguration;
  renderer: TRenderer;
  input: TInput;
  devTools: DevTools;
  elapsedSeconds: number;
  timeSeconds: number;
  timestampMilliseconds: number;
}

export type GameShellPhase<TConfiguration, TRenderer, TInput> =
  (frame: GameShellFrame<TConfiguration, TRenderer, TInput>) => void;

export interface GameShellOptions<TConfiguration, TRenderer extends GameShellResource, TInput extends GameShellResource> {
  title: string;
  development: boolean;
  createConfiguration(): TConfiguration;
  createRenderer(configuration: TConfiguration): Promise<TRenderer>;
  createInput(renderer: TRenderer, configuration: TConfiguration): TInput;
  initialize?(shell: GameShell<TConfiguration, TRenderer, TInput>): Promise<void> | void;
  input?: GameShellPhase<TConfiguration, TRenderer, TInput>;
  simulation?: GameShellPhase<TConfiguration, TRenderer, TInput>;
  presentation?: GameShellPhase<TConfiguration, TRenderer, TInput>;
  render: GameShellPhase<TConfiguration, TRenderer, TInput>;
  afterFrame?: GameShellPhase<TConfiguration, TRenderer, TInput>;
  dispose?(shell: GameShell<TConfiguration, TRenderer, TInput>): void;
  fail(error: unknown): void;
  maximumFrameSeconds?: number;
}

const DEFAULT_MAXIMUM_FRAME_SECONDS = 0.1;

/**
 * Own one browser game's configuration, renderer, input, diagnostics, phased frame loop, and page lifecycle.
 * Factories transfer ownership of their returned resources to the shell. Frame phases run in input, simulation,
 * presentation, render, and after-frame order, using seconds for elapsed time and the animation-frame clock.
 */
export class GameShell<TConfiguration, TRenderer extends GameShellResource, TInput extends GameShellResource> {
  readonly devTools: DevTools;
  configuration: TConfiguration | null = null;
  renderer: TRenderer | null = null;
  input: TInput | null = null;

  private readonly options: GameShellOptions<TConfiguration, TRenderer, TInput>;
  private frameRequest = 0;
  private previousTimestamp = 0;
  private initialized = false;
  private disposed = false;

  constructor(options: GameShellOptions<TConfiguration, TRenderer, TInput>) {
    this.options = options;
    this.devTools = new DevTools({title: options.title, enabled: options.development});
  }

  /** Whether this shell has begun its terminal cleanup path. */
  get isDisposed(): boolean {
    return this.disposed;
  }

  /**
   * Create each owned resource in dependency order, initialize the application, and start rendering.
   */
  async initialize(): Promise<void> {
    if(this.initialized || this.disposed) {
      return;
    }
    this.initialized = true;
    window.addEventListener(BROWSER_EVENT.PAGEHIDE, this.onPageHide);
    document.addEventListener(BROWSER_EVENT.VISIBILITYCHANGE, this.onVisibilityChange);

    try {
      this.configuration = this.options.createConfiguration();
      this.devTools.log('Starting renderer.');
      const renderer = await this.options.createRenderer(this.configuration);
      if(this.disposed) {
        renderer.destroy();
        return;
      }
      this.renderer = renderer;
      this.input = this.options.createInput(this.renderer, this.configuration);
      await this.options.initialize?.(this);
      if(this.disposed) {
        return;
      }
      this.devTools.log('Game shell ready.');
      this.frameRequest = requestAnimationFrame(this.frame);
    } catch(error) {
      try {
        this.dispose();
      } finally {
        this.options.fail(error);
      }
    }
  }

  /** Reset elapsed time before resuming simulation after a pause or visibility change. */
  resetClock(): void {
    this.previousTimestamp = 0;
  }

  /** Replace the configuration record supplied to subsequent phases. Ownership remains with the application. */
  setConfiguration(configuration: TConfiguration): void {
    this.configuration = configuration;
  }

  /**
   * Stop scheduling frames and release application, input, renderer, diagnostics, and listeners in dependency order.
   * Repeated disposal is harmless.
   */
  dispose(): void {
    if(this.disposed) {
      return;
    }
    this.disposed = true;
    cancelAnimationFrame(this.frameRequest);
    window.removeEventListener(BROWSER_EVENT.PAGEHIDE, this.onPageHide);
    document.removeEventListener(BROWSER_EVENT.VISIBILITYCHANGE, this.onVisibilityChange);

    try {
      this.options.dispose?.(this);
    } finally {
      try {
        this.input?.destroy();
      } finally {
        this.input = null;
        try {
          this.renderer?.destroy();
        } finally {
          this.renderer = null;
          this.devTools.destroy();
        }
      }
    }
  }

  private frame = (timestampMilliseconds: number): void => {
    if(this.disposed || !this.configuration || !this.renderer || !this.input) {
      return;
    }
    try {
      const maximumFrameSeconds = this.options.maximumFrameSeconds ?? DEFAULT_MAXIMUM_FRAME_SECONDS;
      const elapsedSeconds = this.previousTimestamp
        ? Math.min(maximumFrameSeconds, Math.max(0, (timestampMilliseconds - this.previousTimestamp)/1000))
        : 0;
      this.previousTimestamp = timestampMilliseconds;
      const frame = {
        configuration: this.configuration,
        renderer: this.renderer,
        input: this.input,
        devTools: this.devTools,
        elapsedSeconds,
        timeSeconds: timestampMilliseconds/1000,
        timestampMilliseconds
      };

      this.options.input?.(frame);
      this.options.simulation?.(frame);
      this.options.presentation?.(frame);
      this.options.render(frame);
      this.options.afterFrame?.(frame);
      this.devTools.frame(timestampMilliseconds);
      this.frameRequest = requestAnimationFrame(this.frame);
    } catch(error) {
      try {
        this.dispose();
      } finally {
        this.options.fail(error);
      }
    }
  };

  private readonly onVisibilityChange = (): void => {
    this.resetClock();
  };

  private readonly onPageHide = (event: PageTransitionEvent): void => {
    if(!event.persisted) {
      this.dispose();
    }
  };
}
