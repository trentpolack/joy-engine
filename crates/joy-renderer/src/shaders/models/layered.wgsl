// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Layered shading model: physically based surface layers (`SurfaceLayer` in joy-render-types), collapsed by the
// renderer into one layer per material. Implements the contract in surface-contract.wgsl.
//
// A layer reflects specularly with a GGX microfacet lobe whose Fresnel runs from F0 at normal incidence to F90 at
// grazing angles, and scatters the rest diffusely (Lambert). `metallic` blends toward a conductor whose base color is
// its F0. Optional features: a hazy second specular lobe, fiber sheen (fuzz) with the Charlie distribution, a clear
// dielectric coat that dims and tints the layer beneath it, and thin-film iridescence (Belcour and Barla 2017) that
// replaces the layer's Fresnel color.
//
// Deferred lighting stores the layer's diffuse color, F0, roughness, a gray F90, and at most one optional lobe (haze,
// an untinted coat with the default coat F0, or fuzz). The renderer shades materials that need more (thin film, a
// colored F90, a tinted coat, or two optional lobes) in the forward pass instead (`layer_fits_gbuffer` in Rust).

const MATERIAL_RECEIVES_LIGHT: bool = true;

// Smallest perceptual roughness shaded; smoother lobes alias under point lights.
const MIN_ROUGHNESS: f32 = 0.045;

struct MaterialParameters {
  // X alpha cutoff, Y alpha mode (0 opaque, 1 mask, 2 blend), ZW reserved.
  coverage: vec4f,
  // Linear RGBA base color: diffuse albedo for dielectrics, F0 for metals.
  baseColor: vec4f,
  // RGB dielectric F0; A metallic.
  f0Metallic: vec4f,
  // RGB dielectric F90; A perceptual roughness.
  f90Roughness: vec4f,
  // RGB emitted radiance; A reserved.
  emissive: vec4f,
  // X haze roughness, Y haze weight, Z fuzz roughness, W fuzz amount.
  hazeFuzz: vec4f,
  // RGB fuzz color; A reserved.
  fuzzColor: vec4f,
  // X coat weight, Y coat roughness, Z coat F0, W reserved.
  coat: vec4f,
  // RGB coat transmission tint; A reserved.
  coatTint: vec4f,
  // X film thickness in nanometers, Y film index of refraction, Z film weight, W reserved.
  thinFilm: vec4f,
  // X has a normal map, Y has an occlusion map, ZW reserved.
  maps: vec4f,
};

@group(3) @binding(0) var<uniform> material: MaterialParameters;
@group(3) @binding(1) var baseColorMap: texture_2d<f32>;
@group(3) @binding(2) var normalMap: texture_2d<f32>;
@group(3) @binding(3) var emissiveMap: texture_2d<f32>;
@group(3) @binding(4) var metallicMap: texture_2d<f32>;
@group(3) @binding(5) var roughnessMap: texture_2d<f32>;
@group(3) @binding(6) var occlusionMap: texture_2d<f32>;
@group(3) @binding(7) var materialSampler: sampler;

struct MaterialSurface {
  // Shading normal after normal mapping, and the unperturbed normal the coat reflects with.
  normal: vec3f,
  coatNormal: vec3f,
  view: vec3f,
  // Straight alpha of base color times the vertex color.
  alpha: f32,
  diffuse: vec3f,
  f0: vec3f,
  f90: vec3f,
  metallic: f32,
  roughness: f32,
  occlusion: f32,
  emission: vec3f,
  hazeRoughness: f32,
  hazeWeight: f32,
  fuzzColor: vec3f,
  fuzzRoughness: f32,
  coatWeight: f32,
  coatRoughness: f32,
  coatF0: f32,
  coatTint: vec3f,
  // Thin-film Fresnel color for this view angle and how strongly it replaces the layer's own.
  filmFresnel: vec3f,
  filmWeight: f32,
};

/**
 * Sample every map and resolve the layer's reflectance for this fragment.
 */
