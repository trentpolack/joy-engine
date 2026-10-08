import test from 'node:test';
import assert from 'node:assert/strict';
import { readDocument } from '../src/document.ts';
import { compile, compileFormScript } from 'joy-engine/form';
import { RECIPES } from '../src/script-guide.ts';
import { EXAMPLES } from '../src/examples.ts';
import { defaultMaterial } from 'joy-engine/form';

test('documents retain source, name, and numeric overrides', () => {
  const doc = {
    version: 1,
    name: 'Study',
    source: 'point(0,0,0)',
    overrides: { size: 2 },
  };
  assert.deepEqual(readDocument(JSON.stringify(doc)), doc);
});

test('documents round-trip embedded textures beyond the former 1 MB limit and enforce the document budget', () => {
  const material = defaultMaterial();
  material.textures.baseColor = `data:image/png;base64,${Buffer.alloc(800_000).toString('base64')}`;
  const doc = { version: 1, name: 'Textured', source: 'point(0,0,0)', overrides: {}, materials: [material] };
  const serialized = JSON.stringify(doc);
  assert.ok(Buffer.byteLength(serialized) > 1_000_000);
  assert.deepEqual(readDocument(serialized), doc);
  assert.throws(() => readDocument(' '.repeat(64_000_001)), /64 MB/);
});
test('rejects unsupported and malformed saved documents', () => {
  for(const value of [
    null,
    {},
    { version: 2, name: 'x', source: '' },
    { version: 1, name: 'x', source: '', overrides: { x: '2' } },
  ]) {
    assert.throws(() => readDocument(JSON.stringify(value)));
  }
});
test('all shipped examples build at default and maximum parameter settings', () => {
  for(const recipe of RECIPES) {
    assert.ok(compile(recipe.code.replaceAll('$id', 'study')).positions.length > 0, recipe.title);
  }
  for(const example of EXAMPLES) {
    const initial = compile(example.source);
    const overrides = Object.fromEntries(
      initial.parameters.filter(parameter => 'max' in parameter).map((parameter) => [
        parameter.name,
        Array.isArray(parameter.value)
          ? parameter.value.map(() => parameter.max)
          : parameter.max,
      ]),
    );
    const maximum = compile(example.source, overrides);
    assert.ok(maximum.positions.length > 0, example.name);
    assert.ok(initial.userData?.some(value => value !== null), `${example.name} should expose user data`);
  }
});

test('documents round-trip vector and color overrides and reject malformed components', () => {
  const doc = {
    version: 1,
    name: 'Typed study',
    source: '',
    overrides: { offset: [1, 2, 3], tint: [1, 0.5, 0, 0.25] },
  };
  assert.deepEqual(readDocument(JSON.stringify(doc)), doc);
  for(const value of [
    [1, 2],
    [1, 2, 3, 4, 5],
    [1, 2, null],
    [1, 2, '3'],
  ]) {
    assert.throws(() =>
      readDocument(JSON.stringify({ ...doc, overrides: { offset: value } })),
    );
  }
});

test('project name edits preserve metadata formatting and unrelated script text', async () => {
  const { projectNameChange } = await import('../src/project-metadata.ts');
  const source =
    '# name="untouched"\nproject meta(info="A", name="Old", title="Keep me")\npoint(0,0,0)';
  const change = projectNameChange(source, 'New "name"');
  const renamed =
    source.slice(0, change.from) + change.insert + source.slice(change.to);
  assert.equal(
    renamed,
    '# name="untouched"\nproject meta(info="A", name="New \\"name\\"", title="Keep me")\npoint(0,0,0)',
  );
  assert.equal(compile(renamed).project.name, 'New "name"');
  const inserted = projectNameChange('point(0,0,0)', 'A');
  assert.equal(compile(inserted.insert + 'point(0,0,0)').project.name, 'A');
  assert.throws(() => projectNameChange('point(', 'A'));
});


