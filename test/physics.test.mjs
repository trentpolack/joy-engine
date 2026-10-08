import assert from 'node:assert/strict';
import test from 'node:test';
import {PhysicsBody, PhysicsVehicle, PhysicsWorld} from '../src/physics/index.ts';
import {meshTransform, transformPoint} from '../src/rendering/scene/mesh-transform.ts';

function simulate(world, seconds) {
  for(let frame = 0; frame < seconds*60; frame+= 1) {
    world.step(1/60);
  }
}

function makeVehicle(world, extraWheel = false) {
  const chassis = world.createBody({shape: 'box', mass: 180, halfExtents: [0.8, 0.25, 1.25], position: [0, 1, 0]});
  const wheels = [];
  for(const z of [1, -1]) {
    for(const x of [-0.8, 0.8]) {
      wheels.push({position: [x, 0, z], radius: 0.35, steering: z > 0, driven: true, brake: true});
    }
  }
  if(extraWheel) {
    wheels.push({position: [0, 0, 0], radius: 0.35, driven: true});
  }
  return {chassis, vehicle: world.createVehicle(chassis, {wheels, engineForce: 400, maxSteer: 0.4, brakeForce: 20})};
}

test('rigid bodies collide with a Y-up floor; transforms, impulses and ray queries are copied', () => {
  const world = new PhysicsWorld();
  const floor = world.createBody({shape: 'plane', friction: 0.6});
  const box = world.createBody({shape: 'box', mass: 2, halfExtents: [0.5, 0.5, 0.5], position: [0, 4, 0]});
  const sphere = world.createBody({shape: 'sphere', mass: 1, radius: 0.5, position: [2, 3, 0]});
  simulate(world, 3);
  assert.ok(Math.abs(box.getTransform().position[1] - 0.5) < 0.05);
  assert.ok(Math.abs(sphere.getTransform().position[1] - 0.5) < 0.05);
  const hit = world.raycast([0, 5, 0], [0, -1, 0]);
  assert.equal(hit.body, box);
  assert.ok(hit.normal[1] > 0.9);
  assert.equal(world.raycast([20, 2, 0], [20, 1, 0]), null);
  box.applyImpulse([2, 0, 0]);
  assert.ok(box.getVelocity()[0] > 0.9);
  const transform = box.getTransform();
  transform.position[0] = 500;
  assert.ok(box.getTransform().position[0] < 1);
  box.setTransform({position: [3, 2, 1], rotation: [0.3, 0.2, 0.1]});
  const restored = box.getTransform();
  restored.rotation.forEach((value, index) => assert.ok(Math.abs(value - [0.3, 0.2, 0.1][index]) < 1e-6));
  floor.destroy();
  world.destroy();
});

test('fixed stepping bounds catch-up and rejects invalid simulation and body configuration', () => {
  const world = new PhysicsWorld({gravity: [0, 0, 0], fixedStep: 1/60, maxSubSteps: 3});
  const body = world.createBody({shape: 'sphere', radius: 1, mass: 1});
  body.setVelocity([0, 0, 1]);
  assert.equal(world.step(1/120), 0);
  assert.equal(world.step(1/120), 1);
  assert.equal(world.step(100000), 3);
  assert.equal(world.step(0), 0);
  assert.ok(body.getTransform().position[2] < 0.07);
  for(const value of [NaN, Infinity, -1]) {
    assert.throws(() => world.step(value));
    assert.throws(() => world.createBody({shape: 'sphere', mass: value}));
  }
  assert.throws(() => world.createBody({shape: 'box', halfExtents: [0, 1, 1]}));
  assert.throws(() => new PhysicsWorld({fixedStep: 0}));
  assert.throws(() => new PhysicsWorld({maxSubSteps: 1000000}));
  assert.throws(() => body.setTransform({position: [0, Infinity, 0]}));
  assert.throws(() => body.applyForce([0, NaN, 0]));
  world.destroy();
});

