// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// FORM LAB owns the Rust parity corpus: the shipped FORM library (examples and structures), the
// script guide's recipes, and authored semantic and rejection probes. The TypeScript runtime in
// `joy-engine/form` is the reference; `rust-form-parity.mjs` evaluates every case in both.

import { createHash } from 'node:crypto';
import { compile, defaultMaterial, materialPbr, meshBox } from 'joy-engine/form';
import { openFormLibraryAsset } from '../src/form-library.ts';
import { RECIPES } from '../src/script-guide.ts';
import { FORM_LIBRARY_FIXTURES } from './form-library-fixtures.js';

/** Absolute and relative tolerance for non-index numeric leaves. */
export const NUMERIC_TOLERANCE = 1e-8;

// Expected inventory; a change means the baseline needs review before it is accepted.
const EXPECTED_INVENTORY = Object.freeze({ examples: 12, structures: 4, recipes: 6 });

const CASE_GROUP = Object.freeze({
  EXAMPLE: 'example',
  RECIPE: 'recipe',
  STRUCTURE: 'structure',
  SEMANTICS: 'semantics',
  ERROR: 'error'
});
const CASE_EXPECT = Object.freeze({ OK: 'ok', ERROR: 'error' });

const EXAMPLE_FOLDER = 'assets/examples';
const STRUCTURE_FOLDER = 'assets/form/structures';
const RECIPE_FILE = 'editor/tools/form-lab/src/script-guide.ts';
const PROBE_FILE = 'editor/tools/form-lab/test/rust-form-corpus.mjs';
const RECIPE_ID_TOKEN = '$id';
const RECIPE_ID_REPLACEMENT = 'study';

// Topology and index integers compare exactly; other numbers use NUMERIC_TOLERANCE.
const EXACT_INTEGER_ARRAYS = new Set(['triangles', 'indices', 'points', 'triangleMaterials', 'pointMaterials', 'faces']);
const EXACT_INTEGER_FIELDS = new Set(['size', 'materialIndex', 'line', 'column']);
const MISMATCH_PREVIEW_CHARACTERS = 180;

/**
 * @typedef {object} CorpusCase
 * @property {string} id Unique, stable case identifier.
 * @property {string} group One of `CASE_GROUP`.
 * @property {string} source FORM source text.
 * @property {Record<string, unknown>} overrides Parameter overrides owned by this case.
 * @property {unknown[]} [materials] Optional palette owned by this case.
 * @property {string} expect `ok` when the reference accepts the case, `error` when it rejects it.
 * @property {Record<string, unknown>} origin Provenance, transformation, and source hash.
 */

/**
 * Collect the 69-case corpus. Each case owns copies of its overrides and palette; no reference
 * GeometryData is retained here.
 * @returns {Promise<CorpusCase[]>} Cases in a stable order.
 */
export async function collectCorpus() {
  const examples = FORM_LIBRARY_FIXTURES.filter(asset => asset.folder === EXAMPLE_FOLDER);
  const structures = FORM_LIBRARY_FIXTURES.filter(asset => asset.folder === STRUCTURE_FOLDER);
  verifyInventory(examples.length, structures.length, RECIPES.length);

  const cases = [];
  for(const asset of examples) {
    cases.push(...exampleCases(asset));
  }
  for(const recipe of RECIPES) {
    cases.push(makeCase(`recipe/${slug(recipe.title)}`, CASE_GROUP.RECIPE, recipe.code.replaceAll(RECIPE_ID_TOKEN, RECIPE_ID_REPLACEMENT), {}, {
      file: RECIPE_FILE,
      name: recipe.title,
      transformation: `Replace every ${RECIPE_ID_TOKEN} with ${RECIPE_ID_REPLACEMENT}, matching the guide preview and document.test.js.`
    }));
  }
  for(const asset of structures) {
    cases.push(structureCase(asset));
  }
  cases.push(...semanticProbes());

  const ids = new Set(cases.map(item => item.id));
  if(ids.size !== cases.length) {
    throw new Error('Corpus IDs must be unique.');
  }
  return cases;
}

/**
 * Compare every GeometryData field, including logical outputs, resources, materials, parameters,
 * and user data. Array length and order and object key sets are exact. Topology and index
 * integers are exact; other finite numbers use max(1e-8, 1e-8*max(|expected|, |actual|)).
 * @param {unknown} expected TypeScript reference value.
 * @param {unknown} actual Rust value.
 * @returns {string | null} The first mismatch path and values, or null when equivalent.
 */
