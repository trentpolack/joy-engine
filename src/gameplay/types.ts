// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { Vector3 as EntityVector3 } from '../core/types.ts';

export type JsonValue = null | boolean | number | string | JsonValue[] | {
    [key: string]: JsonValue;
};
export type JsonRecord = Record<string, JsonValue>;
export type { EntityVector3 };
export interface EntityTransform {
  position: EntityVector3;
  rotation: EntityVector3;
  scale: EntityVector3;
}
export interface EntityDefinition {
  id: string;
  name: string;
  type: string;
  tags: string[];
  transform: EntityTransform;
  components: Record<string, JsonRecord>;
  template?: string;
  overrides?: JsonRecord;
}
export interface EntityOptions {
  id: string;
  name?: string;
  type?: string;
  tags?: readonly string[];
  transform?: {
      position?: readonly number[];
      rotation?: readonly number[];
      scale?: readonly number[];
  };
  components?: Record<string, JsonRecord>;
  template?: string;
  overrides?: JsonRecord;
}
export type ControlIntent = Record<string, number | boolean>;
export {};
