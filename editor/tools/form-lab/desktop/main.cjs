// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
const path = require('node:path');
const { launchDesktopApplication } = require('@joy-games/joy-editor/desktop');

launchDesktopApplication({
  title: 'FORM LAB',
  scheme: 'formlab',
  root: path.resolve(__dirname, '../dist'),
});