fn materialSurface(input: SurfaceInput) -> MaterialSurface {
  // Sample before any discard so implicit derivatives stay valid.
  let sampledBase = textureSample(baseColorMap, materialSampler, input.uv);
  let sampledNormal = (textureSample(normalMap, materialSampler, input.uv).xyz*2.0) - 1.0;
  let sampledEmission = textureSample(emissiveMap, materialSampler, input.uv).rgb;
  let sampledMetallic = textureSample(metallicMap, materialSampler, input.uv).r;
  let sampledRoughness = textureSample(roughnessMap, materialSampler, input.uv).r;
  let sampledOcclusion = textureSample(occlusionMap, materialSampler, input.uv).r;

  var surface: MaterialSurface;
  let geometric = surfaceNormal(input);
  surface.coatNormal = geometric;
  surface.normal = geometric;
  if(material.maps.x > 0.5) {
    surface.normal = perturbNormal(input, geometric, sampledNormal);
  }
  surface.view = input.viewDirection;

  let base = input.color*material.baseColor*sampledBase;
  surface.alpha = base.a;
  surface.metallic = clamp(material.f0Metallic.a*sampledMetallic, 0.0, 1.0);
  surface.roughness = clamp(material.f90Roughness.a*sampledRoughness, MIN_ROUGHNESS, 1.0);
  surface.occlusion = select(1.0, sampledOcclusion, material.maps.y > 0.5);
  surface.diffuse = base.rgb*(1.0 - surface.metallic);
  surface.f0 = mix(material.f0Metallic.rgb, base.rgb, surface.metallic);
  surface.f90 = mix(material.f90Roughness.rgb, vec3f(1.0), surface.metallic);
  surface.emission = material.emissive.rgb*sampledEmission;

  surface.hazeRoughness = clamp(material.hazeFuzz.x, MIN_ROUGHNESS, 1.0);
  surface.hazeWeight = material.hazeFuzz.y;
  surface.fuzzRoughness = clamp(material.hazeFuzz.z, 0.07, 1.0);
  surface.fuzzColor = material.fuzzColor.rgb*material.hazeFuzz.w;
  surface.coatWeight = material.coat.x;
  surface.coatRoughness = clamp(material.coat.y, MIN_ROUGHNESS, 1.0);
  surface.coatF0 = material.coat.z;
  surface.coatTint = mix(vec3f(1.0), material.coatTint.rgb, material.coat.x);

  surface.filmWeight = material.thinFilm.z;
  surface.filmFresnel = surface.f0;
  if(surface.filmWeight > 0.0) {
    let nv = max(dot(surface.normal, surface.view), 0.0001);
    surface.filmFresnel = thinFilmFresnel(1.0, material.thinFilm.y, nv, material.thinFilm.x, surface.f0);
  }
  return surface;
}

fn materialOpacity(input: SurfaceInput) -> f32 {
  return input.color.a*material.baseColor.a*textureSample(baseColorMap, materialSampler, input.uv).a;
}

fn materialAlpha(surface: MaterialSurface) -> f32 {
  return surface.alpha;
}

/**
 * Response to one light: the coat on top, then the layer's specular lobes, diffuse scattering, and sheen beneath it.
 */
fn materialDirect(surface: MaterialSurface, light: LightSample) -> vec3f {
  let n = surface.normal;
  let v = surface.view;
  let l = light.direction;
  let nl = max(dot(n, l), 0.0);

  if(nl <= 0.0) {
    return vec3f(0.0);
  }

  let h = normalize(v + l);
  let nv = max(dot(n, v), 0.0001);
  let nh = max(dot(n, h), 0.0);
  let vh = max(dot(v, h), 0.0);

  // Specular: the primary lobe, the haze lobe, and the thin film's Fresnel replacing the layer's own.
  let fresnel = mix(schlickFresnel(surface.f0, surface.f90, vh), surface.filmFresnel, surface.filmWeight);
  var specular = ggxLobe(nh, nv, nl, surface.roughness);

  if(surface.hazeWeight > 0.0) {
    specular = mix(specular, ggxLobe(nh, nv, nl, surface.hazeRoughness), surface.hazeWeight);
  }

  let diffuse = (vec3f(1.0) - fresnel)*surface.diffuse/PI;
  var layer = (diffuse + (specular*fresnel))*nl;

  // Fiber sheen sits on the layer and takes energy from it.
  if(max(surface.fuzzColor.r, max(surface.fuzzColor.g, surface.fuzzColor.b)) > 0.0) {
    let sheen = charlieDistribution(nh, surface.fuzzRoughness)*neubeltVisibility(nl, nv);
    layer = (layer*sheenScaling(surface.fuzzColor)) + (surface.fuzzColor*sheen*nl);
  }

  // The coat reflects first; what passes through is dimmed by its Fresnel and tinted.
  if(surface.coatWeight > 0.0) {
    let coatNl = max(dot(surface.coatNormal, l), 0.0);
    let coatNv = max(dot(surface.coatNormal, v), 0.0001);
    let coatNh = max(dot(surface.coatNormal, h), 0.0);
    let coatFresnel = schlickFresnel(vec3f(surface.coatF0), vec3f(1.0), vh).x*surface.coatWeight;
    let coat = ggxLobe(coatNh, coatNv, coatNl, surface.coatRoughness)*coatFresnel*coatNl;
    layer = (layer*surface.coatTint*(1.0 - coatFresnel)) + vec3f(coat);
  }
  return layer*light.radiance;
}