test('world ownership is independent and body/vehicle cleanup is idempotent', () => {
  const first = new PhysicsWorld();
  const second = new PhysicsWorld();
  const {chassis, vehicle} = makeVehicle(first, true);
  const otherBody = second.createBody({shape: 'sphere', mass: 1});
  assert.throws(() => second.createVehicle(chassis, {wheels: []}));
  assert.throws(() => first.createVehicle(chassis, {wheels: []}));
  assert.equal(vehicle.getWheelTransforms().length, 5);
  vehicle.destroy();
  vehicle.destroy();
  chassis.applyImpulse([1, 0, 0]);
  assert.ok(first.raycast([0, 5, 0], [0, 0, 0]));
  for(const invalidConfig of [
    {wheels: [{position: [0, 0, 0], radius: NaN}]},
    {wheels: [{position: [0, 0, 0], suspensionStiffness: Infinity}]},
    {wheels: [{position: [0, 0, 0], driven: 1}]},
    {wheels: [{position: [0, 0, 0]}], engineForce: Infinity}
  ]) {
    assert.throws(() => first.createVehicle(chassis, invalidConfig));
    assert.equal(first.vehicles.size, 0);
    assert.equal(chassis.vehicle, null);
  }
  const replacement = first.createVehicle(chassis, {wheels: [{position: [0, 0, 0], radius: 0.3}]});
  chassis.destroy();
  assert.throws(() => replacement.setInput({throttle: 1}));
  assert.equal(first.raycast([0, 5, 0], [0, 0, 0]), null);
  first.destroy();
  first.destroy();
  assert.throws(() => first.step(1/60));
  simulate(second, 0.25);
  assert.ok(otherBody.getTransform().position[1] < 0);
  second.destroy();
});

test('raycast wheels provide real traction, steering and braking with per-wheel flags', () => {
  const straightWorld = new PhysicsWorld();
  straightWorld.createBody({shape: 'plane'});
  const straight = makeVehicle(straightWorld);
  simulate(straightWorld, 1);
  straight.vehicle.setInput({throttle: 1});
  simulate(straightWorld, 2);
  assert.ok(straight.chassis.getTransform().position[2] > 2);
  assert.ok(straight.vehicle.getGroundedWheelCount() >= 2);
  const unbrakedSpeed = straight.vehicle.getSpeed();
  straight.vehicle.setInput({brake: 1});
  simulate(straightWorld, 1);
  assert.ok(straight.vehicle.getSpeed() < unbrakedSpeed*0.5);

  const turningWorld = new PhysicsWorld();
  turningWorld.createBody({shape: 'plane'});
  const turning = makeVehicle(turningWorld);
  simulate(turningWorld, 1);
  turning.vehicle.setInput({throttle: 1, steer: 0.5});
  simulate(turningWorld, 2);
  assert.ok(turning.chassis.getTransform().position[0] > 0.5);
  assert.ok(Math.abs(turning.chassis.getTransform().rotation[1]) > 0.1);
  assert.throws(() => turning.vehicle.setInput({throttle: NaN}));
  const passiveWorld = new PhysicsWorld();
  passiveWorld.createBody({shape: 'plane'});
  const passiveBody = passiveWorld.createBody({shape: 'box', mass: 180, halfExtents: [0.8, 0.25, 1.25], position: [0, 1, 0]});
  const passive = passiveWorld.createVehicle(passiveBody, {wheels: [-1, 1].flatMap((z) => [-0.8, 0.8].map((x) => ({position: [x, 0, z]})))});
  simulate(passiveWorld, 1);
  passive.setInput({throttle: 1, steer: 1});
  simulate(passiveWorld, 1);
  assert.ok(Math.abs(passiveBody.getTransform().position[2]) < 0.01);
  passiveWorld.destroy();
  turningWorld.destroy();
  straightWorld.destroy();
});