test('named variants round-trip owned parameter values and reject ambiguous or oversized collections', () => {
  const doc = {version: 1, name: 'Variants', source: '', overrides: {}, variants: [
    {name: 'Tall', overrides: {height: 4, tint: [1, 0.5, 0, 1]}},
    {name: 'Script defaults', overrides: {}}
  ]};
  const restored = readDocument(JSON.stringify(doc));
  assert.deepEqual(restored, doc);
  restored.variants[0].overrides.tint[0] = 0;
  assert.equal(doc.variants[0].overrides.tint[0], 1);
  for(const variants of [null, {}, [{name: '', overrides: {}}], [doc.variants[0], doc.variants[0]],
    [{name: 'Bad', overrides: {height: '4'}}], Array.from({length: 17}, (_, i) => ({name: String(i), overrides: {}}))]) {
    assert.throws(() => readDocument(JSON.stringify({...doc, variants})), /variant|parameter/i);
  }
});

test('project serialization enforces the reopen budget across all variant resources', async () => {
  const { serializeDocument } = await import('../src/document.ts');
  const material = defaultMaterial();
  material.textures.baseColor = `data:image/png;base64,${Buffer.alloc(3_000_000).toString('base64')}`;
  const overrides = {finish: {kind: 'material', material}};
  const doc = {version: 1, name: 'Heavy', source: '', overrides, variants: Array.from({length: 16}, (_, i) => ({name: `Take ${i}`, overrides}))};
  assert.throws(() => serializeDocument(doc), /64 MB/);
  const small = {...doc, variants: doc.variants.slice(0, 1)};
  assert.deepEqual(readDocument(serializeDocument(small)), small);
});

test('gameplay examples expose independent entity records and parameter-driven generation', () => {
  for(const scenario of [
    {name: 'Harvest grove', output: 'Resource nodes', overrides: {rows: 2, columns: 3}, count: 6, kind: 'resource-node', attribute: 'remaining'},
    {name: 'Patrol circuit', output: 'Waypoints', overrides: {count: 5}, count: 5, kind: 'waypoint', attribute: 'speed'},
    {name: 'Tactical terraces', output: 'Tiles', overrides: {rows: 3, columns: 4}, count: 12, kind: 'tile', attribute: 'moveCost'}
  ]) {
    const example = EXAMPLES.find(value => value.name === scenario.name);
    assert.ok(example, scenario.name);
    const program = compileFormScript(example.source);
    const result = program.evaluate(scenario.overrides);
    const logical = result.outputs.find(output => output.name === scenario.output).value;
    assert.equal(logical.attributes.points.position.values.length, scenario.count);
    assert.equal(logical.userData.length, scenario.count);
    assert.deepEqual(logical.userData.map(record => record.id), Array.from({length: scenario.count}, (_, index) => index));
    assert.ok(logical.userData.every(record => record.kind === scenario.kind));
    assert.equal(logical.attributes.points[scenario.attribute].values.length, scenario.count);
    assert.deepEqual(program.evaluate(scenario.overrides), result);
    if(scenario.kind === 'resource-node') {
      const depleted = program.evaluate({...scenario.overrides, harvestedFraction: 1});
      const nodes = depleted.outputs.find(output => output.name === scenario.output).value;
      assert.ok(nodes.attributes.points.remaining.values.every(value => value === 0));
      assert.deepEqual(nodes.userData, logical.userData);
    }
    if(scenario.kind === 'waypoint') {
      assert.deepEqual(logical.userData.map(record => record.nextId), [1,2,3,4,0]);
    }
    if(scenario.kind === 'tile') {
      assert.deepEqual(logical.userData[11].grid, {x: 3, z: 2});
      const dry = program.evaluate({...scenario.overrides, waterLevel: 0});
      const dryTiles = dry.outputs.find(output => output.name === scenario.output).value;
      assert.ok(dryTiles.attributes.points.walkable.values.every(value => value === 1));
      assert.ok(logical.attributes.points.walkable.values.some(value => value === 0));
    }
    const baseline = structuredClone(result);
    logical.userData[0].kind = 'changed';
    logical.attributes.points[scenario.attribute].values[0] = -1;
    assert.deepEqual(program.evaluate(scenario.overrides), baseline);
  }
});
