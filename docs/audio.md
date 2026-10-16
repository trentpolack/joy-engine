# Audio
`runtime::AudioBank` owns short sound effects and bounded playback voices. Projects provide WAV bytes or PCM samples, sound names, volume, and overlap budgets. Procedural audio can use the same PCM path. See the public Rust API in `src/runtime/audio.rs`.

Native applications mix through the default output device using cpal. Browser builds use Web Audio and resume on the next pointer, touch, or key gesture. If an output device is unavailable, gameplay can continue silently and `play` returns false.

The bank applies per-sound and total voice limits, stealing older voices when necessary. `set_master_volume` scales sounds as they start. Keep sound playback in presentation and independent from simulation outcomes; audio failures must not change cooldowns or damage.

Own the bank with the session that uses it and release it with that session. Browser screenshots generally mute sound and cannot establish audio quality, latency, or interruption recovery; check those interactively on target platforms.
