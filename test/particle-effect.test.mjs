import test from 'node:test';
import assert from 'node:assert/strict';
import * as engine from 'joy-engine';

function asset(overrides = {}) {
  return {
    version: 1,
    name: 'Test',
    maxParticles: 64,
    parameters: { speed: { value: 60, min: 0, max: 120 } },
    emitters: [
      {
        id: 'main',
        enabled: true,
        root: true,
        duration: 0,
        rate: 0,
        bursts: [{ time: 0, count: 2 }],
        lifetime: 1,
        renderer: 'soft',
        spawn: 'vx = speed; size = rand(2, 4);',
        update: 'alpha = 1 - t;',
        ...overrides,
      },
    ],
  };
}
function effect(document = asset(), options) {
  return new engine.ParticleEffect(engine.compileParticleEffect(document), options);
}

test('effect particles use seconds, seeded spawn and independent parameter overrides', () => {
  const a = effect(),
    b = effect();
  assert.deepEqual(a.particles, b.particles);
  assert.equal(a.particles.length, 2);
  a.step();
  assert.equal(a.particles[0].x, 1);
  assert.equal(b.particles[0].x, 0);
  const fast = effect(asset(), {
    parameters: { speed: 120 },
    position: { x: 12, y: 34, z: 5 },
  });
  fast.step();
  assert.equal(fast.particles[0].x, 14);
  assert.equal(fast.particles[0].z, 5);
  a.reset();
  assert.deepEqual(a.particles, b.particles);
});

test('numeric scripts support local values, conditions, persistent channels and bounded math', () => {
  const a = effect(
    asset({
      spawn: 'let angle = pi / 2; vx = cos(angle); vy = sin(angle); u0 = 2;',
      update: 'u0 += dt; if(t < 0.5) { r = 8; } else { r = 1; } size = mix(2, 8, t);',
    }),
  );
  a.step();
  assert.equal(a.particles[0].r, 8);
  assert.ok(a.particles[0].u0 > 2);
  a.seek(0.6);
  assert.equal(a.particles[0].r, 1);
  assert.ok(a.particles[0].size > 5);
});

test('asset validation rejects bad identifiers, scripts, unknown children and nonfinite inputs', () => {
  for(const spawn of [
    'x = globalThis.foo;',
    'while(1) {x=0;}',
    'age = 2;',
    'x = missing;',
    'x = constructor(1);',
  ]) {
    assert.throws(() => engine.compileParticleEffect(asset({ spawn })));
  }
  assert.throws(
    () => engine.compileParticleEffect(asset({ death: { emitter: 'missing', count: 1 } })),
    /missing/,
  );
  assert.throws(() => effect(asset(), { parameters: { speed: 999 } }));
  assert.throws(() => effect(asset(), { position: { x: NaN, y: 0 } }));
  assert.throws(() => effect(asset(), { seed: NaN }));
  assert.throws(() => engine.compileParticleEffect(asset({ bursts: [{ time: 1, count: 2 }] })));
});

test('trails and death children inherit the moving parent origin and remain finite', () => {
  const document = asset({
    bursts: [{ time: 0, count: 1 }],
    lifetime: 0.1,
    trail: { emitter: 'smoke', interval: 1 / 30, count: 1 },
    death: { emitter: 'smoke', count: 3 },
  });
  document.emitters.push({
    ...document.emitters[0],
    id: 'smoke',
    root: false,
    trail: undefined,
    death: undefined,
    lifetime: 0.2,
    spawn: 'vx = parentVx * 0.1;',
    update: '',
  });
  const a = effect(document);
  a.seek(0.1);
  const smoke = a.particles.filter((p) => p.emitterIndex === 1);
  assert.ok(smoke.length >= 5);
  assert.ok(smoke.every((p) => p.x > 0));
  a.seek(1);
  assert.equal(a.isAlive, false);
});

test('capacity and generation limits bound recursive child emission and disposal', () => {
  const document = asset({
    lifetime: 1 / 60,
    death: { emitter: 'main', count: 4096 },
  });
  document.maxParticles = 8;
  const a = effect(document);
  for(let i = 0; i < 20; i++) {
    a.step();
    assert.ok(a.particles.length <= 8);
  }
  assert.ok(a.droppedParticles > 0);
  assert.equal(a.isAlive, false);
  a.destroy();
  a.destroy();
  assert.equal(a.particles.length, 0);
  assert.throws(() => a.step(), /destroy/i);
});

