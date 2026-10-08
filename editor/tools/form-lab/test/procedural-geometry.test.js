// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import test from 'node:test';
import assert from 'node:assert/strict';
import { compile, compileFormScript } from 'joy-engine/form';

test('wrangles require named bindings and only read or update explicitly created attributes', () => {
  for(const operation of ['point.color = rgba(1,0,0,1)', 'let tint = point.color']) {
    assert.throws(() => compile(`let g = grid(1,1,2,2)\nwrangle points in g as point {\n  ${operation}\n}`), error => {
      assert.match(error.message, /Unknown points attribute 'color'/);
      assert.equal(error.line, 3);
      return true;
    });
  }
  assert.throws(() => compile('let g = grid(1,1,2,2) wrangle points g {}'), /Expected 'in'/);
  assert.throws(() => compile('let g = grid(1,1,2,2) setAttribute(g,"points","weight",0,1)'), /Unknown points attribute 'weight'/);
  const result = compile(`let g = grid(1,1,2,2)
    addAttribute(g, "points", "weight", 2)
    color(g, rgba(1,1,1,1))
    wrangle points in g as element { element.weight = element.weight + element.index }
    wrangle points in g as point { point.color = rgba(point.weight/5,0,0,1) }
    output("G",g)`);
  assert.deepEqual(result.outputs[0].value.attributes.points.weight.values, [2,3,4,5]);
  assert.deepEqual(result.outputs[0].value.attributes.points.color.values[3], [1,0,0,1]);
});

test('attribute declarations preserve domain, shape, copies, and empty geometry schemas', () => {
  const result = compile(`let g = geometry()
    addAttribute(g,"points","weight",2)
    addPoint(g,vector(0,0,0))
    setAttribute(g,"points","weight",0,3)
    let duplicate = copy(g)
    wrangle points in duplicate as element { element.weight = 4 }
    let assembly = instances(meshBox(1,1,1),g)
    addAttribute(assembly,"instances","scale",vector(1,1,1))
    wrangle instances in assembly as element { element.scale = vector(2,1,1) }
    output("Original",g)
    output("Copy",duplicate)
    output("Assembly",assembly)`);
  assert.deepEqual(result.outputs[0].value.attributes.points.weight.values, [3]);
  assert.deepEqual(result.outputs[1].value.attributes.points.weight.values, [4]);
  assert.deepEqual(result.outputs[2].value.sites.attributes.points.scale.values, [[2,1,1]]);
  assert.throws(() => compile('let g=meshBox(1,1,1) addAttribute(g,"points","weight",1)'), /editable geometry/);
  for(const operation of [
    'addAttribute(g,"points","position",vector(0,0,0))',
    'addAttribute(g,"points","color",1)',
    'addAttribute(g,"points","weight",0) wrangle points in g as element { element.weight = vector(1,2,3) }',
    'addAttribute(g,"faces","weight",0) wrangle points in g as element { element.weight = 1 }',
    'wrangle points in g as element { addAttribute(g,"points","weight",0) }',
    'wrangle points in g as element { color(g,rgba(1,1,1,1)) }'
  ]) {
    assert.throws(() => compile(`let g = grid(1,1,2,2) ${operation}`));
  }
});

const recipe = `
param height = 1 meta(min=0, max=4, step=0.1)
param piece = meshBox(0.1, 0.2, 0.1)
let terrain = grid(4, 4, 8, 8)
addAttribute(terrain,"points","density",0)
color(terrain,rgba(1,1,1,1))
wrangle points in terrain as element {
  element.position = vector(element.position.x, cos(element.position.x)*height, element.position.z)
  element.density = clamp((element.position.y + 1)/2, 0, 1)
  element.color = rgba(element.density, 0.4, 0.2, 1)
}
inspect("Before", terrain)
let alternate = copy(terrain)
wrangle points in alternate as element { element.position = vector(element.position.x, element.position.y + 10, element.position.z) }
let sites = scatter(terrain, 20, 7777, "density")
addAttribute(sites,"points","scale",vector(1,1,1))
wrangle points in sites as element { element.scale = vector(1, 1 + element.density, 1) }
let pieces = instances(piece, sites, materialPbr(rgba(1, 1, 1, 1), 0, 0.8))
addAttribute(pieces,"instances","rotation",vector(0,0,0))
wrangle instances in pieces as element { element.rotation = vector(0, element.index*0.2, 0) }
output("Terrain", terrain)
inspect("Sites", sites)
output("Pieces", pieces)
`;