/**
 * Ambient and image-based light. Without an environment map the sky's irradiance also lights the specular lobe through
 * an analytic split-sum approximation, so metals reflect the sky instead of going black.
 */
fn materialIndirect(surface: MaterialSurface, ambient: AmbientLight) -> vec3f {
  let n = surface.normal;
  let v = surface.view;
  let nv = max(dot(n, v), 0.0001);
  let reflection = reflect(-v, n);

  // Roughness-aware Fresnel for the hemisphere average.
  let edge = max(surface.f90*(1.0 - surface.roughness), surface.f0);
  let fresnel = mix(surface.f0 + ((edge - surface.f0)*pow(1.0 - nv, 5.0)), surface.filmFresnel, surface.filmWeight);
  let diffuseWeight = (vec3f(1.0) - fresnel)*surface.diffuse;

  var diffuse = ambientIrradiance(ambient, n)*diffuseWeight;
  var specular = vec3f(0.0);
  if(ambient.environment.x > 0.0) {
    let brdf = environmentBrdf(nv, surface.roughness);
    let scale = (fresnel*brdf.x) + (surface.f90*brdf.y);
    diffuse+= environmentDiffuse(ambient, n)*diffuseWeight;
    var radiance = environmentSpecular(ambient, reflection, surface.roughness);
    if(surface.hazeWeight > 0.0) {
      radiance = mix(radiance, environmentSpecular(ambient, reflection, surface.hazeRoughness), surface.hazeWeight);
    }
    specular = radiance*scale;
  } else {
    let brdf = analyticEnvironmentBrdf(nv, surface.roughness);
    specular = ambientIrradiance(ambient, reflection)*((fresnel*brdf.x) + (surface.f90*brdf.y));
  }
  var layer = (diffuse + specular)*surface.occlusion;
  if(max(surface.fuzzColor.r, max(surface.fuzzColor.g, surface.fuzzColor.b)) > 0.0) {
    layer = (layer*sheenScaling(surface.fuzzColor)) + (surface.fuzzColor*ambientIrradiance(ambient, n)*0.5*surface.occlusion);
  }

  if(surface.coatWeight > 0.0) {
    let coatNv = max(dot(surface.coatNormal, v), 0.0001);
    let coatFresnel = schlickFresnel(vec3f(surface.coatF0), vec3f(1.0), coatNv).x*surface.coatWeight;
    let coatReflection = reflect(-v, surface.coatNormal);
    var coatRadiance = ambientIrradiance(ambient, coatReflection);
    if(ambient.environment.x > 0.0) {
      coatRadiance = environmentSpecular(ambient, coatReflection, surface.coatRoughness);
    }
    layer = (layer*surface.coatTint*(1.0 - coatFresnel)) + (coatRadiance*coatFresnel*surface.occlusion);
  }
  return layer;
}

fn materialEmission(surface: MaterialSurface) -> vec3f {
  return surface.emission;
}

// The optional lobe a G-buffer texel carries (bits 0-1 of its third word).
const LOBE_NONE: u32 = 0u;
const LOBE_HAZE: u32 = 1u;
const LOBE_COAT: u32 = 2u;
const LOBE_FUZZ: u32 = 3u;