test('stopping emission drains live particles and invalid scripts report source locations', () => {
  const a = effect(asset({ duration: 2, rate: 60, bursts: [], lifetime: 0.1 }));
  a.step();
  a.stop();
  for(let i = 0; i < 30; i++) {
    a.step();
  }
  assert.equal(a.isAlive, false);
  assert.throws(
    () => effect(asset({ spawn: 'x = 1 / 0;' })),
    (error) => error.line === 1 && error.column > 0,
  );
});

test('rendering appends finite HDR geometry without changing the simulation', () => {
  const a = effect(asset({ renderer: 'billow', spawn: 'r = 8; size = 12;' }));
  const before = JSON.stringify(a.particles),
    vertices = [];
  engine.appendParticleEffect(vertices, a, engine.PARTICLE_XY_BASIS);
  assert.ok(vertices.length > 100);
  assert.equal(vertices.length % 21, 0);
  assert.ok(vertices.every(Number.isFinite));
  assert.ok(vertices.some((v, i) => i % 7 === 3 && v > 1));
  assert.equal(JSON.stringify(a.particles), before);
});

test('parameter names never alias private runtime bookkeeping', () => {
  const document = asset({
    spawn: 'size = generation + emitterIndex + nextTrail;',
  });
  document.parameters = {
    generation: { value: 2, min: 0, max: 10 },
    emitterIndex: { value: 3, min: 0, max: 10 },
    nextTrail: { value: 4, min: 0, max: 10 },
  };
  assert.equal(effect(document).particles[0].size, 9);
});

test('different frame cadences and seek preserve live randomized trajectories', () => {
  const document = asset({
    duration: 2,
    rate: 37,
    bursts: [{ time: 0.1, count: 3 }],
    spawn: 'vx = rand(20, 80); vy = rand(-50, 50); u0 = rand();',
    update: 'ax = noise(age + u0);',
  });
  const a = effect(document),
    b = effect(document),
    c = effect(document);
  for(let i = 0; i < 48; i++) {
    a.update(1 / 120);
  }
  for(let i = 0; i < 12; i++) {
    b.update(1 / 30);
  }
  c.seek(0.4);
  assert.ok(a.particles.length > 10);
  assert.deepEqual(a.particles, b.particles);
  assert.deepEqual(a.particles, c.particles);
});

test('source, nesting and per-step instruction budgets reject excessive work', () => {
  assert.throws(() => engine.compileParticleEffect(asset({ spawn: ' '.repeat(12001) })), /12,000/);
  assert.throws(
    () =>
      engine.compileParticleEffect(
        asset({ spawn: 'x = ' + '('.repeat(70) + '1' + ')'.repeat(70) + ';' }),
      ),
    /nesting/,
  );
  const document = asset({
    bursts: [{ time: 0, count: 4096 }],
    spawn: 'x += sin(0);\n'.repeat(250),
  });
  document.maxParticles = 4096;
  assert.throws(() => effect(document), /instruction budget/);
});

test('game-owned random streams are borrowed and seeded defaults remain repeatable', () => {
  let calls = 0;
  const runtime = effect(asset(), { random: () => {
    calls++;
    return 0.25;
  } });
  assert.equal(calls, 2);
  assert.equal(runtime.particles[0].size, 2.5);
  runtime.reset();
  assert.equal(calls, 4, 'reset continues a borrowed stream');
});

test('owners can cull oldest particles and rescale positions without changing velocity', () => {
  const runtime = effect(asset(), { position: { x: 10, y: 20, z: 3 } });
  runtime.step();
  const newest = runtime.particles[1];
  runtime.removeOldestParticles(1);
  assert.equal(runtime.particles.length, 1);
  assert.equal(runtime.particles[0], newest);
  runtime.scalePositions({ x: 2, y: 0.5, z: 1 });
  assert.equal(newest.x, 22);
  assert.equal(newest.y, 10);
  assert.equal(newest.vx, 60);
  assert.throws(() => runtime.scalePositions({ x: Infinity, y: 1, z: 1 }));
  assert.equal(newest.x, 22, 'invalid scaling is atomic');
  assert.throws(() => runtime.removeOldestParticles(-1));
});

test('container scripts copy values, expose components and preserve seeded evaluation order', () => {
  const document = asset({ spawn: `
    let impulse = vector(rand(1, 2), rand(2, 3), rand(3, 4));
    velocity = impulse;
    impulse.x = 99;
    position = origin + vector(1, 2, 3);
    acceleration = vector(0, -10, 0);
    color = rgba(2, 0.4, 0.2, 0.8);
    color.a *= 0.5;
    u0 = impulse.x;
  `, update: 'velocity += acceleration * dt;' });
  const first = effect(document, { seed: 42, position: { x: 10, y: 20, z: 30 } });
  const second = effect(document, { seed: 42, position: { x: 10, y: 20, z: 30 } });
  assert.deepEqual(first.particles, second.particles);
  const particle = first.particles[0];
  assert.deepEqual([particle.x, particle.y, particle.z], [11, 22, 33]);
  assert.ok(particle.vx >= 1 && particle.vx < 2);
  assert.deepEqual([particle.r, particle.g, particle.b, particle.alpha], [2, 0.4, 0.2, 0.4]);
  assert.equal(particle.u0, 99);
  first.step();
  second.step();
  assert.deepEqual(first.particles, second.particles);
});

