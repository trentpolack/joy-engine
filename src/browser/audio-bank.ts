// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

export interface SoundDefinition {
  url: string;
  volume?: number;
  maximumVoices?: number;
}
export interface AudioBankOptions {
  maximumVoices?: number;
  contextFactory?: () => AudioContext;
}
export interface PreparedSound {
  buffer: AudioBuffer;
  volume: number;
  maximumVoices: number;
}
export interface AudioVoice {
  name: string;
  source: AudioBufferSourceNode;
  gain: GainNode;
}

const DEFAULT_MAXIMUM_VOICES = 32;

/**
 * Owns one Web Audio context, decoded samples, and bounded overlapping voices.
 * Await `create()` before gameplay, call `resume()` directly from a user gesture,
 * and call `destroy()` when the owning session ends. Playback never loads or
 * decodes assets and does not queue sounds while the context is suspended.
 */
export class AudioBank {
  declare context: AudioContext;
  declare sounds: Map<string, PreparedSound>;
  declare maximumVoices: number;
  declare voices: Set<AudioVoice>;
  declare destroyed: boolean;

  /**
   * @private Use the asynchronous factory so callers receive a fully loaded bank.
   * @param context Owned context; never shared with another bank.
   * @param sounds Owned decoded samples.
   * @param maximumVoices Bank-wide limit.
   */
  private constructor(context: AudioContext, sounds: Map<string, PreparedSound>, maximumVoices: number) {
    this.context = context;
    this.sounds = sounds;
    this.maximumVoices = maximumVoices;
    /** Insertion order is playback age, for voice stealing. */
    this.voices = new Set();
    this.destroyed = false;
  }

  /**
   * Fetch and decode all samples before returning. Rejects with the sound name
   * on load failure and releases the context on any initialization failure.
   * @param definitions Borrowed configuration, not mutated.
   * @param [options]
   */
  static async create(definitions: Record<string, SoundDefinition>, options: AudioBankOptions = {}): Promise<AudioBank> {
    const maximumVoices = options.maximumVoices ?? DEFAULT_MAXIMUM_VOICES;
    validateVoiceLimit(maximumVoices);
    for(const definition of Object.values(definitions)) {
      validateVoiceLimit(definition.maximumVoices ?? maximumVoices);
      const volume = definition.volume ?? 1;
      if(!Number.isFinite(volume) || volume < 0 || volume > 1) {
        throw new RangeError('Sound volume must be between zero and one.');
      }
    }

    const context = options.contextFactory?.() ?? new AudioContext({ latencyHint: 'interactive' });

    const sounds: Map<string, PreparedSound> = new Map();
    try {
      for(const [name, definition] of Object.entries(definitions)) {
        try {
          const response = await fetch(definition.url);
          if(!response.ok) {
            throw new Error(`HTTP ${response.status}`);
          }
          const buffer = await context.decodeAudioData(await response.arrayBuffer());
          sounds.set(name, {
            buffer,
            volume: definition.volume ?? 1,
            maximumVoices: definition.maximumVoices ?? maximumVoices,
          });
        } catch (error) {
          throw new Error(`Unable to load sound "${name}" (${definition.url}): ${error instanceof Error ? error.message : error}`, { cause: error });
        }
      }
      return new AudioBank(context, sounds, maximumVoices);
    } catch (error) {
      await context.close().catch(() => {});
      throw error;
    }
  }

  /** Number of currently retained playback voices. */
  get activeVoiceCount() {
    return this.voices.size;
  }

  /**
   * Unlock audio directly inside a pointer, keyboard, or click handler. May be
   * called again after a browser/OS interruption. Rejects if browser activation
   * fails; returns false after disposal. Each gesture retries native activation,
   * even if an earlier request is still pending while playback is disallowed.
   */
  resume(): Promise<boolean> {
    if(this.destroyed || this.context.state === 'closed') {
      return Promise.resolve(false);
    }
    if(this.context.state === 'running') {
      return Promise.resolve(true);
    }
    // A denied request may stay pending indefinitely. Do not let that promise
    // block a later gesture from retrying the browser's permission check.
    return this.context.resume().then(() => !this.destroyed && this.context.state === 'running');
  }

  /**
   * Start a sound immediately; false means unknown, suspended, or disposed.
   * Samples are reused; Web Audio requires a new, inexpensive source per play.
   * At capacity, retire the oldest matching sound, then the oldest bank voice.
   * @param name
   * @returns Whether playback started.
   */
  play(name: string): boolean {
    const sound = this.sounds.get(name);
    if(this.destroyed || this.context.state !== 'running' || !sound) {
      return false;
    }

    let matchingCount = 0;

    let oldestMatching: AudioVoice | undefined;
    for(const voice of this.voices) {
      if(voice.name === name) {
        matchingCount++;
        oldestMatching ??= voice;
      }
    }
    if(matchingCount >= sound.maximumVoices && oldestMatching) {
      this.releaseVoice(oldestMatching, true);
    }
    if(this.voices.size >= this.maximumVoices) {
      const oldest = this.voices.values().next().value;
      if(oldest) {
        this.releaseVoice(oldest, true);
      }
    }

    const source = this.context.createBufferSource();
    const gain = this.context.createGain();
    source.buffer = sound.buffer;
    gain.gain.value = sound.volume;
    source.connect(gain);
    gain.connect(this.context.destination);
    const voice = { name, source, gain };
    this.voices.add(voice);
    source.onended = () => this.releaseVoice(voice, false);
    source.start();
    return true;
  }

  /** Stop voices, discard samples, and close the owned context. Idempotent. */
  destroy() {
    if(this.destroyed) {
      return;
    }
    this.destroyed = true;
    for(const voice of this.voices) {
      this.releaseVoice(voice, true);
    }
    this.sounds.clear();
    void this.context.close().catch(() => {});
  }

  /**
   * @private Retire both natural completions and stolen voices exactly once.
   * @param voice
   * @param stop Whether playback must be stopped before disconnecting.
   */
  private releaseVoice(voice: AudioVoice, stop: boolean) {
    if(!this.voices.delete(voice)) {
      return;
    }
    voice.source.onended = null;
    if(stop) {
      voice.source.stop();
    }
    voice.source.disconnect();
    voice.gain.disconnect();
  }
}

/**
 * Reject unusable voice limits before creating any audio resources.
 * @param maximumVoices
 */
function validateVoiceLimit(maximumVoices: number) {
  if(!Number.isSafeInteger(maximumVoices) || maximumVoices < 1) {
    throw new RangeError('maximumVoices must be a positive safe integer.');
  }
}
