// Copyright (c) 2026 Trent Polack. Licensed under the MIT License.
import assert from 'node:assert/strict';
import {mkdtemp, rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {_electron, expect} from '@playwright/test';
const root = fileURLToPath(new URL('../../../', import.meta.url));
const userData = await mkdtemp(path.join(tmpdir(), 'joy-editor-native-'));
let application;
try {
  application = await _electron.launch({args: ['joy-editor/desktop/workspace.cjs'], cwd: root,
    env: {...process.env, ELECTRON_RUN_AS_NODE: '', JOY_EDITOR_TEST_USER_DATA: userData, JOY_EDITOR_TEST_HIDDEN: '1'}});
  const page = await application.firstWindow();
  page.setDefaultTimeout(60000);
  await expect(page.locator('#project')).toHaveValue('god-game');
  await expect(page.locator('#workspace')).toHaveClass(/preview-collapsed/);
  await page.locator('#play-mode').selectOption('window');
  await page.locator('#play-size').selectOption('720');
  const opened = application.waitForEvent('window');
  await page.locator('#run').click();
  const gameWindow = await opened;
  await expect(gameWindow.locator('iframe')).toBeVisible();
  await expect(page.locator('#pause')).toBeEnabled();
  assert.deepEqual(await gameWindow.evaluate(() => [innerWidth,innerHeight]), [1280,720]);
  await page.locator('#pause').click();
  await expect(page.locator('#pause')).toHaveText('Resume');
  await page.locator('#run').click();
  assert.equal(application.windows().length, 2, 'Repeated Play must not create another native window.');
  await page.locator('#stop').click();
  await expect.poll(() => application.windows().length).toBe(1);
  await expect(page.locator('#stop')).toBeDisabled();
  console.log('PASS hidden isolated Electron: 720p content size, connected pause, single window and stop teardown');
} finally {
  await application?.close();
  await rm(userData, {recursive:true,force:true});
}
