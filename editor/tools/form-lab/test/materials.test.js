import test from 'node:test';
import assert from 'node:assert/strict';
import { DOMParser } from '@xmldom/xmldom';
import { defaultMaterial, readMaterials } from 'joy-engine/form';
import { exportMaterialX, importMaterialX } from '../src/materialx.ts';

globalThis.DOMParser = DOMParser;

const copper = {
  name: 'Copper',
  baseColor: [0.8, 0.25, 0.1, 0.4],
  metallic: 0.9,
  roughness: 0.2,
  emissive: [0.1, 0.05, 0],
  alphaMode: 'MASK',
  alphaCutoff: 0.35,
  textures: { baseColor: null, normal: null, emissive: null, metallic: null, roughness: null, ao: null },
};

test('material defaults and validated reads return owned records with Default first', () => {
  const defaults = readMaterials(undefined);
  assert.deepEqual(defaults, [defaultMaterial()]);
  assert.deepEqual(defaults[0], {
    name: 'Default',
    baseColor: [1, 1, 1, 1],
    metallic: 0,
    roughness: 1,
    emissive: [0, 0, 0],
    alphaMode: 'OPAQUE',
    alphaCutoff: 0.5,
    textures: { baseColor: null, normal: null, emissive: null, metallic: null, roughness: null, ao: null },
  });

  const source = [copper];
  const materials = readMaterials(source);
  assert.deepEqual(materials, [defaultMaterial(), copper]);
  assert.notEqual(materials[1], source[0]);
  assert.notEqual(materials[1].baseColor, source[0].baseColor);
});

test('material validation rejects malformed, duplicate, and excessive definitions', () => {
  assert.throws(() => readMaterials(null), /array/);
  assert.throws(() => readMaterials([{ ...copper, name: 'not valid' }]), /name/);
  assert.throws(() => readMaterials([copper, { ...copper }]), /unique/);
  assert.throws(() => readMaterials([{ ...copper, metallic: 2 }]), /metallic/);
  assert.throws(() => readMaterials([{ ...copper, baseColor: [1, 1, 1] }]), /baseColor/);
  assert.throws(() => readMaterials([{ ...copper, extra: true }]), /unsupported/);
  assert.throws(
    () => readMaterials(Array.from({ length: 128 }, (_, index) => ({ ...copper, name: `M${index}` }))),
    /128/,
  );
});

test('MaterialX constant gltf_pbr materials round-trip all supported values', () => {
  const source = readMaterials([{ ...defaultMaterial(), roughness: 0.75 }, copper]);
  const xml = exportMaterialX(source);
  assert.match(xml, /<gltf_pbr name="FormLabShader0" type="surfaceshader">/);
  assert.match(xml, /name="base_color" type="color3"/);
  assert.match(xml, /name="alpha_mode" type="integer"/);
  assert.deepEqual(importMaterialX(xml), source);
});

test('texture budgets reject oversized images and aggregate documents without mutating materials', () => {
  const texture = `data:image/png;base64,${Buffer.alloc(4_000_000).toString('base64')}`;
  const first = defaultMaterial();
  for(const field of Object.keys(first.textures)) {
    first.textures[field] = texture;
  }
  const second = { ...defaultMaterial(), name: 'Other' };
  second.textures.baseColor = texture;
  second.textures.normal = texture;
  assert.equal(readMaterials([first, second]).length, 2);
  second.textures.emissive = 'data:image/png;base64,AA==';
  assert.throws(() => readMaterials([first, second]), /32 MB/);
  assert.equal(second.textures.emissive, 'data:image/png;base64,AA==');
  const oversized = defaultMaterial();
  oversized.textures.baseColor = `data:image/png;base64,${Buffer.alloc(4_000_001).toString('base64')}`;
  assert.throws(() => readMaterials([oversized]), /4 MB/);
});

test('MaterialX export rejects textures instead of silently discarding them', () => {
  const material = defaultMaterial();
  material.textures.baseColor = 'data:image/png;base64,AA==';
  assert.throws(() => exportMaterialX([material]), /textures.*\.formlab.*GLB/);
  assert.equal(material.textures.baseColor, 'data:image/png;base64,AA==');
});

