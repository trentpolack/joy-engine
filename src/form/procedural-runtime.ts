// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { FORM_ATTRIBUTE, FORM_ATTRIBUTE_DOMAIN, FORM_COMMAND, FORM_VALUE_KIND } from './constants.ts';
import type { GeometryValue, Domain, ProceduralValue } from './procedural-geometry.ts';
import type { ParameterValue } from './parameters.ts';
import type { DataExpression } from './user-data.ts';
import type { NamedOutput } from './procedural-output.ts';
import { GeometryBuilder } from './geometry.ts';
import { GeometryStore, count, attributeAt, removeFace, removePoint } from './procedural-geometry.ts';
import { revolveGeometry, tubeGeometry, writeSmoothNormals } from './procedural-shapes.ts';
import { scatterGeometry } from './procedural-scatter.ts';
import { emitProcedural } from './procedural-output.ts';
import { dataRecord } from './user-data.ts';
import { scalar } from './parameters.ts';
import { meshBox, meshSphere } from './resources.ts';
export type ScriptValue = ParameterValue | DataExpression | string | ProceduralValue | {
    kind: typeof FORM_VALUE_KIND.ELEMENT;
    index: number;
};
export type { GeometryValue };
export type { Domain };

/** Procedural operations owned by one interpreter evaluation; no host state retained. */
export class ProceduralRuntime {
  declare store: GeometryStore;
  declare output: GeometryBuilder;
  declare bindMaterial: (value: ScriptValue) => number;
  declare spend: (amount: number) => void;
  declare current: { geometry: GeometryValue; domain: Domain; index: number; } | null;
  declare positionsChanged: boolean;
  declare normalsWritten: Set<number>;
  declare outputs: NamedOutput[];
  declare previewVertices: number;

