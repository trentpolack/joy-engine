// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Full-screen shader passes in luma.gl's `ShaderPass` shape: a WGSL module with a `<name>_sampleColor` function, an
//! optional `<name>` uniform block, and named texture, sampler, and storage bindings. This module packs uniform blocks
//! with WGSL layout rules and assembles each pass into a complete shader, numbering luma.gl's `@binding(auto)`
//! declarations in source order.

use crate::scene_effects::passes::PASSES;

/// WGSL types used by pass uniform blocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UniformKind {
    F32,
    U32,
    Vec2,
    Vec3,
    Vec4,
    Mat4,
}

impl UniformKind {
    /// WGSL alignment in bytes.
    fn align(self) -> usize {
        match self {
            Self::F32 | Self::U32 => 4,
            Self::Vec2 => 8,
            Self::Vec3 | Self::Vec4 | Self::Mat4 => 16,
        }
    }

    /// WGSL size in bytes.
    fn size(self) -> usize {
        match self {
            Self::F32 | Self::U32 => 4,
            Self::Vec2 => 8,
            Self::Vec3 => 12,
            Self::Vec4 => 16,
            Self::Mat4 => 64,
        }
    }

    /// Scalar components.
    fn components(self) -> usize {
        self.size() / 4
    }
}

/// One uniform block member with luma.gl's default value.
#[derive(Clone, Copy, Debug)]
pub(crate) struct UniformField {
    pub(crate) name: &'static str,
    pub(crate) kind: UniformKind,
    pub(crate) default: &'static [f32],
}

/// One luma.gl shader pass.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PassDefinition {
    pub(crate) name: &'static str,
    pub(crate) source: &'static str,
    pub(crate) uniforms: &'static [UniformField],
}

/// The pass named `name`.
pub(crate) fn find_pass(name: &str) -> Option<&'static PassDefinition> {
    PASSES.iter().find(|pass| pass.name == name)
}

/// Uniform values by member name; later layers override earlier ones.
pub(crate) type UniformValues = Vec<(&'static str, Vec<f32>)>;

/// Byte offsets of a uniform block's members and its padded size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UniformLayout {
    pub(crate) offsets: Vec<usize>,
    pub(crate) size: usize,
}

/// Lay out members with WGSL uniform-buffer rules; the size rounds up to 16 bytes.
pub(crate) fn uniform_layout(fields: &[UniformField]) -> UniformLayout {
    let mut offset = 0_usize;
    let mut offsets = Vec::with_capacity(fields.len());
    for field in fields {
        offset = offset.next_multiple_of(field.kind.align());
        offsets.push(offset);
        offset += field.kind.size();
    }
    UniformLayout { offsets, size: offset.next_multiple_of(16).max(16) }
}

/// Pack a pass's uniform block: defaults, then each layer of overrides in order (step, then frame).
pub(crate) fn pack_uniforms(pass: &PassDefinition, layers: &[&UniformValues]) -> Vec<u8> {
    let layout = uniform_layout(pass.uniforms);
    let mut bytes = vec![0_u8; layout.size];
    for (field, offset) in pass.uniforms.iter().zip(layout.offsets) {
        let value = layers
            .iter()
            .rev()
            .find_map(|layer| {
                layer.iter().rev().find(|(name, _)| *name == field.name).map(|(_, value)| value.as_slice())
            })
            .unwrap_or(field.default);
        for component in 0..field.kind.components() {
            let number = value.get(component).copied().unwrap_or(0.0);
            let word = match field.kind {
                UniformKind::U32 => (number.max(0.0).round() as u32).to_le_bytes(),
                _ => number.to_le_bytes(),
            };
            let start = offset + (component * 4);
            bytes[start..start + 4].copy_from_slice(&word);
        }
    }
    bytes
}

/// What a declared binding holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BindingKind {
    /// A uniform buffer.
    Uniform,
    /// A float color texture.
    Texture,
    /// A depth texture.
    DepthTexture,
    /// A sampler.
    Sampler,
    /// A read-only storage buffer.
    Storage,
}

/// One numbered binding in an assembled shader.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Binding {
    pub(crate) name: String,
    pub(crate) kind: BindingKind,
    pub(crate) index: u32,
}

