// Height-only CLOD: X/Z never move; fractional LOD moves Y onto parent triangles.
// Scene matches TerrainRenderer.frameData: view-projection matrix, eye.xyz +
// CSS projection scale, sample size/tile cells/XZ extents, height offset/scale +
// error tolerance/diagnostic mode, and world-space target.xyz + pin radius.
struct Scene { matrix:mat4x4<f32>, eye:vec4<f32>, geometry:vec4<f32>, settings:vec4<f32>, focus:vec4<f32> };
// Patch base = sample origin X/Z, fractional LOD, padding. Edge LODs are
// -X, +X, -Z, +Z and agree with the adjacent patch's matching edge.
struct Patch { base:vec4<f32>, edges:vec4<f32> };
@group(0) @binding(0) var<uniform> scene:Scene;
@group(0) @binding(1) var heightTexture:texture_2d<f32>;
@group(0) @binding(2) var<storage,read> patches:array<Patch>;
@group(0) @binding(3) var<uniform> drawIndex:vec4<f32>;
struct VertexInput { @location(0) gridPosition:vec2<f32>, @builtin(instance_index) patchIndex:u32 };
struct VertexOutput { @builtin(position) position:vec4<f32>, @location(0) worldPosition:vec3<f32>, @location(1) levelData:vec3<f32> };

// Texture values were decoded to world heights at upload. Integer texel loads
// avoid bilinear filtering, which would disagree with the indexed triangles.
fn heightAt(p:vec2<f32>) -> f32 { return textureLoad(heightTexture,vec2<i32>(p),0).r; }

// Reconstruct the a-to-d diagonal used by terrain-grid.js. Clamp the cell origin
// at the positive landscape boundary to keep the four reads in range.
fn triangleHeight(p:vec2<f32>,stride:f32) -> f32 {
  let b = min(floor(p/stride)*stride,vec2<f32>(scene.geometry.x - 1.0 - stride));
  let f = (p - b)/stride;
  let a = heightAt(b); let bb = heightAt(b + vec2<f32>(stride,0));
  let c = heightAt(b + vec2<f32>(0,stride)); let d = heightAt(b + vec2<f32>(stride));
  if(f.x >= f.y) { return a + (bb - a)*f.x + (d - bb)*f.y; }
  return a + (d - c)*f.x + (c - a)*f.y;
}

// Shared boundary points blend between two triangle lattices at the agreed LOD.
fn morphedHeight(local:vec2<f32>,detail:f32,patchRecord:Patch) -> f32 {
  let stride = exp2(floor(detail));
  let p = patchRecord.base.xy + local;
  return mix(triangleHeight(p,stride),triangleHeight(p,min(scene.geometry.y,stride*2.0)),fract(detail));
}

// The parent surface must include stitched boundary vertices. Raw edge heights
// would give disappearing interior vertices a different destination at seams.
fn parentVertex(local:vec2<f32>,stride:f32,patchRecord:Patch) -> f32 {
  var detail = log2(stride);
  var boundary = false;
  if(local.x == 0.0) { detail = max(detail,patchRecord.edges.x); boundary = true; }
  if(local.x == scene.geometry.y) { detail = max(detail,patchRecord.edges.y); boundary = true; }
  if(local.y == 0.0) { detail = max(detail,patchRecord.edges.z); boundary = true; }
  if(local.y == scene.geometry.y) { detail = max(detail,patchRecord.edges.w); boundary = true; }
  if(boundary) { return morphedHeight(local,detail,patchRecord); }
  return heightAt(patchRecord.base.xy + local);
}

// Rebuild the actual parent triangle from its four potentially stitched corners.
fn parentSurface(local:vec2<f32>,stride:f32,patchRecord:Patch) -> f32 {
  let b = min(floor(local/stride)*stride,vec2<f32>(scene.geometry.y - stride));
  let f = (local - b)/stride;
  let a = parentVertex(b,stride,patchRecord); let bb = parentVertex(b + vec2<f32>(stride,0),stride,patchRecord);
  let c = parentVertex(b + vec2<f32>(0,stride),stride,patchRecord); let d = parentVertex(b + vec2<f32>(stride),stride,patchRecord);
  if(f.x >= f.y) { return a + (bb - a)*f.x + (d - bb)*f.y; }
  return a + (d - c)*f.x + (c - a)*f.y;
}

