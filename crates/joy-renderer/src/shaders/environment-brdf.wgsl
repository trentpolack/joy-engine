// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// The split-sum BRDF table (Karis 2013): for a view angle (X, the cosine between normal and view) and perceptual
// roughness (Y, smooth at the top row), the GGX lobe's response to Schlick Fresnel as a scale (R) on F0 and a bias (G)
// on F90. Visibility is the height-correlated Smith term the direct lights use, so indirect and direct specular agree.
// The table depends on no environment, so the renderer integrates it once. Composed after environment-common.wgsl.

// Edge of the table in pixels (`BRDF_TABLE_SIZE` in capture.rs) and importance samples per texel.
const BRDF_TABLE_SIZE: f32 = 128.0;
const BRDF_SAMPLES: u32 = 512u;

/**
 * Height-correlated Smith visibility for GGX, including the BRDF's 1/(4 n.l n.v).
 */
fn smithVisibility(normalDotView: f32, normalDotLight: f32, alpha: f32) -> f32 {
  let squared = alpha*alpha;
  let viewTerm = normalDotLight*sqrt(((normalDotView*normalDotView)*(1.0 - squared)) + squared);
  let lightTerm = normalDotView*sqrt(((normalDotLight*normalDotLight)*(1.0 - squared)) + squared);
  return 0.5/max(viewTerm + lightTerm, 0.000001);
}

@fragment fn brdfMain(@builtin(position) position: vec4f) -> @location(0) vec4f {
  let coordinate = position.xy/BRDF_TABLE_SIZE;
  let normalDotView = max(coordinate.x, 0.0001);
  let alpha = max(coordinate.y*coordinate.y, 0.0001);
  let view = vec3f(sqrt(max(1.0 - (normalDotView*normalDotView), 0.0)), 0.0, normalDotView);
  var integral = vec2f(0.0);
  for(var index = 0u; index < BRDF_SAMPLES; index++) {
    let halfVector = ggxHalfVector(hammersley(index, BRDF_SAMPLES), alpha);
    let viewDotHalf = max(dot(view, halfVector), 0.0);
    let light = (2.0*viewDotHalf*halfVector) - view;
    let normalDotLight = light.z;
    let normalDotHalf = max(halfVector.z, 0.000001);
    if(normalDotLight > 0.0) {
      // The lobe's value over its sampling density: visibility times 4 v.h n.l / n.h (the distribution cancels).
      let response = smithVisibility(normalDotView, normalDotLight, alpha)*4.0*viewDotHalf*normalDotLight/normalDotHalf;
      let fresnel = pow(1.0 - viewDotHalf, 5.0);
      integral+= vec2f((1.0 - fresnel)*response, fresnel*response);
    }
  }
  return vec4f(integral/f32(BRDF_SAMPLES), 0.0, 1.0);
}
