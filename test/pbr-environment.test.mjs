// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import test from 'node:test';
import {PBREnvironmentGenerator} from '@luma.gl/experimental';
import {PbrEnvironment} from '../src/rendering/environment/pbr-environment.ts';

test('environment replacement is atomic and cleanup leaves the borrowed source alive', t => {
  const generated = [];
  let fail = false;
  t.mock.method(PBREnvironmentGenerator.prototype, 'prepare', () => {
    if(fail) { throw new Error('integration failed'); }
    const value = {diffuseTexture: {}, specularTexture: {mipLevels: 7}, brdfLUTTexture: {}, destroyed: 0,
      destroy() { this.destroyed++; }};
    generated.push(value);
    return value;
  });
  const source = {destroyed: 0, destroy() { this.destroyed++; }};
  const owner = new PbrEnvironment({});
  owner.prepare(source);
  const accepted = owner.bindings;
  fail = true;
  assert.throws(() => owner.prepare(source), /integration failed/);
  assert.equal(owner.bindings, accepted);
  assert.equal(generated[0].destroyed, 0);
  fail = false;
  owner.prepare(source);
  assert.equal(generated[0].destroyed, 1);
  owner.destroy();
  owner.destroy();
  assert.equal(generated[1].destroyed, 1);
  assert.equal(source.destroyed, 0);
});
