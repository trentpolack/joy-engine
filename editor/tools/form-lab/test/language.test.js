import test from 'node:test';
import assert from 'node:assert/strict';
import { compile } from 'joy-engine/form';
import { exportObj, exportPly } from '../src/export.ts';

test('expressions, lexical loops, and point data', () => {
  const mesh = compile(
    'let size = 2 + 3 * 4\nrepeat 3 as i { point(i, size, sin(pi/2)) }',
  );
  assert.deepEqual(mesh.positions, [0, 14, 1, 1, 14, 1, 2, 14, 1]);
  assert.equal(mesh.points.length, 3);
});
test('surface generates indexed geometry with evaluated heights', () => {
  const mesh = compile('surface(2, 2, 4, 4, x+z)');
  assert.equal(mesh.positions.length, 27);
  assert.equal(mesh.triangles.length, 24);
  assert.deepEqual(mesh.positions.slice(0, 3), [-2, -4, -2]);
});
test('random output is reproducible and independent per compile', () => {
  const script = 'seed(42)\nrepeat 10 as i { point(rand(),rand(),rand()) }';
  assert.deepEqual(compile(script), compile(script));
});
test('nested loops preserve outer variables', () => {
  const mesh = compile('repeat 2 as i { repeat 2 as j { point(i,j,0) } }');
  assert.deepEqual(mesh.positions, [0, 0, 0, 0, 1, 0, 1, 0, 0, 1, 1, 0]);
});
test('reports source location for unknown names', () => {
  assert.throws(
    () => compile('point(0,0,0)\npoint(missing,0,0)'),
    (error) => error.line === 2 && /Unknown/.test(error.message),
  );
});
test('rejects nonfinite geometry', () => {
  assert.throws(() => compile('point(1/0,0,0)'), /finite/);
});
test('authored normals normalize, persist, reset to flat, and remain isolated per build', () => {
  const mesh = compile('normal(0,3,4) triangle(0,0,0,1,0,0,0,1,0) normal() point(1,2,3)');
  assert.deepEqual(mesh.normals, [0, 0.6, 0.8, 0, 0.6, 0.8, 0, 0.6, 0.8, 0, 0, 0]);
  assert.deepEqual(compile('point(0,0,0)').normals, [0, 0, 0]);
  for(const source of ['normal(0,0,0)', 'normal(1,2)', 'normal(1/0,0,0)']) {
    assert.throws(() => compile(source), error => error.line === 1 && /normal|finite/.test(error.message));
  }
});
test('rejects arbitrary JavaScript and oversized work', () => {
  assert.throws(() => compile('fetch("https://example.com")'));
  assert.throws(
    () => compile('repeat 10000000 as i { point(0,0,0) }'),
    /limit|between/,
  );
  assert.throws(
    () => compile('repeat 1000 as i { repeat 1000 as j { point(i,j,0) } }'),
    /limit/,
  );
});
test('rejects invalid topology and arity', () => {
  assert.throws(() => compile('surface(0,2,4,4,0)'), /between/);
  assert.throws(() => compile('point(1,2)'), /3 arguments/);
});
test('OBJ preserves triangle and point topology', () => {
  const text = exportObj(compile('point(0,0,0)\ntriangle(0,0,0,1,0,0,0,1,0)'));
  assert.match(text, /p 1/);
  assert.match(text, /f 2 3 4/);
});
test('PLY carries per-vertex colors and faces', () => {
  const text = exportPly(compile('color(1,0.5,0)\nbox(0,0,0,2,2,2)'));
  assert.match(text, /element vertex 8/);
  assert.match(text, /element face 12/);
  assert.match(text, /255 128 0/);
});

test('named metadata and typed parameters generate geometry from components', () => {
  const mesh = compile(
    `param size = 2 meta(step=0.1, max=5, min=1)
param offset = vector(1, 2, 3) meta(min=-5, max=5, step=0.1)
param tint = rgba(1, 0.5, 0, 0.25)
color(tint)
point(offset.x, offset.y * size, offset.z)`,
    { offset: [3, 4, 99] },
  );
  assert.deepEqual(mesh.positions, [3, 8, 5]);
  assert.deepEqual(mesh.colors, [1, 0.5, 0]);
  assert.deepEqual(mesh.alphas, [0.25]);
  assert.deepEqual(
    mesh.parameters.map((parameter) => parameter.type),
    ['scalar', 'vector', 'color'],
  );
  assert.deepEqual(mesh.parameters[1].defaultValue, [1, 2, 3]);
});

