// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
const {spawn} = require('node:child_process');
const path = require('node:path');

// The workspace host owns Vite, its file service, project servers and cleanup.
const child = spawn(require('electron'), [path.join(__dirname, 'workspace.cjs')], {stdio: 'inherit'});
const stop = () => child.kill('SIGTERM');
process.once('SIGINT', stop);
process.once('SIGTERM', stop);
child.once('error', error => {
  console.error(error);
  process.exitCode = 1;
});
child.once('exit', code => {
  process.removeListener('SIGINT', stop);
  process.removeListener('SIGTERM', stop);
  process.exitCode = code ?? 1;
});