/// The full-screen vertex stage: luma.gl's `ClipSpace` triangle strip with its clip positions and coordinates.
const VERTEX_SOURCE: &str = r"
struct FragmentInputs {
  @builtin(position) Position: vec4<f32>,
  @location(0) position: vec2<f32>,
  @location(1) coordinate: vec2<f32>,
  @location(2) uv: vec2<f32>,
};

@vertex
fn vertexMain(@builtin(vertex_index) vertexIndex: u32) -> FragmentInputs {
  let positions = array<vec2<f32>, 4>(vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, 1.0));
  let coordinates = array<vec2<f32>, 4>(vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 1.0));
  var outputs: FragmentInputs;
  outputs.Position = vec4<f32>(positions[vertexIndex], 0.0, 1.0);
  outputs.position = positions[vertexIndex];
  outputs.coordinate = coordinates[vertexIndex];
  outputs.uv = coordinates[vertexIndex];
  return outputs;
}
";

/// luma.gl's `textureTransform` module: maps strip coordinates to source UVs, flipping Y for top-left textures.
const TEXTURE_TRANSFORM_SOURCE: &str = r"
struct textureTransformUniforms {
  scale: vec2<f32>,
  flipY: i32,
};
@group(0) @binding(auto) var<uniform> textureTransform: textureTransformUniforms;
fn shaderPassRenderer_getTextureUV(uv: vec2f) -> vec2f {
  var position = (uv - vec2f(0.5, 0.5)) / textureTransform.scale + vec2f(0.5, 0.5);
  if (textureTransform.flipY != 0) {
    position.y = 1.0 - position.y;
  }
  return position;
}
fn shaderPassRenderer_getRenderTargetUV(textureUV: vec2f) -> vec2f {
  let unscaledCoord = (textureUV - vec2f(0.5, 0.5)) * textureTransform.scale + vec2f(0.5, 0.5);
  return select(vec2f(unscaledCoord.x, 1.0 - unscaledCoord.y), unscaledCoord, textureTransform.flipY != 0);
}
";

/// The name of the shared texture-transform uniform binding.
pub(crate) const TEXTURE_TRANSFORM: &str = "textureTransform";

/// Assemble a sampling pass into a complete shader and list its numbered bindings.
pub(crate) fn assemble(pass: &PassDefinition) -> (String, Vec<Binding>) {
    let fragment = format!(
        r"
@group(0) @binding(auto) var sourceTexture: texture_2d<f32>;
@group(0) @binding(auto) var sourceTextureSampler: sampler;
@fragment
fn fragmentMain(inputs: FragmentInputs) -> @location(0) vec4f {{
  let texCoord = shaderPassRenderer_getTextureUV(inputs.coordinate);
  let texSize = vec2f(textureDimensions(sourceTexture));
  return {name}_sampleColor(sourceTexture, sourceTextureSampler, texSize, texCoord);
}}
",
        name = pass.name
    );
    let combined = format!("{VERTEX_SOURCE}{TEXTURE_TRANSFORM_SOURCE}{}\n{fragment}", pass.source);
    number_bindings(&combined)
}

/// Replace each `@binding(auto)` with sequential numbers and record each declaration's name and kind.
fn number_bindings(source: &str) -> (String, Vec<Binding>) {
    const AUTO: &str = "@binding(auto)";
    let mut output = String::with_capacity(source.len() + 64);
    let mut bindings = Vec::new();
    let mut rest = source;
    while let Some(position) = rest.find(AUTO) {
        let index = bindings.len() as u32;
        output.push_str(&rest[..position]);
        output.push_str(&format!("@binding({index})"));
        rest = &rest[position + AUTO.len()..];
        let declaration = rest.split(';').next().unwrap_or_default();
        if let Some(binding) = parse_declaration(declaration, index) {
            bindings.push(binding);
        }
    }
    output.push_str(rest);
    (output, bindings)
}