test('typed declarations reject invalid metadata, components, and override shapes', () => {
  assert.throws(
    () => compile('param p = 1 meta(min=0, min=1, max=2, step=1)'),
    /Duplicate/,
  );
  assert.throws(
    () => compile('param p = 1 meta(low=0, max=2, step=1)'),
    /metadata/,
  );
  assert.throws(() => compile('param p = rgba(1, 0, 0, 2)'), /0.*1/);
  assert.throws(
    () =>
      compile(
        'param p = vector(1,2,3) meta(min=0,max=5,step=1) point(p.r,0,0)',
      ),
    /component/,
  );
  assert.throws(
    () =>
      compile('param p = vector(1,2,3) meta(min=0,max=5,step=1)', {
        p: [1, 2],
      }),
    /components/,
  );
});

test('project metadata travels with generated geometry and decodes escaped text', () => {
  const mesh = compile(
    'project meta(name="Wave study", info="SURFACE / 001", title="MAKE\\nWAVES.")\npoint(0,0,0)',
  );
  assert.deepEqual(mesh.project, {
    name: 'Wave study',
    info: 'SURFACE / 001',
    title: 'MAKE\nWAVES.',
  });
});

test('project metadata rejects duplicate declarations, nontext fields, and nested declarations', () => {
  for(const source of [
    'project meta(name="A") project meta(name="B")',
    'project meta(name="A", name="B")',
    'project meta(name=12)',
    'project meta(name="A", unknown="B")',
    'repeat 1 as i { project meta(name="A") }',
  ]) {
    assert.throws(() => compile(source));
  }
});

test('parameter bounds validate script defaults and clamp overrides before geometry evaluation', () => {
  assert.throws(
    () =>
      compile('param height = 8 meta(min=0,max=5,step=1) point(0,height,0)'),
    /min ≤ default ≤ max/,
  );
  const source = 'param height = 2 meta(min=0,max=5,step=1) point(0,height,0)';
  assert.deepEqual(compile(source, { height: 8 }).positions, [0, 5, 0]);
  // let is a separate lexical binding, not a constrained parameter assignment.
  assert.deepEqual(
    compile(`${source}\nlet height = 9\npoint(0,height,0)`).positions,
    [0, 2, 0, 0, 9, 0],
  );
});
test('user data survives independent emissions, evaluation, and JSON export', () => {
  const source = `
    let home = {kind: "home", settings: {capacity: 2 + 2, enabled: true}, tags: ["village", 7, null]}
    userData(home)
    point(0, 0, 0)
    color(0, 1, 0)
    point(1, 0, 0)
    userData()
    point(2, 0, 0)
    userData({kind: "tree", position: vector(1, 2, 3)})
    point(3, 0, 0)
  `;
  const data = compile(source);
  const expected = {kind: 'home', settings: {capacity: 4, enabled: true}, tags: ['village', 7, null]};
  assert.deepEqual(data.userData[0], expected);
  assert.deepEqual(JSON.parse(JSON.stringify(data)).userData[1], expected);
  assert.equal(data.userData[2], null);
  assert.deepEqual(data.userData[3], {kind: 'tree', position: [1, 2, 3]});
  data.userData[0].settings.capacity = 99;
  assert.deepEqual(data.userData[1], expected);
  assert.deepEqual(compile(source).userData[0], expected);
});

test('scoped user data restores enclosing records and keeps local bindings inside the block', () => {
  const data = compile(`
    let homeData = {
      kind: "home",
      settings: {capacity: 4}
    }
    let height = 2
    userData({kind: "world"})
    with userData(homeData) {
      let height = 5
      repeat 2 as i { point(i, height, 0) }
      with userData({kind: "tree"}) { point(2, 0, 0) }
      point(3, 0, 0)
      with userData(null) { point(4, 0, 0) }
      userData({kind: "temporary"})
    }
    point(5, height, 0)
    userData()
    with userData(homeData) { point(6, 0, 0) }
    point(7, 0, 0)
  `);
  const home = {kind: 'home', settings: {capacity: 4}};
  assert.deepEqual(data.userData, [home, home, {kind: 'tree'}, home, null, {kind: 'world'}, home, null]);
  assert.deepEqual(data.positions.slice(0, 6), [0, 5, 0, 1, 5, 0]);
  assert.deepEqual(data.positions.slice(15, 18), [5, 2, 0]);
  assert.throws(() => compile('with userData(3) { point(0,0,0) }'), /record/);
  assert.throws(() => compile('with userData({}) { let local = 1 } point(local,0,0)'), /Unknown variable/);
  assert.throws(() => compile('with userData({}) point(0,0,0)'), /Expected '\{'/);
});

test('user data rejects invalid records and bounds accumulated allocations', () => {
  assert.throws(() => compile('userData({kind: "home", kind: "tree"})'), /Duplicate/);
  assert.throws(() => compile('userData({__proto__: 1})'), /field name/);
  assert.throws(() => compile('userData({mesh: meshBox(1, 1, 1)})'), /data/);
  assert.throws(() => compile('userData(3)'), /record/);
  assert.throws(() => compile(`userData({label: "${'x'.repeat(1024)}"}) repeat 3000 as i { point(i, 0, 0) }`), /User data allocation limit/);
});
