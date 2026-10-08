import test from 'node:test';
import assert from 'node:assert/strict';
import { createAssetText, editorUrl, isSupportedAssetPath } from '../site/src/workspace/asset-types.ts';
import { compile } from 'joy-engine/form';
import { compileParticleEffect } from 'joy-engine';
import { parseObject } from 'joy-engine/objects';
import { readDocument } from '../tools/form-lab/src/document.ts';

test('new workspace assets open in their proper editor with immediately usable content', () => {
  for(const extension of ['form', 'formlab', 'joyfx', 'joyobject', 'joylevel']) {
    const path = `assets/new.${extension}`;
    assert.equal(isSupportedAssetPath(path), true);
    const text = createAssetText(path);
    if(extension === 'form') {
      assert.ok(compile(text).triangles.length > 0);
    }
    if(extension === 'formlab') {
      assert.ok(compile(readDocument(text).source).triangles.length > 0);
    }
    if(extension === 'joyfx') {
      assert.equal(compileParticleEffect(JSON.parse(text)).emitters.length, 1);
    }
    if(extension === 'joyobject') {
      assert.equal(parseObject(text).entity.components.render.shape, 'box');
    }
    assert.match(editorUrl(path), extension === 'joyfx' ? /particle-lab/ : extension.startsWith('form') ? /form-lab/ : /level.html/);
  }
  assert.throws(() => editorUrl('assets/unknown.txt'), /Unsupported/);
  assert.equal(isSupportedAssetPath('assets/unknown.txt'), false);
});
