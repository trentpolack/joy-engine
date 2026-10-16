# Live Tuning
The engine's `development` module owns numeric configuration metadata, draft validation, history, panels, and persistence adapters. Games own defaults and decide when accepted values affect gameplay: immediately, on the next spawn, or after restart.

`ConfigField` describes labels, bounds, units, defaults, and application timing. `ConfigSession` owns accepted values and undo history; `ConfigPanel` presents them through egui. `LiveTuning` connects registered fields to game-owned state. Keep complete validation ahead of mutation so a rejected edit cannot partially change a session.

Session tuning does not imply a saved source change. Native persistence can target the declared project file; browser workflows export data or use explicit host support. Show save failures and preserve the draft. New sessions and replaced runtime owners must not retain references to previous tuning targets.

Game defaults live in Rust configuration modules; external override documents are validated serialized data. See [JoyGames config tuning](https://github.com/trentpolack/JoyGames/blob/develop/docs/development/config-tuning.md) and the public API for current field and persistence contracts.