export function compareData(expected, actual) {
  return compareValue(expected, actual, '$', false);
}

/**
 * Fail fast when the shipped library or guide changes size.
 * @param {number} exampleCount
 * @param {number} structureCount
 * @param {number} recipeCount
 */
function verifyInventory(exampleCount, structureCount, recipeCount) {
  const inventory = { examples: exampleCount, structures: structureCount, recipes: recipeCount };
  const changed = Object.entries(EXPECTED_INVENTORY).some(([key, count]) => inventory[key] !== count);
  if(changed) {
    throw new Error(`Corpus inventory changed: found ${JSON.stringify(inventory)}, expected ${JSON.stringify(EXPECTED_INVENTORY)}; review the inventory before accepting a new baseline.`);
  }
}

/**
 * Default and numeric-maximum takes of one example, matching FORM LAB document.test.js.
 * @param {import('../src/form-library.ts').FormLibraryAsset} asset
 * @returns {CorpusCase[]}
 */
function exampleCases(asset) {
  const document = openFormLibraryAsset(asset);
  const stem = asset.filename.replace(/\.form$/i, '');
  const defaults = compile(document.source, document.overrides, document.materials);
  const maximum = {...document.overrides};
  for(const parameter of defaults.parameters) {
    if(!('max' in parameter)) {
      continue;
    }
    maximum[parameter.name] = Array.isArray(parameter.value) ? parameter.value.map(() => parameter.max) : parameter.max;
  }

  const takes = [
    { take: 'default', overrides: document.overrides, transformation: 'Source unchanged.' },
    { take: 'maximum', overrides: maximum, transformation: 'Numeric maximum overrides exactly match FORM LAB document.test.js.' }
  ];
  return takes.map(({ take, overrides, transformation }) => makeCase(`example/${slug(stem)}/${take}`, CASE_GROUP.EXAMPLE, document.source, overrides, {
    file: asset.path,
    name: document.name,
    take,
    transformation
  }, document.materials));
}

/**
 * One structure, opened exactly as FORM LAB's library opens it.
 * @param {import('../src/form-library.ts').FormLibraryAsset} asset
 * @returns {CorpusCase}
 */
function structureCase(asset) {
  const document = openFormLibraryAsset(asset);
  const isScript = asset.filename.toLowerCase().endsWith('.form');
  const stem = asset.filename.replace(/\.(form|formlab)$/i, '');
  return makeCase(isScript ? `structure/${stem}/default` : `structure/${stem}`, CASE_GROUP.STRUCTURE, document.source, document.overrides, {
    file: asset.path,
    name: document.name,
    transformation: isScript ? 'Source unchanged.' : 'Embedded source, overrides, and optional palette as validated by the FORM LAB document reader.'
  }, document.materials);
}

/**
 * Recursive comparison behind `compareData`.
 * @param {unknown} expected
 * @param {unknown} actual
 * @param {string} path JSONPath-like location used in mismatch messages.
 * @param {boolean} exactNumbers True inside exact integer fields.
 * @returns {string | null}
 */
function compareValue(expected, actual, path, exactNumbers) {
  if(typeof expected !== typeof actual || expected === null || actual === null) {
    return expected === actual ? null : mismatch(path, expected, actual);
  }
  if(typeof expected === 'number') {
    return compareNumber(expected, /** @type {number} */ (actual), path, exactNumbers);
  }
  if(Array.isArray(expected) || Array.isArray(actual)) {
    return compareArray(expected, actual, path, exactNumbers);
  }
  if(typeof expected === 'object') {
    return compareObject(/** @type {Record<string, unknown>} */ (expected), /** @type {Record<string, unknown>} */ (actual), path, exactNumbers);
  }
  return expected === actual ? null : mismatch(path, expected, actual);
}

/**
 * @param {number} expected
 * @param {number} actual
 * @param {string} path
 * @param {boolean} exactNumbers
 * @returns {string | null}
 */
function compareNumber(expected, actual, path, exactNumbers) {
  if(!Number.isFinite(expected) || !Number.isFinite(actual)) {
    return mismatch(path, expected, actual);
  }
  const tolerance = exactNumbers ? 0 : Math.max(NUMERIC_TOLERANCE, NUMERIC_TOLERANCE*Math.max(Math.abs(expected), Math.abs(actual)));
  if(Math.abs(expected - actual) <= tolerance) {
    return null;
  }
  return `${mismatch(path, expected, actual)}; tolerance=${tolerance}`;
}

/**
 * @param {unknown} expected
 * @param {unknown} actual
 * @param {string} path
 * @param {boolean} exactNumbers
 * @returns {string | null}
 */