test('procedural values retain attributes and instances across deterministic runtime evaluation and owned snapshots', () => {
  const program = compileFormScript(recipe);
  const result = program.evaluate();
  assert.equal(result.outputs.length, 4);
  const terrain = result.outputs[1].value;
  assert.equal(terrain.attributes.points.position.values.length, 25);
  assert.equal(terrain.faces.length, 32);
  assert.equal(result.outputs[2].value.attributes.points.density.values.length, 20);
  assert.equal(result.outputs[3].value.kind, 'instances');
  assert.equal(result.outputs[3].value.sites.attributes.points.rotation.values.length, 20);
  assert.deepEqual(program.evaluate(), result);
  const before = structuredClone(result.outputs[0]);
  terrain.attributes.points.position.values[0][1] = 999;
  assert.deepEqual(result.outputs[0], before);
  assert.notDeepEqual(program.evaluate({height: 2}).positions, program.evaluate().positions);
  assert.deepEqual(JSON.parse(JSON.stringify(before)), before);
});

test('topology editing and corner attributes preserve seams and explicit copies', () => {
  const result = compile(`
    let g = geometry()
    let a = addPoint(g, vector(0, 0, 0))
    let b = addPoint(g, vector(1, 0, 0))
    let c = addPoint(g, vector(0, 0, 1))
    addFace(g, a, c, b)
    addFace(g, a, b, c)
    addAttribute(g,"corners","uv",vector(0,0,0))
    addAttribute(g,"faces","tag",0)
    wrangle corners in g as element { element.uv = vector(element.index/6, 0, 0) }
    wrangle faces in g as element { element.tag = element.index + 10 }
    let duplicate = copy(g)
    removeFace(duplicate, 1)
    removePoint(duplicate, 1)
    inspect("Empty faces", duplicate)
    output("Seams", g)
  `);
  assert.equal(result.outputs[0].value.faces.length, 0);
  assert.equal(result.outputs[0].value.attributes.points.position.values.length, 2);
  assert.equal(result.outputs[1].value.attributes.corners.uv.values.length, 6);
  assert.equal(result.positions.length/3, 6);
  assert.notEqual(result.uvs[0], result.uvs[6]);
  assert.deepEqual(result.outputs[1].value.attributes.faces.tag.values, [10, 11]);
});

test('procedural scripts reject unsafe work and malformed attributes with source locations', () => {
  for(const source of [
    'let g = grid(1,1,2,2) wrangle points in g as element { element.position = 1 }',
    'let g = grid(1,1,2,2) addAttribute(g,"points","a",vector(0,0,0)) wrangle points in g as element { element.a = vector(1,2,3) element.a = 1 }',
    'let g = geometry() addFace(g,0,1,2)',
    'let g = grid(1,1,2,2) wrangle points in g as element { addPoint(g,vector(0,0,0)) }',
    'let g = grid(1,1,2,2) wrangle points in g as element { element.position = vector(1e9,0,0) }',
    'let g = grid(256,256,2,2) let a = copy(g) let b = copy(g)',
    'let g = grid(1,1,2,2) let p = scatter(g,10,1,"missing")',
    'let g = grid(1,1,2,2) output("X",g) output("X",g)'
  ]) {
    assert.throws(() => compile(source), error => error instanceof Error && 'line' in error);
  }
  const empty = compile('let g = grid(1,1,2,2) addAttribute(g,"points","density",0) wrangle points in g as element { element.density = 0 } let p = scatter(g,10,1,"density") output("Empty",p)');
  assert.equal(empty.positions.length, 0);
  const filtered = compile('let g = grid(1,1,2,2) addAttribute(g,"points","tag",0) wrangle points in g as element { element.tag = 0 if element.index >= 2 { element.tag = 1 } } output("G",g)');
  assert.deepEqual(filtered.outputs[0].value.attributes.points.tag.values, [0,0,1,1]);
});

