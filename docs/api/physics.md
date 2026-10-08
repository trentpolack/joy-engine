# Physics API
[API index](README.md) · [Engine guide](../README.md)

Generated from public exports, TypeScript declarations and source documentation with `npm run docs:api`. Implementations remain the source of truth; private methods are omitted.

## PhysicsWorld
Import from `joy-engine`. [Source](../../src/physics/physics-world.ts)

Owns a solver and every body/vehicle created through it. Independent instances share no gameplay state. Uses discrete collisions: thin surfaces and very fast bodies can tunnel; raycast tires have no solid wheel collider.

### constructor
```ts
constructor(options: PhysicsWorldOptions = {})
```
Create an independent Y-up solver with bounded fixed-step work.

- **param** [options] Gravity is meters/second²; fixedStep is seconds.

### createBody
```ts
createBody(options: PhysicsBodyOptions)
```
Create a world-owned rigid body; invalid configuration leaves the world unchanged.

- **param** options

### createVehicle
```ts
createVehicle(body: PhysicsBody, config: PhysicsVehicleOptions)
```
Attach one vehicle to an owned dynamic chassis. The vehicle borrows its chassis.

- **param** body
- **param** config

### step
```ts
step(elapsedSeconds: number)
```
Advance by elapsed seconds; returns executed fixed steps. Excess catch-up time is discarded.

- **param** elapsedSeconds

### raycast
```ts
raycast(from: readonly number[], to: readonly number[])
```
Closest hit along a world-space segment. Result vectors are copied; body is borrowed.

- **param** from
- **param** to

### destroy
```ts
destroy()
```
Release vehicle callbacks before bodies; repeated destruction is harmless.


### assertFactoryCapability
```ts
assertFactoryCapability(capability: symbol | undefined, factory: 'createBody' | 'createVehicle')
```
- **internal** Reject direct adapter construction before resources can be allocated.
- **param** capability
- **param** factory


## PhysicsBody
Import from `joy-engine`. [Source](../../src/physics/physics-body.ts)

World-owned rigid body. Construction requires PhysicsWorld.createBody; mass 0 is static. World coordinates are meters, Y up, +Z forward; XYZ Euler radians apply X, then Y, then Z.

### constructor
```ts
constructor(world: PhysicsWorld, options: PhysicsBodyOptions, capability?: symbol)
```
- **internal** Factory-only constructor; direct calls throw before allocating a solver body.
- **param** world
- **param** options
- **param** [capability]

### getTransform
```ts
getTransform(): {
    position: [
        number,
        number,
        number
    ];
    rotation: [
        number,
        number,
        number
    ];
}
```
Copied transform; mutating the result never changes the simulation.


### setTransform
```ts
setTransform(transform: PhysicsTransform)
```
Teleport in world space without changing velocity.

- **param** transform

### getVelocity
```ts
getVelocity()
```
Copied world-space velocity in meters/second.


### setVelocity
```ts
setVelocity(velocity: readonly number[])
```
Set world-space velocity in meters/second.

- **param** velocity

### applyForce
```ts
applyForce(force: readonly number[], point?: readonly number[])
```
Force in newtons until the next fixed step; optional application point is world space.

- **param** force
- **param** [point]

### applyImpulse
```ts
applyImpulse(impulse: readonly number[], point?: readonly number[])
```
Instantaneous impulse in newton-seconds; optional application point is world space.

- **param** impulse
- **param** [point]

### destroy
```ts
destroy()
```
Detach dependent vehicle, remove this body, and release ownership. Idempotent.


### assertActive
```ts
assertActive()
```
Reject solver access after either the body or its owner is disposed.

- **internal**


## PhysicsVehicle
Import from `joy-engine`. [Source](../../src/physics/physics-vehicle.ts)

Raycast suspension for a data-defined wheel array; tire contacts are rays, not rolling rigid bodies. Construct through PhysicsWorld.createVehicle. World owns this adapter; chassis is borrowed and survives adapter destruction.

### constructor
```ts
constructor(world: PhysicsWorld, chassis: PhysicsBody, config: PhysicsVehicleOptions, capability?: symbol)
```
- **internal** Factory-only constructor; direct calls throw before attaching solver callbacks.
- **param** world
- **param** chassis
- **param** config
- **param** [capability]

### setInput
```ts
setInput(input: PhysicsVehicleInput)
```
Replace intent; omitted fields reset to zero. Throttle/steer clamp to [-1,1], brake to [0,1]. Positive throttle drives local +Z; positive steer turns toward local +X.

- **param** input

### getWheelTransforms
```ts
getWheelTransforms()
```
Copied wheel transforms in world meters and XYZ Euler radians; radius remains configuration data.


### getSpeed
```ts
getSpeed()
```
Unsigned chassis speed in meters/second.


### getGroundedWheelCount
```ts
getGroundedWheelCount()
```
Tire contacts recorded during the most recent fixed step.


### destroy
```ts
destroy()
```
Remove the solver's pre-step callback while retaining the borrowed chassis. Idempotent.