function compareArray(expected, actual, path, exactNumbers) {
  if(!Array.isArray(expected) || !Array.isArray(actual)) {
    return `${path}: array/object type differs`;
  }
  if(expected.length !== actual.length) {
    return `${path}.length: expected ${expected.length}, actual ${actual.length}`;
  }
  for(let index = 0; index < expected.length; index++) {
    const difference = compareValue(expected[index], actual[index], `${path}[${index}]`, exactNumbers);
    if(difference) {
      return difference;
    }
  }
  return null;
}

/**
 * @param {Record<string, unknown>} expected
 * @param {Record<string, unknown>} actual
 * @param {string} path
 * @param {boolean} exactNumbers
 * @returns {string | null}
 */
function compareObject(expected, actual, path, exactNumbers) {
  const expectedKeys = Object.keys(expected).sort();
  const actualKeys = Object.keys(actual).sort();
  const sameKeys = expectedKeys.length === actualKeys.length && expectedKeys.every((key, index) => key === actualKeys[index]);
  if(!sameKeys) {
    return `${path}: keys differ; expected ${JSON.stringify(expectedKeys)}, actual ${JSON.stringify(actualKeys)}`;
  }
  for(const key of expectedKeys) {
    const exact = EXACT_INTEGER_FIELDS.has(key) || (EXACT_INTEGER_ARRAYS.has(key) && Array.isArray(expected[key]));
    const difference = compareValue(expected[key], actual[key], `${path}.${key}`, exact);
    if(difference) {
      return difference;
    }
  }
  return null;
}

/**
 * @param {string} path
 * @param {unknown} expected
 * @param {unknown} actual
 * @returns {string}
 */
function mismatch(path, expected, actual) {
  return `${path}: expected ${preview(expected)}, actual ${preview(actual)}`;
}

/** @param {unknown} value @returns {string} */
function preview(value) {
  const text = JSON.stringify(value) ?? String(value);
  return text.length > MISMATCH_PREVIEW_CHARACTERS ? `${text.slice(0, MISMATCH_PREVIEW_CHARACTERS - 3)}...` : text;
}

/** @param {string} value @returns {string} */
function slug(value) {
  return value.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');
}

/**
 * Create a case that owns copies of its inputs and records its source hash.
 * @param {string} id
 * @param {string} group
 * @param {string} source
 * @param {Record<string, unknown>} overrides
 * @param {Record<string, unknown>} origin
 * @param {unknown[]} [materials]
 * @param {string} [expect]
 * @returns {CorpusCase}
 */
function makeCase(id, group, source, overrides, origin, materials, expect = CASE_EXPECT.OK) {
  const palette = materials === undefined ? {} : { materials: structuredClone(materials) };
  return {
    id,
    group,
    source,
    overrides: structuredClone(overrides),
    expect,
    ...palette,
    origin: {...origin, source_sha256: createHash('sha256').update(source).digest('hex')}
  };
}

/**
 * Authored compatibility probes: 17 accepted semantics cases and 18 rejections.
 * @returns {CorpusCase[]}
 */
