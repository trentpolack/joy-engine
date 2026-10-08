// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

export {Entity} from './entity.ts';
export {EntityComponent} from './component.ts';
export {ComponentRegistry} from './component-registry.ts';
export type {ComponentFactory} from './component-registry.ts';
export {EntitySystem} from './entity-system.ts';
export type {EntitySystemOptions} from './entity-system.ts';
export {Character, PlayerCharacter} from './character.ts';
export {Controller} from './controller.ts';
export {PlayerController, AIController} from './player-controller.ts';
export {EntityRegistry} from './entity-registry.ts';
export {EntityWorld} from './entity-world.ts';
export type {EntityWorldOptions} from './entity-world.ts';
export {copyEntityDefinition} from './entity-definition.ts';

export { assetPath } from './asset-path.ts';
export { parseObject, serializeObject, createObjectInstance, resolveObjectInstance, resolveLevelObjects } from './object-document.ts';
export { AssetScene } from './asset-scene.ts';
