// Ported from luma.gl 9.4.2 (@luma.gl/effects src/passes/screen-space/copy-pass.ts).
// Copyright (c) vis.gl contributors. MIT License; see LICENSE-luma.gl beside this folder.

fn advancedCopy_sampleColor(
  sourceTexture: texture_2d<f32>, sourceTextureSampler: sampler, texSize: vec2f, texCoord: vec2f
) -> vec4f { return textureSample(sourceTexture, sourceTextureSampler, texCoord); }