test('vector and color properties supply typed read-only inputs and detached overrides', () => {
  const document = asset({ spawn: 'velocity = direction; color = tint; size = tint.a + direction.z;', update: '' });
  document.parameters = {
    direction: { type: 'vector3', value: [1, 2, 3], min: [-10, -10, -10], max: [10, 10, 10] },
    tint: { type: 'color', value: [0.2, 0.4, 0.6, 1], min: [0, 0, 0, 0], max: [1, 1, 1, 1] },
  };
  const override = [0.8, 0.7, 0.6, 0.5];
  const runtime = effect(document, { parameters: { tint: override } });
  override[0] = 0;
  assert.deepEqual([runtime.particles[0].r, runtime.particles[0].alpha, runtime.particles[0].size], [0.8, 0.5, 3.5]);
  runtime.reset();
  assert.equal(runtime.particles[0].r, 0.8);
  assert.throws(() => effect(document, { parameters: { tint: [1, 0, 0, 2] } }), /Invalid value/);
  document.parameters.tint.value[0] = 1.1;
  assert.throws(() => engine.compileParticleEffect(document), /tint.value/);
});

test('container scripts reject shape mismatches, readonly writes and arbitrary property access', () => {
  for(const source of [
    'position = rgba(1, 1, 1, 1);', 'color = vector(1, 2, 3);', 'size = position;',
    'position = 1;', 'origin.x = 1;', 'parentVelocity = vector(1, 2, 3);',
    'position.w = 1;', 'color.x = 1;', 'position.constructor = 1;',
    'let v = vector(1, 2, 3); v = 2;', 'let v = vector(1, 2, 3); v.x = color;',
    'if (position) { size = 1; }', 'let v = rgba(1, 2, 3);',
  ]) {
    assert.throws(() => effect(asset({ spawn: source })), /Emitter 'main'/, source);
  }
});

test('container assignments evaluate the entire right side before writing and retain budgets', () => {
  const runtime = effect(asset({ spawn: 'position = vector(1, 2, 3); position = vector(position.z, position.x, position.y);', update: '' }));
  assert.deepEqual([runtime.particles[0].x, runtime.particles[0].y, runtime.particles[0].z], [3, 1, 2]);
  assert.throws(() => effect(asset({ spawn: 'velocity = vector(1 / 0, 0, 0);' })), /finite/);
});

test('container overrides reject sparse component arrays before simulation', () => {
  const document = asset({ spawn: '', update: '' });
  document.parameters.direction = { type: 'vector3', value: [1, 2, 3], min: [-10, -10, -10], max: [10, 10, 10] };
  document.parameters.tint = { type: 'color', value: [1, 1, 1, 1], min: [0, 0, 0, 0], max: [1, 1, 1, 1] };
  assert.throws(() => effect(document, { parameters: { direction: new Array(3) } }), /Invalid value/);
  assert.throws(() => effect(document, { parameters: { tint: [1, , 0, 1] } }), /Invalid value/);
});

test('named containers cannot shadow another property component alias', () => {
  const document = asset({ spawn: '', update: '' });
  document.parameters.direction = { type: 'vector3', value: [1, 2, 3], min: [-10, -10, -10], max: [10, 10, 10] };
  document.parameters.directionX = { type: 'color', value: [1, 1, 1, 1], min: [0, 0, 0, 0], max: [1, 1, 1, 1] };
  assert.throws(() => engine.compileParticleEffect(document), /unique|reserved/);
});

test('version-1 scalar names can shadow new built-in containers without changing saved scripts', () => {
  for(const name of ['position', 'velocity', 'acceleration', 'color', 'origin', 'parentVelocity', 'vector', 'rgba']) {
    const parameterDocument = asset({ spawn: `vx = ${name};`, update: '' });
    parameterDocument.parameters[name] = { value: 3, min: 0, max: 4 };
    assert.equal(effect(parameterDocument).particles[0].vx, 3, `parameter ${name}`);
    const localDocument = asset({ spawn: `let ${name} = 3; vx = ${name};`, update: '' });
    assert.equal(effect(localDocument).particles[0].vx, 3, `local ${name}`);
  }
});
