# Audio
[Engine guide](README.md) · [Browser API](api/browser.md)

`AudioBank` owns short sound effects: one `AudioContext`, decoded `AudioBuffer` samples, and a bounded set of overlapping playback voices. Sound URLs, volume, and per-sound overlap budgets belong to the project. SPACE SCUFFLES defines them in `src/config/audio-config.js`.

## Initialization and playback
Await `AudioBank.create(definitions, options)` before starting gameplay. Each sample is fetched and decoded once; loading failure rejects with the sound name and closes the allocated context. This replaces the previous synchronous `new AudioBank(definitions)` API.

```js
import { AudioBank } from 'joy-engine';

const audio = await AudioBank.create({
  fire: { url: '/assets/fire.wav', volume: 0.18, maximumVoices: 4 },
}, { maximumVoices: 32 });

function activateAudio() {
  void audio.resume().catch(reportAudioFailure);
}
startButton.addEventListener('click', activateAudio);

// Gameplay presentation callback; does not fetch, decode, or resume audio.
audio.play('fire');
```

Call `resume()` directly inside a real keyboard, click, or pointer gesture. Keep activation handlers available for subsequent gestures after browser or operating-system interruptions. Handle rejected activation promises. Calls made while the context is suspended are dropped rather than queued and replayed late. `play()` returns whether a voice started; unknown names and disposed banks return false.

Each playback reuses the decoded sample and creates a one-shot source plus a gain node. Volume is linear from zero to one and defaults to one. The bank-wide limit defaults to 32 voices; each sound can override its own limit. At capacity, the oldest matching sound is stopped first, followed by the oldest bank voice if the global limit is still reached. Voice stealing cuts the old sound immediately, so use budgets large enough to preserve normal tails. These limits affect presentation only, not simulation or cooldowns.

## Ownership and cleanup
The caller owns the bank. Remove its activation handlers and call `destroy()` on permanent session teardown; destruction stops and disconnects voices, drops decoded buffers, and closes the context. Repeated destruction is safe. A custom `contextFactory` transfers exclusive context ownership to the bank, including when loading fails.

A `pagehide` event with `persisted: true` means the browser may restore the same document from its back/forward cache. Keep the bank alive in that case; destroying it would leave restored presentation callbacks pointing to a permanently disposed owner. SPACE SCUFFLES disposes audio on non-persisted pagehide, startup failure, and Vite hot replacement. If audio initialization fails, it reports the problem in the console and continues with silent gameplay.

## Verification
Unit regressions cover sample reuse, limits, interruption recovery, initialization failure, and resource release. Project captures provide the browser-level audio and startup smoke check.
The lifecycle event probe checks the pagehide handler contract using synthetic events; it does not establish native back/forward-cache behavior. Software-rendered browser automation also does not establish native Safari frame pacing. The normal screenshot capture mode mutes sounds and cannot verify audio performance.