function semanticProbes() {
  const cases = [];
  const add = (name, source, overrides = {}, materials = undefined, expect = CASE_EXPECT.OK) => {
    const group = expect === CASE_EXPECT.ERROR ? CASE_GROUP.ERROR : CASE_GROUP.SEMANTICS;
    cases.push(makeCase(`probe/${name}`, group, source, overrides, {
      file: PROBE_FILE,
      name,
      transformation: 'Authored compatibility probe; not a shipped asset.'
    }, materials, expect));
  };
  const reject = (name, source, overrides = {}, materials = undefined) => add(name, source, overrides, materials, CASE_EXPECT.ERROR);

  // Accepted semantics: numeric rules, scopes, metadata, state restoration, resources, and procedural ownership.
  add('numeric-precedence-round-remainder', `point(-2^2,2^3^2,round(-1.5))
point(-5%3,5%-3,2+3*4)
point(pow(2,3),atan2(1,1),smoothstep(0,1,0.25))
point(clamp(3,2,1),floor(-1.2),ceil(-1.2))`);
  add('vector-color-and-math', `let a = vector(2,3,4)
let b = 2*a - vector(1,1,1)
let c = cross(a,vector(0,1,0))
color(mix(rgba(0,0.2,0.4,1),rgba(1,0.8,0.6,0.5),0.25))
point(b.x,b.y,b.z) point(c.x,c.y,c.z)
let n = normalize(vector(1e100,2e100,-3e100))
point(n.x,n.y,n.z) point(length(a),dot(a,a),noise(0.3,-1.2,7))`);
  add('seed-wrapping-and-scopes', `seed(-4294967297)
repeat 3 as i { point(rand(),rand(),i) }
seed(4294967303)
let value = 2
repeat 2 as i { let value = value + i if(i) { let value = 99 point(value,rand(),0) } point(value,rand(),1) }
point(value,rand(),2)`);
  add('comments-strings-and-user-data', `// LF and semicolons remain supported.
project meta(name="Café 😀",info="ESCAPED DATA",title="Owned metadata")
userData({text:"quote: \\" / \\u03bb",values:[true,false,null,{a:1}],vector:vector(1,2,3)})
point(.5,1.,1e-3); # Keep metadata through immediate geometry.
with userData({kind:"inner"}) { point(1,0,0) userData(null) point(2,0,0) }
point(3,0,0) userData() point(4,0,0)`);
  add('legacy-parameters-clamping-shadowing', `param size = 2,0,4,0.25
param offset = vector(1,2,3) meta(min=-4,max=4,step=0.1)
param tint = rgba(0.2,0.4,0.6,0.8)
let size = size + 1
color(tint) point(size,offset.y,offset.z)`, { size: 99, offset: [-99, 1, 99], tint: [-1, 0.5, 2, 0.25], removed_parameter: 7 });
  add('immediate-state-restoration', `color(2,-1,0.5,0.25) uv(0.2,0.3) normal(0,3,4)
sphere(0,0,0,1,5) point(2,0,0)
instance(meshBox(1,1,1),vector(4,0,0),vector(0.2,0.3,0.4),vector(-2,1,0.5))
point(6,0,0) normal() surface(1,1,2,2,x+z) point(7,0,0)`);
  add('mesh-transform-deform-copy', `let original = meshBox(2,2,2)
let deformed = deform(original,x,y+x*x,z)
let moved = transformMesh(deformed,vector(4,2,0),vector(0.2,-0.3,0.4),vector(-2,1,0.5))
instance(original) instance(moved)`);
  add('geometry-alias-and-copy-ownership', `let original = box(2,2,2)
let alias = original
translate(alias,vector(1,0,0))
let owned = copy(original)
inspect("Before",original)
translate(original,vector(4,0,0))
output("Copy",owned)
output("After",original)`);
  add('retained-instance-resource-ownership', `let source = box(1,2,1)
let sites = geometry()
addPoint(sites,vector(2,0,0))
setUserData(sites,0,{kind:"site"})
let assembly = instances(source,sites,materialPbr(rgba(0.2,0.4,0.6,0.5),0.3,0.7))
translate(source,vector(99,0,0)) translate(sites,vector(99,0,0))
let duplicate = copy(assembly)
addAttribute(assembly,"instances","scale",vector(1,1,1))
wrangle instances in assembly as site { site.scale = vector(2,1,1) }
inspect("OriginalSiteSnapshot",duplicate) output("Scaled",assembly)`);
  add('topology-attributes-corner-priority', `let g = geometry()
addAttribute(g,"points","weight",9)
addPoint(g,vector(0,0,0)) addPoint(g,vector(1,0,0)) addPoint(g,vector(0,1,0)) addPoint(g,vector(1,1,0))
addFace(g,0,1,2) addFace(g,1,3,2)
addAttribute(g,"points","color",rgba(0,1,0,1))
addAttribute(g,"faces","color",rgba(1,0,0,1))
addAttribute(g,"corners","color",rgba(0,0,1,1))
addAttribute(g,"corners","uv",vector(0,0,0))
wrangle corners in g as corner { corner.uv = vector(corner.index/6,0.2,0) }
setAttribute(g,"points","weight",1,7)
inspect("Before",g) scale(g,vector(-1,2,1)) removeFace(g,0) removePoint(g,0)
output("After",g)`);
  add('attributes-own-numeric-reads', `let g = grid(1,1,2,2)
addAttribute(g,"points","toString",0)
wrangle points in g as point { point.toString = point.index }
let saved = attribute(g,"points","position",0)
translate(g,vector(10,0,0))
point(saved.x,saved.y,saved.z) output("Grid",g)`);
  add('weighted-scatter-local-random-stream', `seed(19)
let terrain = grid(2,2,4,4)
addAttribute(terrain,"points","density",0)
color(terrain,rgba(0.2,0.4,0.6,1))
wrangle points in terrain as point { point.density = (point.position.x + 2)/4 }
let sites = scatter(terrain,12,42,"density")
point(rand(),rand(),rand()) inspect("Sites",sites)
output("Instances",instances(meshBox(0.1,0.2,0.1),sites))`);
  add('sphere-topology-and-isolated-points', `sphere(0,0,0,1,5)
output("ObjectSphere",sphere(1,5))
output("MeshGeometry",geometry(meshSphere(1,5)))`);
  add('user-data-null-and-output-snapshots', `userData({kind:"ambient"})
let g = geometry() addPoint(g,vector(0,0,0)) addPoint(g,vector(1,0,0))
setUserData(g,0,{kind:"first"}) setUserData(g,1,null)
inspect("Before",g) setUserData(g,0,{kind:"changed"})
let saved = userData(g,0) setUserData(g,0,{kind:"changed-again"})
userData(saved) point(0,0,0)
output("After",g)`);
  add('unexecuted-bodies-and-discarded-values', `if(0) { unavailable(1) }
repeat 0 as i { unavailable(2) }
box(1,1,1)
point(1,2,3)`);

  // Accepted semantics: host-supplied resources and palettes.
  const piece = meshBox(0.5, 0.75, 1);
  const finish = materialPbr([0.4, 0.2, 0.8, 0.6], 0.25, 0.7);
  add('all-five-parameter-resource-overrides', `param count = 2 meta(min=1,max=5,step=1)
param origin = vector(0,0,0) meta(min=-10,max=10,step=0.1)
param tint = rgba(0.2,0.4,0.6,1)
param piece = meshBox(1,1,1)
param finish = materialPbr(tint,0,0.8)
repeat count as i { instance(piece,origin + vector(i*2,0,0),vector(0,0,0),vector(1,1,1),finish) }`, {
    count: 3,
    origin: [1, 2, 3],
    tint: [0.6, 0.4, 0.2, 0.5],
    piece,
    finish
  });
  const paint = {...defaultMaterial(), name: 'Paint', baseColor: [0.3, 0.4, 0.5, 0.6], metallic: 0.2, roughness: 0.7, alphaMode: 'BLEND'};
  paint.textures.baseColor = 'data:image/png;base64,iVBORw0KGgo=';
  add('external-palette-and-material-binding', `material(Paint) box(0,0,0,1,1,1)
material() point(2,0,0)
let Paint = materialPbr(rgba(1,0,0,0.5),0.3,0.4)
material(Paint) point(3,0,0)`, {}, [paint]);

  // Rejections: acceptance and one-based line/column must match.
  reject('error-unknown-binding-location', 'point(0,0,0)\npoint(missing,0,0)');
  reject('error-unknown-command', 'unknownCommand(1)');
  reject('error-nonfinite-arithmetic', 'point(1/0,0,0)');
  reject('error-command-arity', 'point(1,2)');
  reject('error-repeat-fraction', 'repeat 1.5 as i { point(i,0,0) }');
  reject('error-repeat-budget', 'repeat 150001 as i { point(i,0,0) }');
  reject('error-coordinate-bound', 'point(1000001,0,0)');
  reject('error-nested-parameter', 'repeat 1 as i { param size = 1 meta(min=0,max=2,step=1) }');
  reject('error-zero-instance-scale', 'instance(meshBox(1,1,1),vector(0,0,0),vector(0,0,0),vector(0,1,1))');
  reject('error-vector-override-shape', 'param offset = vector(0,0,0) meta(min=-1,max=1,step=0.1)', { offset: [1, 2] });
  reject('error-default-invalid-despite-override', 'param size = 5 meta(min=0,max=2,step=1)', { size: 1 });
  reject('error-mesh-override-index', 'param piece = meshBox(1,1,1) instance(piece)', { piece: {...piece, indices: [0, 1, 99999]} });
  reject('error-material-factor', 'let finish = materialPbr(rgba(1,1,1,1),2,0.5)');
  reject('error-wrangle-undeclared-attribute', 'let g = box(1,1,1)\nwrangle points in g as point { point.density = 1 }');
  reject('error-wrangle-topology-edit', 'let g = box(1,1,1) wrangle points in g as point { addPoint(g,vector(0,0,0)) }');
  reject('error-record-prototype-key', 'userData({"__proto__":1}) point(0,0,0)');
  reject('error-binding-does-not-escape', 'if(1) { let inner = 2 }\npoint(inner,0,0)');
  reject('error-utf16-column', 'userData({label:"😀"}) point(missing,0,0)');
  return cases;
}
