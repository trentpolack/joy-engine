// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import {readFileSync,writeFileSync} from 'node:fs';
import {compile} from 'joy-engine/form';
import {assetFromEvaluation} from '../src/asset.ts';
const source = readFileSync(new URL('../assets/building.form',import.meta.url),'utf8');
const presets = JSON.parse(readFileSync(new URL('../assets/presets.json',import.meta.url),'utf8'));
for(const [name,overrides] of Object.entries(presets)) {
  writeFileSync(new URL(`../assets/${name}.formlab`,import.meta.url),JSON.stringify({version:1,name,source,overrides},null,2) + '\n');
}
const asset = assetFromEvaluation(compile(source,presets.cottage));
writeFileSync(new URL('../assets/cottage.mesh.json',import.meta.url),JSON.stringify(asset.geometry) + '\n');
console.info('Saved editable .formlab presets and a baked cottage mesh.');
