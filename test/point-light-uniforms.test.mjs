import test from 'node:test';
import assert from 'node:assert/strict';
import { createPointLightUniforms, POINT_LIGHT_UNIFORM_BYTES } from '../src/rendering/pbr/point-lights.ts';

const matrix = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
const light = { x: 1, y: 2, z: 3, range: 40, r: 4, g: 5, b: 6 };

test('point light uniforms preserve shared scene prefix and portable array offsets', () => {
  const data = createPointLightUniforms(matrix, [light], 0.5);
  assert.equal(data.byteLength, POINT_LIGHT_UNIFORM_BYTES);
  assert.deepEqual(Array.from(data.slice(0, 16)), matrix);
  assert.deepEqual(Array.from(data.slice(16, 24)), [1, 1, 1, 1, 1, 0.5, 0, 0]);
  assert.deepEqual(Array.from(data.slice(24, 32)), [1, 2, 3, 40, 4, 5, 6, 0]);
  assert.ok(data.slice(32).every(value => value === 0));
});

test('point light uniforms reject nonrepresentable GPU values and invalid light limits', () => {
  assert.throws(() => createPointLightUniforms(matrix, Array(33).fill(light)), /32/);
  assert.throws(() => createPointLightUniforms(matrix, [light], 1e100), /float32/);
  assert.throws(() => createPointLightUniforms(matrix.map(value => value * 1e100), []), /float32/);
  for(const changes of [{ x: 1e100 }, { r: 1e100 }, { range: 1e100 }, { range: 1e-100 }]) {
    assert.throws(() => createPointLightUniforms(matrix, [{ ...light, ...changes }]), /float32/);
  }
  for(const changes of [{ range: 0 }, { r: -1 }, { x: NaN }]) {
    assert.throws(() => createPointLightUniforms(matrix, [{ ...light, ...changes }]));
  }
});