@vertex fn vertexMain(input:VertexInput) -> VertexOutput {
  // Indirect firstInstance is zero; the host binds the patch ID for each draw.
  let patchRecord = patches[u32(drawIndex.x)]; let local = input.gridPosition;
  let p = patchRecord.base.xy + local;
  var detail = patchRecord.base.z;
  // Interior vertices follow patch LOD. Boundary vertices follow shared edges;
  // tile corners survive every stride, so corner heights remain invariant.
  if(local.x == 0.0) { detail = patchRecord.edges.x; }
  if(local.x == scene.geometry.y) { detail = patchRecord.edges.y; }
  if(local.y == 0.0) { detail = max(detail,patchRecord.edges.z); }
  if(local.y == scene.geometry.y) { detail = max(detail,patchRecord.edges.w); }
  let stride = exp2(floor(detail));
  var h = morphedHeight(local,detail,patchRecord);
  // Current interior vertices are authored samples. Before the index range
  // changes, morph removed vertices onto the stitched parent triangle surface.
  if(local.x > 0.0 && local.x < scene.geometry.y && local.y > 0.0 && local.y < scene.geometry.y) {
    h = mix(heightAt(p),parentSurface(local,min(scene.geometry.y,stride*2.0),patchRecord),fract(detail));
  }
  var out:VertexOutput;
  // Convert global sample offsets into centered world X/Z; Y is already world
  // height. Diagnostic coordinates count cells at the current integer LOD.
  out.worldPosition = vec3<f32>((p.x/(scene.geometry.x - 1.0) - 0.5)*scene.geometry.z,h,(p.y/(scene.geometry.x - 1.0) - 0.5)*scene.geometry.w);
  out.position = scene.matrix*vec4<f32>(out.worldPosition,1);
  out.levelData = vec3<f32>(local/exp2(floor(patchRecord.base.z)),patchRecord.base.z);
  return out;
}

@fragment fn fragmentMain(input:VertexOutput) -> @location(0) vec4<f32> {
  // Derivatives follow the actual morphed triangle, without a normal texture.
  // Orient upward because either winding can be visible with culling disabled.
  var n = normalize(cross(dpdx(input.worldPosition),dpdy(input.worldPosition)));
  if(n.y < 0.0) { n = -n; }
  // Simple slope/altitude coloring and diffuse light; this is not a PBR material.
  let slope = 1.0 - n.y;
  var color = mix(vec3<f32>(0.21,0.36,0.26),vec3<f32>(0.49,0.43,0.36),smoothstep(0.1,0.65,slope));
  color = mix(color,vec3<f32>(0.79,0.82,0.77),smoothstep(390.0,520.0,input.worldPosition.y));
  if(scene.settings.w > 0.5) { color = 0.5 + 0.4*cos(vec3<f32>(0,2,4) + input.levelData.z*1.2); }
  let light = 0.3 + 0.7*max(0.0,dot(n,normalize(vec3<f32>(-0.4,0.8,0.2))));
  color*= light;
  // Screen derivatives keep the diagnostic grid about one pixel wide as LOD
  // and distance change. Mode 2 overlays it on the LOD colors from mode 1.
  let lines = abs(fract(input.levelData.xy - 0.5) - 0.5)/max(fwidth(input.levelData.xy),vec2<f32>(0.0001));
  if(scene.settings.w > 1.5) { color*= mix(0.3,1.0,smoothstep(0.0,1.0,min(lines.x,lines.y))); }
  // Apply display gamma to the lit color for this standalone terrain pass.
  return vec4<f32>(pow(color,vec3<f32>(1.0/2.2)),1);
}