// The coat F0 deferred surfaces decode; must match `Coat::default` in joy-render-types.
const DEFERRED_COAT_F0: f32 = 0.04;

/**
 * Word Y: diffuse color as RGB9E5. Word Z: F0 (square-root encoded, eight bits per channel) and roughness. Word W: the
 * lobe, F90 in six bits, then 24 bits of lobe data.
 */
fn materialEncode(surface: MaterialSurface) -> vec3u {
  let f0 = sqrt(clamp(surface.f0, vec3f(0.0), vec3f(1.0)));
  let specular =
    packUnorm(f0.r, 8u) | (packUnorm(f0.g, 8u) << 8u) | (packUnorm(f0.b, 8u) << 16u) | (packUnorm(surface.roughness, 8u) << 24u);

  var lobe = LOBE_NONE;
  var data = 0u;
  if(surface.coatWeight > 0.0) {
    lobe = LOBE_COAT;
    data = packUnorm(surface.coatWeight, 5u) | (packUnorm(sqrt(surface.coatRoughness), 5u) << 5u) |
      (packOctahedral(surface.coatNormal, 7u) << 10u);
  } else if(surface.hazeWeight > 0.0) {
    lobe = LOBE_HAZE;
    data = packUnorm(surface.hazeWeight, 8u) | (packUnorm(surface.hazeRoughness, 8u) << 8u);
  } else if(max(surface.fuzzColor.r, max(surface.fuzzColor.g, surface.fuzzColor.b)) > 0.0) {
    lobe = LOBE_FUZZ;
    let fuzz = sqrt(clamp(surface.fuzzColor, vec3f(0.0), vec3f(1.0)));
    data = packUnorm(fuzz.r, 5u) | (packUnorm(fuzz.g, 5u) << 5u) | (packUnorm(fuzz.b, 5u) << 10u) |
      (packUnorm(surface.fuzzRoughness, 6u) << 15u);
  }
  let f90 = packUnorm(surface.f90.g, 6u);
  return vec3u(packRgb9e5(surface.diffuse), specular, lobe | (f90 << 2u) | (data << 8u));
}

/**
 * The surface `materialDirect` needs, from `materialEncode`'s words, the engine's shading normal, and the view
 * direction. Indirect light, emission, and occlusion were applied when the G-buffer was written.
 */
fn materialDecode(payload: vec3u, normal: vec3f, view: vec3f) -> MaterialSurface {
  var surface: MaterialSurface;
  surface.normal = normal;
  surface.coatNormal = normal;
  surface.view = view;
  surface.alpha = 1.0;
  surface.diffuse = unpackRgb9e5(payload.x);
  let f0 = vec3f(
    unpackUnorm(gbufferField(payload.y, 0u, 8u), 8u),
    unpackUnorm(gbufferField(payload.y, 8u, 8u), 8u),
    unpackUnorm(gbufferField(payload.y, 16u, 8u), 8u)
  );
  surface.f0 = f0*f0;
  surface.roughness = max(unpackUnorm(gbufferField(payload.y, 24u, 8u), 8u), MIN_ROUGHNESS);
  surface.f90 = vec3f(unpackUnorm(gbufferField(payload.z, 2u, 6u), 6u));
  surface.metallic = 0.0;
  surface.occlusion = 1.0;
  surface.emission = vec3f(0.0);
  surface.hazeRoughness = surface.roughness;
  surface.hazeWeight = 0.0;
  surface.fuzzColor = vec3f(0.0);
  surface.fuzzRoughness = 0.5;
  surface.coatWeight = 0.0;
  surface.coatRoughness = MIN_ROUGHNESS;
  surface.coatF0 = DEFERRED_COAT_F0;
  surface.coatTint = vec3f(1.0);
  surface.filmFresnel = surface.f0;
  surface.filmWeight = 0.0;

  let data = payload.z >> 8u;
  switch gbufferField(payload.z, 0u, 2u) {
    case LOBE_COAT: {
      surface.coatWeight = unpackUnorm(gbufferField(data, 0u, 5u), 5u);
      let coatRoughness = unpackUnorm(gbufferField(data, 5u, 5u), 5u);
      surface.coatRoughness = max(coatRoughness*coatRoughness, MIN_ROUGHNESS);
      surface.coatNormal = unpackOctahedral(gbufferField(data, 10u, 14u), 7u);
    }
    case LOBE_HAZE: {
      surface.hazeWeight = unpackUnorm(gbufferField(data, 0u, 8u), 8u);
      surface.hazeRoughness = max(unpackUnorm(gbufferField(data, 8u, 8u), 8u), MIN_ROUGHNESS);
    }
    case LOBE_FUZZ: {
      let fuzz = vec3f(
        unpackUnorm(gbufferField(data, 0u, 5u), 5u),
        unpackUnorm(gbufferField(data, 5u, 5u), 5u),
        unpackUnorm(gbufferField(data, 10u, 5u), 5u)
      );
      surface.fuzzColor = fuzz*fuzz;
      surface.fuzzRoughness = max(unpackUnorm(gbufferField(data, 15u, 6u), 6u), 0.07);
    }
    default: {}
  }
  return surface;
}

