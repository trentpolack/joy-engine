#version 300 es
precision highp float;
// settings.w selects material (0), LOD colors (1), or LOD colors + grid (2).
layout(std140) uniform scene { mat4 matrix; vec4 eye; vec4 geometry; vec4 settings; vec4 target; };
in vec3 worldPosition;
in vec3 diagnostic;
out vec4 fragmentColor;
void main() {
  // Derivatives give the normal of the rendered, morphed triangle. Flip upward
  // since the draw pipeline disables culling and may show either winding.
  vec3 n = normalize(cross(dFdx(worldPosition),dFdy(worldPosition)));
  if(n.y < 0.0) { n = -n; }
  // Simple slope/altitude palette with diffuse light, matching terrain.wgsl.
  float slope = 1.0 - n.y;
  vec3 color = mix(vec3(0.21,0.36,0.26),vec3(0.49,0.43,0.36),smoothstep(0.1,0.65,slope));
  color = mix(color,vec3(0.79,0.82,0.77),smoothstep(390.0,520.0,worldPosition.y));
  if(settings.w > 0.5) { color = 0.5 + 0.4*cos(vec3(0,2,4) + diagnostic.z*1.2); }
  float light = 0.3 + 0.7*max(0.0,dot(n,normalize(vec3(-0.4,0.8,0.2))));
  color*= light;
  if(settings.w > 1.5) {
    // Derivative-scaled distance to a cell line gives a roughly pixel-wide
    // overlay even as camera distance and the selected grid spacing change.
    vec2 lines = abs(fract(diagnostic.xy - 0.5) - 0.5)/max(fwidth(diagnostic.xy),vec2(0.0001));
    color*= mix(0.3,1.0,smoothstep(0.0,1.0,min(lines.x,lines.y)));
  }
  // Encode lit color for display in this standalone terrain pass.
  fragmentColor = vec4(pow(color,vec3(1.0/2.2)),1);
}