  /** @param output @param bindMaterial @param spend */
  constructor(output: GeometryBuilder, bindMaterial: (value: ScriptValue) => number, spend: (amount: number) => void) {
    this.store = new GeometryStore(output.userDataStore);
    this.output = output;
    this.bindMaterial = bindMaterial;
    this.spend = spend;

    this.current = null;
    this.positionsChanged = false;
    this.normalsWritten = new Set();

    this.outputs = [];
    this.previewVertices = 0;
  }
  /** Dispatch procedural commands; undefined lets the interpreter try other command families.
   * @param name @param values */
  call(name: string, values: ScriptValue[]): ScriptValue | undefined {
    switch(name) {
      case FORM_COMMAND.BOX:
        return values.length === 3
          ? this.store.fromMesh(meshBox(... ((values.map(scalar)) as [
    number,
    number,
    number
])))
          : undefined;
      case FORM_COMMAND.SPHERE: {
        if(values.length !== 1 && values.length !== 2) {
          return undefined;
        }
        return this.store.fromMesh(meshSphere(scalar(values[0]), values.length === 2 ? scalar(values[1]) : 20));
      }
      case FORM_COMMAND.TRANSLATE:
      case FORM_COMMAND.ROTATE:
      case FORM_COMMAND.SCALE: {
        arity(name, values, 2);
        const geometry = this.geometry(values[0]);
        this.spend(count(geometry, FORM_ATTRIBUTE_DOMAIN.POINTS) + geometry.faces.length*3);
        this.store.transform(geometry, name, vector(values[1]));
        return geometry;
      }
      case FORM_COMMAND.COLOR: {
        if(values.length !== 2 || typeof values[0] !== 'object' || Array.isArray(values[0]) || values[0].kind !== FORM_VALUE_KIND.GEOMETRY) {
          return undefined;
        }
        const geometry = this.geometry(values[0]);
        const color = numeric(values[1]);
        if(!Array.isArray(color) || color.length !== 4) {
          throw new Error('color expects geometry and an RGBA color value.');
        }
        this.spend(count(geometry, FORM_ATTRIBUTE_DOMAIN.POINTS));
        if(!Object.hasOwn(geometry.attributes.points, 'color')) {
          if(this.current) {
            throw new Error('Create point colors with color(geometry, rgba) before the wrangle.');
          }
          this.store.addAttribute(geometry, FORM_ATTRIBUTE_DOMAIN.POINTS, 'color', color);
          return geometry;
        }
        for(let index = 0; index < count(geometry, FORM_ATTRIBUTE_DOMAIN.POINTS); index++) {
          this.store.set(geometry, FORM_ATTRIBUTE_DOMAIN.POINTS, 'color', index, color);
        }
        return geometry;
      }
      case FORM_COMMAND.GEOMETRY:
        if(values.length > 1) {
          throw new Error('geometry expects zero arguments or a mesh.');
        }
        if(!values.length) {
          return this.store.create();
        }
        return typeof values[0] === 'object' && !Array.isArray(values[0]) && values[0].kind === FORM_VALUE_KIND.MESH
          ? this.geometry(values[0]) : this.store.copy(this.geometry(values[0]));
      case FORM_COMMAND.GRID:
        arity(name, values, 4);
        this.spend((scalar(values[0]) + 1)*(scalar(values[1]) + 1));
        return this.store.grid(... ((values.map(scalar)) as [
    number,
    number,
    number,
    number
]));
      case FORM_COMMAND.REVOLVE:
      case FORM_COMMAND.TUBE: {
        arity(name, values, name === FORM_COMMAND.REVOLVE ? 2 : 3);
        const source = this.geometry(values[0]);
        const sides = scalar(values[name === FORM_COMMAND.REVOLVE ? 1 : 2]);
        this.spend(count(source, FORM_ATTRIBUTE_DOMAIN.POINTS)*sides*20);
        return name === FORM_COMMAND.REVOLVE ? revolveGeometry(this.store, source, sides)
          : tubeGeometry(this.store, source, scalar(values[1]), sides);
      }
      case FORM_COMMAND.SMOOTH_NORMALS: {
        arity(name, values, 1);
        const source = this.geometry(values[0]);
        this.spend(count(source, FORM_ATTRIBUTE_DOMAIN.POINTS) + source.faces.length*3);
        const result = this.store.copy(source);
        writeSmoothNormals(this.store, result);
        return result;
      }
      case FORM_COMMAND.COPY: {
        arity(name, values, 1);
        const source = this.procedural(values[0]);
        this.spend(this.size(source));
        return this.snapshot(source);
      }
      case FORM_COMMAND.ADD_POINT: {
        this.allowTopology();
        arity(name, values, 2);
        return this.store.addPoint(this.geometry(values[0]), vector(values[1]));
      }
      case FORM_COMMAND.ADD_FACE: {
        this.allowTopology();
        arity(name, values, 4);
        return this.store.addFace(this.geometry(values[0]), values.slice(1).map(scalar));
      }
      case FORM_COMMAND.REMOVE_POINT:
      case FORM_COMMAND.REMOVE_FACE: {
        this.allowTopology();
        arity(name, values, 2);
        const geometry = this.geometry(values[0]);
        this.spend(count(geometry, FORM_ATTRIBUTE_DOMAIN.POINTS) + geometry.faces.length*3);
        (name === FORM_COMMAND.REMOVE_POINT ? removePoint : removeFace)(geometry, scalar(values[1]));
        return 0;
      }
      case FORM_COMMAND.POINT_COUNT:
      case FORM_COMMAND.FACE_COUNT: {
        arity(name, values, 1);
        return count(this.geometry(values[0]), name === FORM_COMMAND.POINT_COUNT ? FORM_ATTRIBUTE_DOMAIN.POINTS : FORM_ATTRIBUTE_DOMAIN.FACES);
      }
      case FORM_COMMAND.SET_USER_DATA: {
        arity(name, values, 3);
        this.store.setUserData(this.geometry(values[0]), scalar(values[1]), dataRecord(values[2]));
        return 0;
      }
      case FORM_COMMAND.USER_DATA: {
        arity(name, values, 2);
        const geometry = this.geometry(values[0]);
        const index = scalar(values[1]);
        attributeAt(geometry, FORM_ATTRIBUTE_DOMAIN.POINTS, 'position', index);
        return {kind: FORM_VALUE_KIND.DATA, value: this.output.userDataStore.copy(geometry.userData?.[index] ?? null)};
      }
      case FORM_COMMAND.ATTRIBUTE:
        arity(name, values, 4);
        return attributeAt(this.geometry(values[0]), domain(values[1]), text(values[2]), scalar(values[3]));
      case FORM_COMMAND.ADD_ATTRIBUTE: {
        if(this.current) {
          throw new Error('Declare attributes outside wrangles with addAttribute().');
        }
        arity(name, values, 4);
        const requestedDomain = text(values[1]);
        const target = values[0];
        let geometry;
        let targetDomain;
        if(requestedDomain === FORM_ATTRIBUTE_DOMAIN.INSTANCES) {
          if(typeof target !== 'object' || Array.isArray(target) || target.kind !== FORM_VALUE_KIND.INSTANCES) {
            throw new Error('The instances domain requires an instance collection.');
          }
          geometry = target.sites;
          targetDomain = (FORM_ATTRIBUTE_DOMAIN.POINTS);
        } else {
          if(typeof target !== 'object' || Array.isArray(target) || target.kind !== FORM_VALUE_KIND.GEOMETRY) {
            throw new Error('addAttribute requires editable geometry; use geometry(mesh) first.');
          }
          geometry = target;
          targetDomain = domain(requestedDomain);
        }
        this.spend(count(geometry, targetDomain));
        this.store.addAttribute(geometry, targetDomain, text(values[2]), numeric(values[3]));
        return 0;
      }
      case FORM_COMMAND.SET_ATTRIBUTE: {
        if(this.current) {
          throw new Error('Use named element assignments inside wrangles, such as point.density = value.');
        }
        arity(name, values, 5);
        const geometry = this.geometry(values[0]);
        const targetDomain = domain(values[1]);
        const attributeName = text(values[2]);
        const index = scalar(values[3]);
        attributeAt(geometry, targetDomain, attributeName, index);
        this.store.set(geometry, targetDomain, attributeName, index, numeric(values[4]));
        return 0;
      }
      case FORM_COMMAND.SCATTER: {
        if(values.length !== 3 && values.length !== 4) {
          throw new Error('scatter expects geometry, count, seed, and optional weight attribute name.');
        }
        const geometry = this.geometry(values[0]);
        this.spend(geometry.faces.length + scalar(values[1])*(8 + Object.keys(geometry.attributes.points).length*3));
        return scatterGeometry(this.store, geometry, scalar(values[1]), scalar(values[2]), values.length === 4 ? text(values[3]) : '');
      }
      case FORM_COMMAND.INSTANCES: {
        if(values.length !== 2 && values.length !== 3) {
          throw new Error('instances expects source geometry/mesh, sites, and optional material.');
        }
        const source = this.geometry(values[0]), sites = this.geometry(values[1]);
        const material = values[2] ?? null;
        if(material !== null && (typeof material !== 'object' || Array.isArray(material) || material.kind !== FORM_VALUE_KIND.MATERIAL)) {
          throw new Error('instances expects a material value.');
        }
        this.spend(count(source, FORM_ATTRIBUTE_DOMAIN.POINTS) + count(sites, FORM_ATTRIBUTE_DOMAIN.POINTS));
        return {kind: FORM_VALUE_KIND.INSTANCES, source: this.store.copy(source), sites: this.store.copy(sites), material: material ? structuredClone(material) : null};
      }
      case FORM_COMMAND.OUTPUT:
      case FORM_COMMAND.INSPECT: {
        if(this.current) {
          throw new Error('Declare named outputs outside wrangles.');
        }
        arity(name, values, 2);
        const label = text(values[0]);
        if(!label.trim() || label.length > 64 || this.outputs.some(output => output.name === label)) {
          throw new Error('Output names must be unique, nonempty text of at most 64 characters.');
        }
        if(this.outputs.length >= 16) {
          throw new Error('Named output limit exceeded (16).');
        }
        const value = this.procedural(values[1]);
        this.spend(this.size(value));
        const snapshot = this.snapshot(value);
        const builder = new GeometryBuilder(this.output.userDataStore);
        builder.color = [...this.output.color];
        builder.userData = this.output.userData;
        builder.materialIndex = this.output.materialIndex;
        emitProcedural(snapshot, builder, this.bindMaterial);
        this.previewVertices+= builder.data.positions.length/3;
        if(this.previewVertices > 150000) {
          throw new Error('Named output preview vertex limit exceeded (150,000).');
        }
        if(name === FORM_COMMAND.OUTPUT) {
          emitProcedural(snapshot, this.output, this.bindMaterial);
        }
        this.outputs.push({name: label, visible: name === FORM_COMMAND.OUTPUT, value: snapshot, geometry: builder.data});
        return 0;
      }
      default:
        return undefined;
    }
  }
  /** @param name */
  read(name: string) {
    if(!this.current) {
      throw new Error('Attribute access requires a wrangle.');
    }
    return attributeAt(this.current.geometry, this.current.domain, name, this.current.index);
  }
  /** @param name @param value */
  write(name: string, value: ScriptValue) {
    if(!this.current) {
      throw new Error('Attribute assignment requires a wrangle.');
    }
    // Reads and assignments share the same existence check; a typo must never
    // allocate a new attribute on the current geometry.
    this.read(name);
    this.store.set(this.current.geometry, this.current.domain, name, this.current.index, numeric(value), true);
    if(name === FORM_ATTRIBUTE.POSITION) {
      this.positionsChanged = true;
    }
    if(name === FORM_ATTRIBUTE.NORMAL) {
      this.normalsWritten.add(this.current.index);
    }
  }
  /** Iterate stable topology; each body receives fresh lexical variables.
   * @param requestedDomain @param value @param body
   */
  wrangle(requestedDomain: string, value: ScriptValue, body: (index: number) => void) {
    if(this.current) {
      throw new Error('Nested wrangles are not supported; query other geometry with attribute().');
    }
    if(typeof value !== 'object' || Array.isArray(value) ||
        value.kind !== (requestedDomain === FORM_ATTRIBUTE_DOMAIN.INSTANCES ? FORM_VALUE_KIND.INSTANCES : FORM_VALUE_KIND.GEOMETRY)) {
      throw new Error('Wrangles require a geometry value, or an instance collection for the instances domain. Use geometry(mesh) to edit a mesh.');
    }
    const geometry = requestedDomain === FORM_ATTRIBUTE_DOMAIN.INSTANCES && typeof value === 'object' && !Array.isArray(value) && value.kind === FORM_VALUE_KIND.INSTANCES
      ? value.sites : this.geometry(value);
    const targetDomain = domain(requestedDomain === FORM_ATTRIBUTE_DOMAIN.INSTANCES ? FORM_ATTRIBUTE_DOMAIN.POINTS : requestedDomain);
    this.positionsChanged = false;
    this.normalsWritten.clear();
    try {
      for(let index = 0; index < count(geometry, targetDomain); index++) {
        this.spend(1);
        this.current = {geometry, domain: targetDomain, index};
        body(index);
      }
      if(this.positionsChanged) {
        // Input normals remain readable throughout the pass. Keep deliberate
        // replacements and flatten only unwritten elements after deformation.
        delete geometry.attributes.corners.normal;
        delete geometry.attributes.faces.normal;
        if(!this.normalsWritten.size) {
          delete geometry.attributes.points.normal;
        } else {
          const normals = geometry.attributes.points.normal;
          for(let index = 0; index < normals.values.length; index++) {
            if(!this.normalsWritten.has(index)) {
              normals.values[index] = [0, 0, 0];
            }
          }
        }
      }
    } finally {
      this.current = null;
    }
  }
  /** Attach named snapshots after material binding has produced the final shared table. */
  finish() {
    if(this.outputs.length) {
      this.output.data.outputs = this.outputs;
      for(const output of this.outputs) {
        output.geometry.materials = this.output.data.materials;
      }
    }
  }
  allowTopology() {
    if(this.current) {
      throw new Error('Edit topology outside wrangles so element indices remain stable.');
    }
  }
  /** Borrow editable geometry, or create a budgeted copy of a mesh value.
   * @param value */
  geometry(value: ScriptValue): GeometryValue {
    if(typeof value === 'object' && !Array.isArray(value)) {
      if(value.kind === FORM_VALUE_KIND.GEOMETRY) {
        return value;
      }
      if(value.kind === FORM_VALUE_KIND.MESH) {
        this.spend(value.positions.length/3 + value.indices.length/3);
        return this.store.fromMesh(value);
      }
    }
    throw new Error('Expected a geometry or mesh value.');
  }
  /** @param value */
  procedural(value: ScriptValue): ProceduralValue {
    if(typeof value === 'object' && !Array.isArray(value) && value.kind === FORM_VALUE_KIND.INSTANCES) {
      return value;
    }
    return this.geometry(value);
  }
  /** @param value */
  snapshot(value: ProceduralValue): ProceduralValue {
    if(value.kind === FORM_VALUE_KIND.GEOMETRY) {
      return this.store.copy(value);
    }
    return {kind: FORM_VALUE_KIND.INSTANCES, source: this.store.copy(value.source), sites: this.store.copy(value.sites), material: value.material ? structuredClone(value.material) : null};
  }
  /** @param value */
  size(value: ProceduralValue) {
    if(value.kind === FORM_VALUE_KIND.GEOMETRY) {
      return count(value, FORM_ATTRIBUTE_DOMAIN.POINTS) + value.faces.length*3;
    }
    return count(value.sites, FORM_ATTRIBUTE_DOMAIN.POINTS)*(count(value.source, FORM_ATTRIBUTE_DOMAIN.POINTS) + value.source.faces.length*3);
  }
}
/** @param name @param values @param amount */
function arity(name: string, values: unknown[], amount: number) {
  if(values.length !== amount) {
    throw new Error(`${name} expects ${amount} arguments.`);
  }
}
/** @param value */
function text(value: ScriptValue): string {
  if(typeof value !== 'string') {
    throw new Error('Expected quoted text.');
  }
  return value;
}
/** @param value */
function domain(value: ScriptValue): Domain {
  if(value !== FORM_ATTRIBUTE_DOMAIN.POINTS && value !== FORM_ATTRIBUTE_DOMAIN.FACES && value !== FORM_ATTRIBUTE_DOMAIN.CORNERS) {
    throw new Error('Expected points, faces, or corners attribute domain.');
  }
  return value;
}
/** @param value */
function numeric(value: ScriptValue): number | number[] {
  if(typeof value !== 'number' && !Array.isArray(value)) {
    throw new Error('Attributes require scalar, vector, or color values.');
  }
  return value;
}
/** @param value */
function vector(value: ScriptValue): number[] {
  if(!Array.isArray(value) || value.length !== 3) {
    throw new Error('Expected an XYZ vector.');
  }
  return value;
}