fn materialAttributes(surface: MaterialSurface) -> SurfaceAttributes {
  var attributes: SurfaceAttributes;
  attributes.normal = surface.normal;
  attributes.roughness = surface.roughness;
  attributes.diffuseAlbedo = surface.diffuse;
  attributes.specularColor = surface.f0;
  attributes.metallic = surface.metallic;
  attributes.emissive = surface.emission;
  attributes.occlusion = surface.occlusion;
  return attributes;
}

// ---------------------------------------------------------------------------------------------------------------------
// Lobes
// ---------------------------------------------------------------------------------------------------------------------

/**
 * Schlick's Fresnel from `f0` at normal incidence to `f90` at grazing.
 */
fn schlickFresnel(f0: vec3f, f90: vec3f, cosine: f32) -> vec3f {
  return f0 + ((f90 - f0)*pow(1.0 - clamp(cosine, 0.0, 1.0), 5.0));
}

/**
 * GGX distribution times the height-correlated Smith visibility for a perceptual `roughness`; multiply by Fresnel and
 * the cosine to get the specular BRDF's contribution.
 */
fn ggxLobe(nh: f32, nv: f32, nl: f32, roughness: f32) -> f32 {
  let a2 = pow(roughness, 4.0);
  let denominator = (nh*nh*(a2 - 1.0)) + 1.0;
  let distribution = a2/max(PI*denominator*denominator, 0.000000000001);
  let visibility = 0.5/max((nl*sqrt((nv*nv*(1.0 - a2)) + a2)) + (nv*sqrt((nl*nl*(1.0 - a2)) + a2)), 0.0001);
  return distribution*visibility;
}

/**
 * Charlie sheen distribution (Estevez and Kulla 2017) for a fiber `roughness`.
 */
fn charlieDistribution(nh: f32, roughness: f32) -> f32 {
  let alpha = roughness*roughness;
  let inverse = 1.0/alpha;
  let sine = sqrt(max(1.0 - (nh*nh), 0.0));
  return (2.0 + inverse)*pow(sine, inverse)/(2.0*PI);
}

/**
 * Neubelt and Pettineo's sheen visibility.
 */
fn neubeltVisibility(nl: f32, nv: f32) -> f32 {
  return 1.0/max(4.0*(nl + nv - (nl*nv)), 0.0001);
}

/**
 * How much light sheen leaves for the layer beneath it: sheen's directional albedo averages roughly 0.157 at full
 * coverage, so the layer keeps the rest.
 */
fn sheenScaling(fuzzColor: vec3f) -> f32 {
  return 1.0 - (0.157*max(fuzzColor.r, max(fuzzColor.g, fuzzColor.b)));
}

/**
 * Analytic fit of the split-sum environment BRDF (Karis 2014, after Lazarov 2013): F0 scale (X) and bias (Y).
 */
fn analyticEnvironmentBrdf(nv: f32, roughness: f32) -> vec2f {
  let c0 = vec4f(-1.0, -0.0275, -0.572, 0.022);
  let c1 = vec4f(1.0, 0.0425, 1.04, -0.04);
  let r = (roughness*c0) + c1;
  let a004 = (min(r.x*r.x, exp2(-9.28*nv))*r.x) + r.y;
  return (vec2f(-1.04, 1.04)*a004) + r.zw;
}

