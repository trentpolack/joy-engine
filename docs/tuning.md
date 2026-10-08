# Live object tuning
`LiveTuning` from `joy-engine/development` registers borrowed live objects with explicit numeric metadata and supplies the existing ConfigSession and ConfigPanel. Games declare fields and register objects; they do not build custom panels, manually map values on every edit, or maintain separate histories.

```js
if(import.meta.env.DEV) {
  const {LiveTuning} = await import('joy-engine/development');
  const tuning = new LiveTuning({title: 'MOVEMENT FEEL'});
  tuning.register('player', movement, [
    {key: 'speed', label: 'Speed', description: 'Top movement speed.',
      group: 'Movement', min: 0, max: 20, step: 0.1, units: 'm/s'}
  ]);
  await tuning.mount();
  // Destroy before releasing movement or replacing its properties.
}
```

Register all objects before accessing `session` or calling `mount()`. An identifier names each object's values as `player.speed`; identifiers cannot contain dots. Fields specify labels, bounds, units and steps. Defaults capture each property's registration value unless `defaultValue` supplies an authored default. Only writable numeric own data properties are supported. Accessors and frozen properties are rejected; do not register proxies or replace descriptors while the owner is alive. Unlisted fields, nested records and runtime collaborators remain untouched.

All registered objects share one session and panel. Accepted values affect live objects synchronously; validation completes before any object changes. `session.undo()`, `redo()`, `resetField(key)`, `resetAll()` and `revert()` work across registered objects. `beginEdit()` and `endEdit()` group slider or other continuous previews into a single undo entry. New accepted edits discard the redo branch; rejected transitions retain values and history. The panel groups slider edits until change, focus loss, cancellation or closing. Ctrl/Cmd+Z undoes and Shift+Ctrl/Cmd+Z redoes within the panel. Incomplete or invalid numeric drafts remain visible and block Save until corrected.

`capturePreset()` returns a copied flat numeric record. `applyPreset(record)` validates a partial namespaced preset and applies it as one transaction. Unknown fields or out-of-range values reject the entire preset. Applications can offer their own named preset library using these records.

Without a save adapter, tuning is session-only: the panel states this explicitly and hides Save. No source file, authored level, local storage or runtime object serialization happens automatically. Supply `save: async values => ...` only when the application has an explicit persistence boundary. It receives a complete copied snapshot and ConfigSession tracks the latest successfully saved baseline independently of edits during the save. Destroying tuning removes its panel and history while retaining accepted values on borrowed objects.

Vehicle Playground dynamically imports tuning in development, registers live engine force, steering lock and braking impulse for each kart plus the camera pitch/yaw, and exposes the shared TUNE panel. Regenerating the authored level or restarting replaces the vehicles, disposes the old tuning owner and begins a new session against the replacements. Its runtime defaults and saved level are unchanged by session tuning. Production builds eliminate the development import and panel.