test('MaterialX import rejects graphs, textures, and unsupported inputs explicitly', () => {
  const xml = exportMaterialX([defaultMaterial()]);
  assert.throws(() => importMaterialX(xml.replace('/>', ' nodename="image1"/>')), /connection|constant/i);
  assert.throws(() => importMaterialX(xml.replace('</gltf_pbr>', '<input name="coat" type="float" value="1"/></gltf_pbr>')), /unsupported.*coat/i);
  assert.throws(() => importMaterialX(xml.replace('</materialx>', '<image name="texture" type="color3"/></materialx>')), /unsupported.*image/i);
});

test('MaterialX export generates shader names outside the shared material namespace', () => {
  const materials = readMaterials([
    { ...defaultMaterial(), name: 'Foo' },
    { ...defaultMaterial(), name: 'Foo_shader' },
  ]);
  const xml = exportMaterialX(materials);
  const imported = importMaterialX(xml);
  assert.deepEqual(imported.map((material) => material.name), ['Default', 'Foo', 'Foo_shader']);
  assert.doesNotMatch(xml, /<gltf_pbr name="Foo_shader"/);
});

test('MaterialX import returns only authored definitions while preserving an authored Default', () => {
  const onlyCopper = exportMaterialX([copper]).replace(
    /\s*<surfacematerial name="Default"[\s\S]*?<\/surfacematerial>/,
    '',
  ).replace(/\s*<gltf_pbr name="[^\"]+" type="surfaceshader">[\s\S]*?<\/gltf_pbr>/, '');
  assert.deepEqual(importMaterialX(onlyCopper), [copper]);

  const customDefault = { ...defaultMaterial(), roughness: 0.3 };
  assert.deepEqual(importMaterialX(exportMaterialX([customDefault])), [customDefault]);
});

test('MaterialX import rejects unsafe declarations, unknown versions, and lossy attributes', () => {
  const xml = exportMaterialX([defaultMaterial()]);
  assert.throws(() => importMaterialX(`<!DOCTYPE materialx [<!ENTITY x "bad">]>${xml}`), /DOCTYPE|entit/i);
  assert.throws(() => importMaterialX(xml.replace('version="1.39"', 'version="9.0"')), /version/i);
  assert.throws(() => importMaterialX(xml.replace('name="base_color"', 'colorspace="srgb_texture" name="base_color"')), /colorspace/i);
  assert.throws(() => importMaterialX(xml.replace('name="metallic"', 'unit="meter" name="metallic"')), /unit/i);
  assert.throws(() => importMaterialX(xml.replace('name="roughness"', 'interfacename="roughness" name="roughness"')), /interfacename|connection/i);
});

test('MaterialX import rejects missing and globally duplicated node names', () => {
  const xml = exportMaterialX([defaultMaterial()]);
  assert.throws(() => importMaterialX(xml.replace(/<gltf_pbr name="[^"]+"/, '<gltf_pbr')), /name/i);
  const shaderName = xml.match(/<gltf_pbr name="([^"]+)"/)[1];
  assert.throws(() => importMaterialX(xml.replace('name="Default" type="material"', `name="${shaderName}" type="material"`)), /duplicate|unique/i);
});

test('MaterialX import rejects empty and malformed numeric values', () => {
  const xml = exportMaterialX([defaultMaterial()]);
  for(const malformed of ['', '1oops', '0x10', '1 2', '1,,2']) {
    assert.throws(
      () => importMaterialX(xml.replace('value="0"/>', `value="${malformed}"/>`)),
      /invalid/i,
    );
  }
});

test('MaterialX import rejects unreferenced shaders and significant element text', () => {
  const xml = exportMaterialX([defaultMaterial()]);
  const shader = xml.match(/  <gltf_pbr[\s\S]*?  <\/gltf_pbr>\n/)[0];
  const extraShader = shader.replace(/name="[^"]+"/, 'name="UnusedShader"');
  assert.throws(() => importMaterialX(xml.replace('</materialx>', `${extraShader}</materialx>`)), /UnusedShader.*unreferenced/i);
  assert.throws(() => importMaterialX(xml.replace('<materialx version="1.39">', '<materialx version="1.39">lost')), /text/i);
});

