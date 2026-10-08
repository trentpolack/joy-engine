// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Shared editor library entry point: authoring UI consumed by the Joy Editor
// workspace and its lab tools. Games never import this package; game-facing
// contracts (preview protocol, project manifests, asset documents) stay in
// joy-engine so a shipped game does not depend on editor code.

// Script editing (CodeMirror-backed).
export {
  createScriptEditor,
  createScriptEditorState,
  StreamLanguage
} from './script-editor.ts';

// Preview presentation.
export {
  PreviewFullscreen
} from './preview-fullscreen.ts';
