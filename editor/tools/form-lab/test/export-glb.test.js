import test from 'node:test';
import assert from 'node:assert/strict';
import { exportGlb } from '../src/export-glb.ts';
import { defaultMaterial } from 'joy-engine/form';
import { compile } from 'joy-engine/form';

test('GLB contains aligned JSON and binary chunks with declared total length', () => {
  const glb = exportGlb(sampleGeometry());
  const view = new DataView(glb);
  assert.equal(view.getUint32(0, true), 0x46546c67);
  assert.equal(view.getUint32(4, true), 2);
  assert.equal(view.getUint32(8, true), glb.byteLength);
  const jsonLength = view.getUint32(12, true);
  assert.equal(jsonLength % 4, 0);
  assert.equal(view.getUint32(16, true), 0x4e4f534a);
  const binaryHeader = 20 + jsonLength;
  assert.equal(view.getUint32(binaryHeader + 4, true), 0x004e4942);
  assert.equal(view.getUint32(binaryHeader, true) % 4, 0);
  assert.equal(binaryHeader + 8 + view.getUint32(binaryHeader, true), glb.byteLength);
});

test('GLB preserves mixed triangle and point assignments, flat normals, and RGBA', () => {
  const parsed = parseGlb(exportGlb(sampleGeometry()));
  assert.equal(parsed.json.meshes[0].primitives.length, 2);
  const triangle = parsed.json.meshes[0].primitives.find((primitive) => primitive.mode === 4);
  const point = parsed.json.meshes[0].primitives.find((primitive) => primitive.mode === 0);
  assert.equal(triangle.material, 1);
  assert.equal(point.material, 0);
  assert.equal(parsed.json.accessors[triangle.attributes.POSITION].count, 3);
  assert.deepEqual(readAccessor(parsed, triangle.attributes.NORMAL), [0, 0, 1, 0, 0, 1, 0, 0, 1]);
  assert.deepEqual(readAccessor(parsed, triangle.attributes.COLOR_0), [1, 0, 0, 0.25, 0, 1, 0, 0.5, 0, 0, 1, 0.75]);
  assert.deepEqual(readAccessor(parsed, point.attributes.POSITION), [2, 3, 4]);
  assert.deepEqual(parsed.json.accessors[triangle.attributes.POSITION].min, [0, 0, 0]);
  assert.deepEqual(parsed.json.accessors[triangle.attributes.POSITION].max, [1, 1, 0]);
});

test('GLB duplicates shared hard-edge vertices and keeps authored normals, UVs, colors, and materials aligned', () => {
  const box = parseGlb(exportGlb(compile('box(0,0,0,2,2,2)')));
  const primitive = box.json.meshes[0].primitives[0];
  const positions = readAccessor(box, primitive.attributes.POSITION);
  const normals = readAccessor(box, primitive.attributes.NORMAL);
  const cornerNormals = [];
  for(let index = 0; index < positions.length; index += 3) {
    if(positions.slice(index, index + 3).every(value => value === -1)) {
      cornerNormals.push(normals.slice(index, index + 3).map(value => value || 0));
    }
  }
  assert.equal(new Set(cornerNormals.map(value => value.join(','))).size, 3);
  assert.ok(cornerNormals.every(value => value.filter(component => component !== 0).length === 1));

  const data = compile('material(Glass) uv(0.25,0.75) color(1,0,0,0.5) normal(0,3,4) triangle(0,0,0,1,0,0,0,1,0) material() normal() uv(0.5,1) triangle(0,0,0,1,0,0,0,1,0)', {}, sampleGeometry().materials);
  const parsed = parseGlb(exportGlb(data));
  const authored = parsed.json.meshes[0].primitives.find(item => item.material === 1);
  const flat = parsed.json.meshes[0].primitives.find(item => item.material === 0);
  assert.deepEqual(readAccessor(parsed, authored.attributes.NORMAL), Array(3).fill([0, Math.fround(0.6), Math.fround(0.8)]).flat());
  assert.deepEqual(readAccessor(parsed, authored.attributes.TEXCOORD_0), Array(3).fill([0.25, 0.75]).flat());
  assert.deepEqual(readAccessor(parsed, authored.attributes.COLOR_0), Array(3).fill([1, 0, 0, 0.5]).flat());
  assert.deepEqual(readAccessor(parsed, flat.attributes.NORMAL), Array(3).fill([0, 0, 1]).flat());
  assert.deepEqual(readAccessor(parsed, flat.attributes.TEXCOORD_0), Array(3).fill([0.5, 1]).flat());
});