// ---------------------------------------------------------------------------------------------------------------------
// Thin-film iridescence (Belcour and Barla 2017)
// ---------------------------------------------------------------------------------------------------------------------

/**
 * CIE XYZ to linear Rec. 709 RGB.
 */
const XYZ_TO_REC709: mat3x3f = mat3x3f(
  3.2404542, -0.9692660, 0.0556434,
  -1.5371385, 1.8760108, -0.2040259,
  -0.4985314, 0.0415560, 1.0572252
);

/**
 * Fresnel reflectance at normal incidence between two media.
 */
fn interfaceReflectance(transmitted: f32, incident: f32) -> f32 {
  let ratio = (transmitted - incident)/(transmitted + incident);
  return ratio*ratio;
}

/**
 * The spectral sensitivity of the eye to an optical path difference, integrated against Gaussian fits of the CIE
 * color matching functions and returned as linear Rec. 709 RGB.
 */
fn filmSensitivity(pathDifference: f32, shift: vec3f) -> vec3f {
  let phase = 2.0*PI*pathDifference*0.000000001;
  let amplitude = vec3f(5.4856e-13, 4.4201e-13, 5.2481e-13);
  let position = vec3f(1.6810e+06, 1.7953e+06, 2.2084e+06);
  let variance = vec3f(4.3278e+09, 9.3046e+09, 6.6121e+09);
  var xyz = amplitude*sqrt(2.0*PI*variance)*cos((position*phase) + shift)*exp(-(phase*phase)*variance);
  xyz.x+= 9.7470e-14*sqrt(2.0*PI*4.5282e+09)*cos((2.2399e+06*phase) + shift.x)*exp(-4.5282e+09*(phase*phase));
  return XYZ_TO_REC709*(xyz/1.0685e-7);
}

/**
 * Reflectance of a thin film of index `filmIor` and `thickness` nanometers over a base with reflectance `baseF0`,
 * seen at `cosine` from a medium of index `outsideIor`.
 */
fn thinFilmFresnel(outsideIor: f32, filmIor: f32, cosine: f32, thickness: f32, baseF0: vec3f) -> vec3f {
  // The film fades into the outside medium as it vanishes.
  let ior = mix(outsideIor, filmIor, smoothstep(0.0, 0.03, thickness));
  let sinSquared = ((outsideIor/ior)*(outsideIor/ior))*(1.0 - (cosine*cosine));
  let cosSquared = 1.0 - sinSquared;
  if(cosSquared < 0.0) {
    return vec3f(1.0);
  }
  let filmCosine = sqrt(cosSquared);

  // First interface: outside medium to film.
  let reflectance12 = schlickFresnel(vec3f(interfaceReflectance(ior, outsideIor)), vec3f(1.0), cosine).x;
  let transmittance121 = 1.0 - reflectance12;
  let phase12 = select(0.0, PI, ior < outsideIor);
  let phase21 = PI - phase12;

  // Second interface: film to base, with the base's index recovered from its F0.
  let baseRoot = sqrt(clamp(baseF0, vec3f(0.0), vec3f(0.9999)));
  let baseIor = (vec3f(1.0) + baseRoot)/(vec3f(1.0) - baseRoot);
  let baseRatio = (baseIor - vec3f(ior))/(baseIor + vec3f(ior));
  let reflectance23 = schlickFresnel(baseRatio*baseRatio, vec3f(1.0), filmCosine);
  let phase23 = select(vec3f(0.0), vec3f(PI), baseIor < vec3f(ior));

  // Interference between the film's internal reflections.
  let pathDifference = 2.0*ior*thickness*filmCosine;
  let phase = vec3f(phase21) + phase23;
  let reflectance123 = clamp(reflectance12*reflectance23, vec3f(0.00001), vec3f(0.9999));
  let amplitude123 = sqrt(reflectance123);
  let transmitted = (transmittance121*transmittance121)*reflectance23/(vec3f(1.0) - reflectance123);
  var intensity = vec3f(reflectance12) + transmitted;
  var term = transmitted - vec3f(transmittance121);
  for(var order = 1; order <= 2; order++) {
    term = term*amplitude123;
    intensity+= term*2.0*filmSensitivity(f32(order)*pathDifference, f32(order)*phase);
  }
  return max(intensity, vec3f(0.0));
}