test('vector math and seeded fields support reusable geometry recipes without component boilerplate', () => {
  const result = compile(`
    let g = grid(1,1,2,2)
    addAttribute(g,"points","direction",vector(0,0,0))
    addAttribute(g,"points","distance",0)
    addAttribute(g,"points","noise",0)
    color(g,rgba(1,1,1,1))
    wrangle points in g as element {
      element.position = element.position*2 + vector(0,3,0)
      element.direction = normalize(vector(0,4,0))
      element.distance = length(element.position)
      element.noise = noise(element.position.x, element.position.y, 7777)
      element.color = mix(rgba(0,0,0,1), rgba(1,1,1,1), smoothstep(-1,1,element.noise))
    }
    output("Math",g)
  `);
  const attributes = result.outputs[0].value.attributes.points;
  assert.deepEqual(attributes.position.values[0], [-2,3,-2]);
  assert.deepEqual(attributes.direction.values[0], [0,1,0]);
  assert.ok(attributes.noise.values.every(value => value >= -1 && value <= 1));
  assert.throws(() => compile('let x = vector(1,2,3) + rgba(1,1,1,1)'), /component/);
  assert.throws(() => compile('let x = vector(1,2,3)/0'), /finite/);
});

test('object-first primitives support explicit styling, transforms, and named wrangle elements', () => {
  const result = compile(`
    let shape = box(2, 4, 6)
    scale(shape, vector(2, 0.5, 1))
    rotate(shape, vector(0, pi/2, 0))
    translate(shape, vector(3, 5, 7))
    color(shape, rgba(0.2, 0.4, 0.6, 1))
    addAttribute(shape,"points","weight",0)
    wrangle points in shape as point {
      point.position = point.position + vector(0, point.index, 0)
      point.weight = point.index
    }
    output("Shape", shape)
  `);
  const shape = result.outputs[0].value;
  assert.deepEqual(shape.attributes.points.color.values[0], [0.2, 0.4, 0.6, 1]);
  assert.deepEqual(shape.attributes.points.weight.values, shape.attributes.points.weight.values.map((_, index) => index));
  assert.ok(result.positions.every(Number.isFinite));
  assert.ok(result.normals.every(Number.isFinite));
  assert.throws(() => compile('let shape = box(1,1,1) scale(shape, vector(1,0,1))'), /nonzero/);
  assert.throws(() => compile('let shape = box(1,1,1) wrangle points in shape as point { other.position = point.position }'), /binding/);
});


test('normal-driven wrangles retain readable input normals and all explicitly authored output normals', () => {
  const displaced = compile('let g=geometry(meshSphere(1,8)) wrangle points in g as element { element.position=element.position+element.normal*0.1 } output("G",g)');
  assert.ok(displaced.positions.every(Number.isFinite));
  const authored = compile('let g=grid(1,1,2,2) addAttribute(g,"points","normal",vector(0,0,0)) wrangle points in g as element { element.position=element.position+vector(0,1,0) element.normal=vector(0,1,0) } output("G",g)');
  assert.deepEqual(authored.normals, Array.from({length:4}, () => [0,1,0]).flat());
  const extreme = compile(`let g=grid(1,1,2,2) addAttribute(g,"points","normal",vector(0,0,0)) wrangle points in g as element { element.normal=vector(1e308,0,0) }
    let p=geometry() addPoint(p,vector(0,0,0)) addAttribute(p,"points","scale",vector(1,1,1)) wrangle points in p as element { element.scale=vector(1e-8,1,1) }
    let assembly=instances(g,p) output("A",assembly)`);
  assert.ok(extreme.normals.every(Number.isFinite));
  assert.deepEqual(extreme.normals, Array.from({length:4}, () => [1,0,0]).flat());
});

test('face render attributes and legal custom names survive copying and instance realization', () => {
  const result = compile(`
    color(1,1,1)
    let g=grid(1,1,2,2)
    addAttribute(g,"points","toString",0)
    color(g,rgba(1,1,1,1))
    wrangle points in g as element { element.toString=7 element.color=rgba(0,1,0,1) }
    let duplicate=copy(g)
    addAttribute(duplicate,"faces","color",rgba(0,0,0,0))
    addAttribute(duplicate,"faces","normal",vector(0,0,0))
    addAttribute(duplicate,"corners","color",rgba(0,0,0,0))
    wrangle faces in duplicate as element { element.color=rgba(1,0,0,1) element.normal=vector(0,1,0) }
    wrangle corners in duplicate as element { if element.index == 0 { element.color=rgba(0,0,1,1) } }
    output("Faces",g)
    output("Overrides",duplicate)
  `);
  assert.deepEqual(result.outputs[0].value.attributes.points.toString.values, [7,7,7,7]);
  const faces = compile('color(1,1,1) let g=grid(1,1,2,2) addAttribute(g,"faces","color",rgba(0,0,0,0)) wrangle faces in g as element { element.color=rgba(1,0,0,1) } output("G",g)');
  assert.deepEqual(faces.colors, Array.from({length:6}, () => [1,0,0]).flat());
  assert.deepEqual(result.outputs[1].geometry.colors.slice(0,3), [0,0,1]);
});