test('GLB materials preserve PBR alpha and two-sided properties', () => {
  const { json } = parseGlb(exportGlb(sampleGeometry()));
  assert.equal('alphaCutoff' in json.materials[0], false);
  assert.deepEqual(json.materials[1], {
    name: 'Glass',
    pbrMetallicRoughness: {
      baseColorFactor: [0.2, 0.3, 0.4, 0.6],
      metallicFactor: 0.7,
      roughnessFactor: 0.1,
    },
    emissiveFactor: [0.05, 0.1, 0.15],
    alphaMode: 'MASK',
    alphaCutoff: 0.4,
    doubleSided: true,
  });
});

test('GLB bounds describe serialized float32 values exactly', () => {
  const data = sampleGeometry();
  data.positions[3] = 1 / 3;
  const { json } = parseGlb(exportGlb(data));
  const triangle = json.meshes[0].primitives.find((primitive) => primitive.mode === 4);
  assert.equal(json.accessors[triangle.attributes.POSITION].max[0], Math.fround(1 / 3));
});

test('GLB rejects material reordering, float32 overflow, and out-of-range vertex colors', () => {
  const missingDefault = sampleGeometry();
  missingDefault.materials = [missingDefault.materials[1]];
  missingDefault.triangleMaterials = [0];
  assert.throws(() => exportGlb(missingDefault), /Default.*index 0/i);

  const overflowing = sampleGeometry();
  overflowing.positions[0] = 1e40;
  assert.throws(() => exportGlb(overflowing), /float32/i);

  const invalidColor = sampleGeometry();
  invalidColor.alphas[0] = 1.1;
  assert.throws(() => exportGlb(invalidColor), /color|alpha|0.*1/i);

  const collapsedTriangle = sampleGeometry();
  collapsedTriangle.positions[3] = 1e-50;
  assert.throws(() => exportGlb(collapsedTriangle), /degenerate/i);
});

test('GLB rejects empty or unsupported geometry instead of silently dropping it', () => {
  assert.throws(() => exportGlb({ ...sampleGeometry(), triangles: [], points: [] }), /empty/i);
  assert.throws(() => exportGlb({ ...sampleGeometry(), triangleMaterials: [] }), /assignment/i);
  assert.throws(() => exportGlb({ ...sampleGeometry(), points: [99] }), /index/i);
});

function sampleGeometry() {
  return {
    positions: [0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 3, 4],
    colors: [1, 0, 0, 0, 1, 0, 0, 0, 1, 0.5, 0.6, 0.7],
    alphas: [0.25, 0.5, 0.75, 0.8],
    triangles: [0, 1, 2],
    points: [3],
    materials: [
      defaultMaterial(),
      {
        name: 'Glass',
        baseColor: [0.2, 0.3, 0.4, 0.6],
        metallic: 0.7,
        roughness: 0.1,
        emissive: [0.05, 0.1, 0.15],
        alphaMode: 'MASK',
        alphaCutoff: 0.4,
      },
    ],
    triangleMaterials: [1],
    pointMaterials: [0],
  };
}

function parseGlb(buffer) {
  const view = new DataView(buffer);
  const jsonLength = view.getUint32(12, true);
  const jsonBytes = new Uint8Array(buffer, 20, jsonLength);
  const json = JSON.parse(new TextDecoder().decode(jsonBytes).trim());
  const binaryOffset = 28 + jsonLength;
  return { json, binary: new Uint8Array(buffer, binaryOffset) };
}

function readAccessor(parsed, accessorIndex) {
  const accessor = parsed.json.accessors[accessorIndex];
  const bufferView = parsed.json.bufferViews[accessor.bufferView];
  const componentCount = { SCALAR: 1, VEC2: 2, VEC3: 3, VEC4: 4 }[accessor.type];
  const offset = (bufferView.byteOffset ?? 0) + (accessor.byteOffset ?? 0);
  return Array.from(new Float32Array(parsed.binary.buffer, parsed.binary.byteOffset + offset, accessor.count * componentCount));
}


test('GLB declares the required WebP extension without inventing a core fallback', () => {
  const geometry = sampleGeometry();
  const textured = {...defaultMaterial(), ...geometry.materials[1]};
  textured.textures = {...defaultMaterial().textures, baseColor: 'data:image/webp;base64,UklGRg=='};
  geometry.materials[1] = textured;
  const {json} = parseGlb(exportGlb(geometry));
  assert.deepEqual(json.extensionsUsed, ['EXT_texture_webp']);
  assert.deepEqual(json.extensionsRequired, ['EXT_texture_webp']);
  assert.equal(json.images[0].mimeType, 'image/webp');
  assert.deepEqual(json.textures[0], {extensions: {EXT_texture_webp: {source: 0}}});
  textured.textures.baseColor = 'data:image/png;base64,iVBORw==';
  const png = parseGlb(exportGlb(geometry)).json;
  assert.equal(png.extensionsRequired, undefined);
  assert.deepEqual(png.textures[0], {source: 0});
});