test('sparse vectors fail before world, body, or vehicle state is changed', () => {
  assert.throws(() => new PhysicsWorld({gravity: Array(3)}), /finite/);
  const world = new PhysicsWorld();
  const body = world.createBody({shape: 'sphere', mass: 1});
  for(const sparse of [Array(3), [1, , 2], [, 1, 2], [1, 2, ,]]) {
    assert.throws(() => world.createBody({shape: 'sphere', position: sparse}), /finite/);
    assert.throws(() => body.setTransform({position: sparse}), /finite/);
    assert.throws(() => body.applyImpulse(sparse), /finite/);
    assert.throws(() => world.createVehicle(body, {wheels: [{position: sparse}]}), /finite/);
    assert.equal(world.bodies.size, 1);
    assert.equal(world.vehicles.size, 0);
    assert.equal(body.vehicle, null);
  }
  assert.deepEqual(body.getTransform().position, [0, 0, 0]);
  assert.deepEqual(body.getVelocity(), [0, 0, 0]);
  world.destroy();
});

test('direct construction cannot escape world ownership or leak solver callbacks', () => {
  const owner = new PhysicsWorld();
  const foreign = new PhysicsWorld();
  const chassis = owner.createBody({shape: 'box', mass: 180});
  const config = {wheels: [{position: [0, 0, 0]}]};
  for(const world of [owner, foreign]) {
    assert.throws(() => new PhysicsBody(world, {shape: 'sphere', mass: 1}), /createBody/);
    assert.throws(() => new PhysicsVehicle(world, chassis, config), /createVehicle/);
    assert.equal(world.vehicles.size, 0);
    assert.equal(world.backend.hasAnyEventListener('preStep'), false);
    assert.equal(chassis.vehicle, null);
  }
  assert.equal(owner.bodies.size, 1);
  assert.equal(owner.backend.bodies.length, 1);
  assert.equal(foreign.bodies.size, 0);
  assert.equal(foreign.backend.bodies.length, 0);
  const vehicle = owner.createVehicle(chassis, config);
  assert.equal(owner.backend.hasAnyEventListener('preStep'), true);
  foreign.destroy();
  owner.destroy();
  assert.equal(vehicle.disposed, true);
  assert.equal(chassis.disposed, true);
  assert.equal(owner.backend.bodies.length, 0);
  assert.equal(owner.backend.hasEventListener('preStep', vehicle.backend.preStepCallback), false);
});


test('multi-axis colliders and wheel rotations match rendered X-then-Y-then-Z transforms', () => {
  const world = new PhysicsWorld();
  const rotations = [
    [Math.PI/4, Math.PI/4, 0],
    [0.8, -0.7, 2.4],
    [-1.2, 2.2, -2.8],
    [0.6, Math.PI/2, -0.3],
    [-0.9, -Math.PI/2, 1.1],
    [0.3, Math.PI/2 - 1e-5, -0.2]
  ];
  const plane = world.createBody({shape: 'plane', rotation: rotations[0]});
  const chassis = world.createBody({shape: 'box', mass: 180, position: [100, 0, 0]});
  const vehicle = world.createVehicle(chassis, {wheels: [{position: [0, 0, 0]}]});
  for(const [index, rotation] of rotations.entries()) {
    if(index > 0) {
      plane.setTransform({rotation});
    }
    chassis.setTransform({rotation});
    const visual = meshTransform({position: [0, 0, 0], rotation, scale: [1, 1, 1]});
    const normal = transformPoint(visual, 0, 1, 0);
    const hit = world.raycast(normal.map((value) => value*3), normal.map((value) => -value*3));
    assert.equal(hit.body, plane);
    normal.forEach((value, axis) => assert.ok(Math.abs(hit.normal[axis] - value) < 1e-7, `Collider normal must match rendered normal for ${rotation}`));
    for(const result of [plane.getTransform(), vehicle.getWheelTransforms()[0]]) {
      const roundTrip = meshTransform({position: [0, 0, 0], rotation: result.rotation, scale: [1, 1, 1]});
      for(let component = 0; component < 9; component+= 1) {
        assert.ok(Math.abs(roundTrip[component] - visual[component]) < 1e-7, `Returned orientation must match renderer for ${rotation}`);
      }
    }
  }
  world.destroy();
});