/// Parse ` var<qualifier> name: type` after a binding attribute.
fn parse_declaration(declaration: &str, index: u32) -> Option<Binding> {
    let text = declaration.trim_start().strip_prefix("var")?;
    let (qualifier, text) = match text.trim_start().strip_prefix('<') {
        Some(inner) => {
            let end = inner.find('>')?;
            (inner[..end].trim(), &inner[end + 1..])
        }
        None => ("", text),
    };
    let (name, kind_text) = text.split_once(':')?;
    let name = name.trim().to_owned();
    let kind_text = kind_text.trim();
    let kind = if qualifier == "uniform" {
        BindingKind::Uniform
    } else if qualifier.starts_with("storage") {
        BindingKind::Storage
    } else if kind_text.starts_with("texture_depth") {
        BindingKind::DepthTexture
    } else if kind_text.starts_with("texture") {
        BindingKind::Texture
    } else if kind_text.starts_with("sampler") {
        BindingKind::Sampler
    } else {
        return None;
    };
    Some(Binding { name, kind, index })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_follow_wgsl_alignment() {
        let fields = [
            UniformField { name: "a", kind: UniformKind::F32, default: &[1.0] },
            UniformField { name: "b", kind: UniformKind::Vec3, default: &[0.0; 3] },
            UniformField { name: "c", kind: UniformKind::Vec2, default: &[0.0; 2] },
            UniformField { name: "d", kind: UniformKind::Mat4, default: &[0.0; 16] },
        ];
        assert_eq!(uniform_layout(&fields), UniformLayout { offsets: vec![0, 16, 32, 48], size: 112 });
    }

    #[test]
    fn packing_layers_overrides_over_defaults() {
        let pass = find_pass("depthAwareBlur").unwrap();
        let step: UniformValues = vec![("direction", vec![0.0, 1.0])];
        let frame: UniformValues = vec![("radius", vec![2.0])];
        let bytes = pack_uniforms(pass, &[&step, &frame]);
        let floats: Vec<f32> = bytes.as_chunks::<4>().0.iter().map(|word| f32::from_le_bytes(*word)).collect();
        // direction (vec2), radius, depthSigma, spatialSigma.
        assert_eq!(&floats[..5], &[0.0, 1.0, 2.0, 0.01, 3.0]);
    }

    #[test]
    fn every_pass_assembles_with_named_bindings_and_matching_uniform_blocks() {
        for pass in PASSES {
            let (source, bindings) = assemble(pass);
            assert!(!source.contains("@binding(auto)"), "{}", pass.name);
            let names: Vec<&str> = bindings.iter().map(|binding| binding.name.as_str()).collect();
            assert!(names.contains(&"sourceTexture") && names.contains(&TEXTURE_TRANSFORM), "{}: {names:?}", pass.name);
            assert_eq!(
                bindings.iter().any(|binding| binding.kind == BindingKind::Uniform && binding.name == pass.name),
                !pass.uniforms.is_empty(),
                "{}",
                pass.name
            );
            // Every sampler belongs to a texture named before its `Sampler` suffix (some textures are only loaded).
            for binding in bindings.iter().filter(|binding| binding.kind == BindingKind::Sampler) {
                let texture = binding.name.strip_suffix("Sampler").unwrap_or_default();
                assert!(names.contains(&texture), "{}: {}", pass.name, binding.name);
            }
            // The WGSL uniform struct lists members in the table's order.
            if let Some(declaration) = source.split(&format!("var<uniform> {}:", pass.name)).nth(1) {
                let type_name = declaration.split(';').next().unwrap().trim();
                let body = source.split(&format!("struct {type_name} {{")).nth(1).unwrap().split('}').next().unwrap();
                let members: Vec<&str> = body
                    .split(',')
                    .filter_map(|member| member.split(':').next())
                    .map(|member| member.trim().rsplit(|c: char| c.is_whitespace()).next().unwrap_or_default())
                    .filter(|member| !member.is_empty() && !member.starts_with("//"))
                    .collect();
                let table: Vec<&str> = pass.uniforms.iter().map(|field| field.name).collect();
                assert_eq!(members, table, "{}", pass.name);
            }
            wgpu::naga::front::wgsl::parse_str(&source).unwrap_or_else(|error| panic!("{}: {error}", pass.name));
        }
    }
}
