// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import {MeshAsset, PhysicsWorld, createChunkMeshData} from 'joy-engine';
import {PRIMITIVE_SHAPE} from 'joy-engine/constants';
import type {DestructibleChunkDefinition, MeshInstance, PhysicsBody, RenderScene} from 'joy-engine';
import {DESTRUCTION_CONFIG, GROUND_CONFIG} from './config/destruction-config.ts';

const GROUND_ID = 'ground';
const GROUND_MATERIAL = 'frame';
type DebrisPiece = {body: PhysicsBody; mesh: MeshInstance};

/**
 * Owns the example's ground and persistent rigid debris; borrows a live render scene.
 * One body per released chunk ID bounds this example to its finite structure; reset releases all debris.
 * Physics uses meters, seconds, and world XYZ with Y up. Dispose before destroying the scene.
 */
export class DestructionDebris {
  private scene: RenderScene;
  private world = new PhysicsWorld({
    gravity: [0, -DESTRUCTION_CONFIG.debrisGravity, 0],
    fixedStep: DESTRUCTION_CONFIG.physicsStepSeconds,
    maxSubSteps: DESTRUCTION_CONFIG.maximumPhysicsSubsteps
  });
  private groundMesh: MeshInstance;
  private pieces = new Map<string, DebrisPiece>();
  private staticChunks = new Map<string, PhysicsBody>();
  private disposed = false;

  constructor(scene: RenderScene) {
    this.scene = scene;
    this.world.createBody({
      shape: PRIMITIVE_SHAPE.BOX,
      position: GROUND_CONFIG.center,
      halfExtents: GROUND_CONFIG.size.map(size => size/2),
      friction: GROUND_CONFIG.friction,
      restitution: GROUND_CONFIG.restitution
    });
    const geometry = createChunkMeshData([{
      id: GROUND_ID, material: GROUND_MATERIAL, hitPoints: 1,
      center: [...GROUND_CONFIG.center], size: [...GROUND_CONFIG.size]
    }]);
    // Ground uses its own neutral color, independent of the destruction material palette.
    this.groundMesh = scene.createMesh(new MeshAsset({positions: geometry.positions, indices: geometry.indices}), {
      tint: GROUND_CONFIG.tint,
      sceneRoughness: 1
    });
  }

  /**
   * Copy removed chunks into local mesh assets and matching world-owned box bodies.
   * Debris is cosmetic and does not re-enter the destructible structure's query state.
   * Directly fractured chunks lose a configured outer margin; support failures retain size and begin at rest.
   */
  spawn(chunks: readonly DestructibleChunkDefinition[], applyImpactImpulse = true) {
    for(const chunk of chunks) {
      if(this.pieces.has(chunk.id)) {
        continue;
      }
      // A directly fractured chunk loses its thin outer shell, opening room to leave a tightly fitted wall.
      // Keep its rendered dimensions and collider identical; support-detached healthy chunks keep full size.
      const scale = applyImpactImpulse ? DESTRUCTION_CONFIG.fracturedChunkScale : 1;
      const size = chunk.size.map(value => value*scale) as [number, number, number];
      const body = this.world.createBody({
        shape: PRIMITIVE_SHAPE.BOX,
        mass: DESTRUCTION_CONFIG.debrisMass,
        position: chunk.center,
        halfExtents: size.map(value => value/2),
        friction: DESTRUCTION_CONFIG.debrisFriction,
        restitution: DESTRUCTION_CONFIG.debrisRestitution,
        linearDamping: DESTRUCTION_CONFIG.debrisLinearDamping,
        angularDamping: DESTRUCTION_CONFIG.debrisAngularDamping
      });
      const direction = Math.sign(chunk.center[0]) || 1;
      const launch = DESTRUCTION_CONFIG.debrisLaunchSpeed;
      const variation = DESTRUCTION_CONFIG.debrisLaunchVariation;
      if(applyImpactImpulse) {
        body.setVelocity([
          direction*(launch[0] + (this.pieces.size%3)*variation[0]),
          launch[1] + (this.pieces.size%4)*variation[1],
          launch[2]
        ]);
        // Support failures start at rest and fall; only directly broken chunks receive an impact impulse.
        body.applyImpulse(DESTRUCTION_CONFIG.debrisSpinImpulse, [
          chunk.center[0], chunk.center[1] + chunk.size[1]/2, chunk.center[2]
        ]);
      }
      const asset = new MeshAsset(createChunkMeshData([{...chunk, size, center: [0, 0, 0]}]));
      const mesh = this.scene.createMesh(asset, {position: chunk.center, sceneRoughness: 0.7});
      this.pieces.set(chunk.id, {body, mesh});
    }
  }

  /**
   * Match static colliders to committed structure before releasing chunks into rigid-body simulation.
   * Remaining walls catch falling debris; removed static bodies cannot overlap their new dynamic bodies.
   */
  synchronizeStructure(chunks: readonly DestructibleChunkDefinition[]) {
    const activeIds = new Set(chunks.map(chunk => chunk.id));
    for(const [id, body] of this.staticChunks) {
      if(!activeIds.has(id)) {
        body.destroy();
        this.staticChunks.delete(id);
      }
    }
    for(const chunk of chunks) {
      if(!this.staticChunks.has(chunk.id)) {
        this.staticChunks.set(chunk.id, this.world.createBody({
          shape: PRIMITIVE_SHAPE.BOX,
          position: chunk.center,
          halfExtents: chunk.size.map(size => size/2),
          friction: DESTRUCTION_CONFIG.debrisFriction,
          restitution: DESTRUCTION_CONFIG.debrisRestitution
        }));
      }
    }
  }

  /**
   * Advance bounded fixed-step physics by elapsed seconds and copy transforms into the scene.
   */
  update(elapsedSeconds: number) {
    this.world.step(elapsedSeconds);
    for(const piece of this.pieces.values()) {
      const transform = piece.body.getTransform();
      piece.mesh.position = transform.position;
      piece.mesh.rotation = transform.rotation;
    }
  }

  /**
   * Copied diagnostics for inspecting body motion; positions use meters and velocities meters/second.
   */
  snapshot() {
    return {
      groundTop: GROUND_CONFIG.center[1] + GROUND_CONFIG.size[1]/2,
      staticChunks: this.staticChunks.size,
      pieces: [...this.pieces].map(([chunkId, piece]) => ({
        chunkId,
        position: piece.body.getTransform().position,
        velocity: piece.body.getVelocity()
      }))
    };
  }

  get count() {
    return this.pieces.size;
  }

  /**
   * Release every dynamic body and mesh, retaining the shared ground for the next reset.
   */
  reset() {
    for(const piece of this.pieces.values()) {
      piece.body.destroy();
      this.scene.remove(piece.mesh);
    }
    this.pieces.clear();
  }

  /**
   * Release the ground, debris, and physics world. Safe to repeat.
   */
  dispose() {
    if(this.disposed) {
      return;
    }
    this.disposed = true;
    this.reset();
    this.world.destroy();
    this.staticChunks.clear();
    this.scene.remove(this.groundMesh);
  }
}