test('profile and path generators produce owned, bounded geometry with smooth normals and UV seams', () => {
  const source = `let profile = geometry()
    addPoint(profile, vector(1, 0, 0))
    addPoint(profile, vector(1, 2, 0))
    let vessel = revolve(profile, 8)
    let path = geometry()
    addPoint(path, vector(0, 0, 0))
    addPoint(path, vector(0, 2, 0))
    addPoint(path, vector(1, 3, 0))
    let pipe = tube(path, 0.25, 8)
    wrangle points in profile as element { element.position = element.position*2 }
    output("Vessel", vessel)
    output("Pipe", pipe)`;
  const program = compileFormScript(source);
  const data = program.evaluate();
  assert.deepEqual(program.evaluate(), data);
  const vessel = data.outputs[0].value;
  assert.equal(vessel.faces.length, 16);
  assert.equal(vessel.attributes.points.position.values.length, 16);
  assert.equal(vessel.attributes.corners.uv.values.length, 48);
  assert.ok(vessel.attributes.corners.uv.values.some(uv => uv[0] === 1));
  for(const output of data.outputs) {
    const geometry = output.value;
    for(const normal of geometry.attributes.points.normal.values) {
      assert.ok(Math.abs(Math.hypot(...normal) - 1) < 1e-6);
    }
  }
  assert.ok(vessel.attributes.points.normal.values[0][0] > 0.9, 'outward winding');
  for(const expression of ['revolve(profile, 1000000)', 'tube(profile, 0, 8)', 'tube(profile, 1, 2)']) {
    assert.throws(() => compile(`let profile = grid(1,1,1,1)\noutput("Bad", ${expression})`));
  }
  assert.throws(() => compile('let p = geometry()\naddPoint(p,vector(0,0,0))\naddPoint(p,vector(0,0,0))\noutput("Bad",tube(p,1,8))'), /distinct|duplicate/);
});


test('point user data follows copies, topology edits, named outputs, and instances', () => {
  const result = compile(`
    let sites = geometry()
    addPoint(sites, vector(0, 0, 0))
    addPoint(sites, vector(1, 0, 0))
    setUserData(sites, 0, {kind: "shrine"})
    setUserData(sites, 1, {kind: "home", settings: {capacity: 4}})
    let homes = copy(sites)
    removePoint(homes, 0)
    let capacity = userData(homes, 0).settings.capacity
    setUserData(sites, 1, {kind: "tree"})
    output("Homes", smoothNormals(homes))
    setUserData(homes, 0, {kind: "changed"})
    output("Sites", sites)
    let marker = geometry()
    addPoint(marker, vector(0, capacity, 0))
    output("Instances", instances(marker, sites))
  `);
  assert.deepEqual(result.userData, [
    {kind: 'home', settings: {capacity: 4}}, {kind: 'shrine'}, {kind: 'tree'},
    {kind: 'shrine'}, {kind: 'tree'}
  ]);
  assert.deepEqual(result.outputs[0].value.userData, [{kind: 'home', settings: {capacity: 4}}]);
  assert.deepEqual(result.outputs[0].geometry.userData, result.outputs[0].value.userData);
  assert.equal(result.outputs[2].geometry.positions[1], 4);
  assert.throws(() => compile('let g = geometry() setUserData(g, 0, {})'), /integer/);
});

test('retired attribute syntax reports migration diagnostics before evaluation', () => {
  for(const operation of ['@position = vector(0,0,0)', 'let position = @position']) {
    const source = `let g = geometry()\nwrangle points in g as point {\n  ${operation}\n}`;
    assert.throws(() => compileFormScript(source), error => {
      assert.match(error.message, /@.*no longer supported/);
      assert.match(error.message, /as.*point/);
      assert.equal(error.line, 3);
      assert.equal(error.column, operation.indexOf('@') + 3);
      return true;
    });
  }
  assert.throws(() => compileFormScript('let g = geometry()\nwrangle points in g {}'), error => {
    assert.match(error.message, /Expected 'as'/);
    assert.equal(error.line, 2);
    return true;
  });
  const result = compile('userData({label: "@position"}) # @position is text here\npoint(0,0,0)');
  assert.deepEqual(result.userData, [{label: '@position'}]);
});
