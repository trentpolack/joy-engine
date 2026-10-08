// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { FORM_ATTRIBUTE, FORM_ATTRIBUTE_DOMAIN, FORM_VALUE_KIND } from './constants.ts';
import type { UserData } from './user-data.ts';
import type { MaterialValue, MeshValue } from './resources.ts';
import { UserDataStore } from './user-data.ts';
import { integerRange } from './geometry.ts';
export type AttributeValue = number | number[];
export interface Attribute {
  size: number;
  values: AttributeValue[];
}
export type Domain = typeof FORM_ATTRIBUTE_DOMAIN.POINTS | typeof FORM_ATTRIBUTE_DOMAIN.FACES | typeof FORM_ATTRIBUTE_DOMAIN.CORNERS;
export interface GeometryValue {
  kind: typeof FORM_VALUE_KIND.GEOMETRY;
  userData?: (UserData | null)[];
  faces: number[][];
  attributes: Record<Domain, Record<string, Attribute>>;
}
export interface InstanceValue {
  kind: typeof FORM_VALUE_KIND.INSTANCES;
  source: GeometryValue;
  sites: GeometryValue;
  material: MaterialValue | null;
}
export type ProceduralValue = GeometryValue | InstanceValue;

/** Evaluation-local allocation accounting, including copies and inspection snapshots.
 * Geometry uses Y-up script units. Bindings alias; copy() and output snapshots own data.
 */
export class GeometryStore {
  declare userDataStore: UserDataStore;
  declare points: number;
  declare faces: number;
  declare components: number;

  /** @param [userDataStore] */
  constructor(userDataStore: UserDataStore = new UserDataStore()) {
    this.userDataStore = userDataStore;
    this.points = 0;
    this.faces = 0;
    this.components = 0;
  }
  /** @param points @param faces @param components */
  reserve(points: number, faces: number, components: number) {
    if(this.points + points > 150000 || this.faces + faces > 180000 || this.components + components > 2000000) {
      throw new Error('Procedural geometry allocation limit exceeded (150,000 points, 180,000 faces, 2,000,000 attribute components).');
    }
    this.points+= points;
    this.faces+= faces;
    this.components+= components;
  }
  /** Create empty owned topology; storage is charged when elements are added.
   */
  create(): GeometryValue {
    return {kind: FORM_VALUE_KIND.GEOMETRY, faces: [], attributes: {
      points: {position: {size: 3, values: []}}, faces: Object.create(null), corners: Object.create(null)
    }};
  }
  /** @param geometry @param position */
  addPoint(geometry: GeometryValue, position: number[]) {
    validateAttribute(FORM_ATTRIBUTE_DOMAIN.POINTS, FORM_ATTRIBUTE.POSITION, position);
    const attributes = geometry.attributes.points;
    this.reserve(1, 0, Object.values(attributes).reduce((sum, attribute) => sum + attribute.size, 0));
    const index = count(geometry, FORM_ATTRIBUTE_DOMAIN.POINTS);
    for(const [name, attribute] of Object.entries(attributes)) {
      attribute.values.push(name === FORM_ATTRIBUTE.POSITION ? [...position] : zero(attribute.size));
    }
    geometry.userData?.push(null);
    return index;
  }
  /** @param geometry @param points */
  addFace(geometry: GeometryValue, points: number[]) {
    if(points.length !== 3) {
      throw new Error('Faces require three point indices.');
    }
    points.forEach(index => integerRange(index, 0, count(geometry, FORM_ATTRIBUTE_DOMAIN.POINTS) - 1));
    this.reserve(0, 1, Object.values(geometry.attributes.faces).reduce((n, a) => n + a.size, 0) +
      Object.values(geometry.attributes.corners).reduce((n, a) => n + a.size*3, 0));
    geometry.faces.push([...points]);
    for(const domain of  (([FORM_ATTRIBUTE_DOMAIN.FACES, FORM_ATTRIBUTE_DOMAIN.CORNERS]) as Domain[])) {
      for(const attribute of Object.values(geometry.attributes[domain])) {
        for(let i = 0; i < (domain === FORM_ATTRIBUTE_DOMAIN.FACES ? 1 : 3); i++) {
          attribute.values.push(zero(attribute.size));
        }
      }
    }
    return geometry.faces.length - 1;
  }
  /** Create a domain attribute with owned copies of an initial value for existing elements.
   * Declarations also work on empty domains. Later topology additions retain zero defaults.
   * Reject duplicate names and invalid built-in attribute shapes before reserving storage.
   * @param geometry @param domain @param name @param value
   */
  addAttribute(geometry: GeometryValue, domain: Domain, name: string, value: AttributeValue) {
    validateAttribute(domain, name, value);
    const attributes = geometry.attributes[domain];
    if(Object.hasOwn(attributes, name)) {
      throw new Error(`${domain} attribute '${name}' already exists.`);
    }
    if(Object.keys(attributes).length >= 64) {
      throw new Error('Attribute limit exceeded (64 per domain).');
    }
    const size = Array.isArray(value) ? value.length : 1;
    const elementCount = count(geometry, domain);
    this.reserve(0, 0, elementCount*size);
    Object.defineProperty(attributes, name, {
      value: {size, values: Array.from({length: elementCount}, () => cloneAttribute(value))},
      enumerable: true, configurable: true, writable: true
    });
  }
  /** Internal generation may create attributes; script assignments must check existence first.
   * @param geometry @param domain @param name @param index @param value @param [deferNormals]
   */
  set(geometry: GeometryValue, domain: Domain, name: string, index: number, value: AttributeValue, deferNormals: boolean = false) {
    integerRange(index, 0, count(geometry, domain) - 1);
    validateAttribute(domain, name, value);
    const size = Array.isArray(value) ? value.length : 1;
    let attribute = Object.hasOwn(geometry.attributes[domain], name) ? geometry.attributes[domain][name] : undefined;
    if(!attribute) {
      if(Object.keys(geometry.attributes[domain]).length >= 64) {
        throw new Error('Attribute limit exceeded (64 per domain).');
      }
      this.reserve(0, 0, count(geometry, domain)*size);
      attribute = {size, values: Array.from({length: count(geometry, domain)}, () => zero(size))};
      geometry.attributes[domain][name] = attribute;
    }
    if(attribute.size !== size) {
      throw new Error(`Attribute '${name}' must retain its ${attribute.size}-component type.`);
    }
    attribute.values[index] = cloneAttribute(value);
    if(domain === FORM_ATTRIBUTE_DOMAIN.POINTS && name === FORM_ATTRIBUTE.POSITION && !deferNormals) {
      // Recompute flat normals at realization; author new normals after deformation.
      delete geometry.attributes.points.normal;
      delete geometry.attributes.corners.normal;
      delete geometry.attributes.faces.normal;
    }
  }
  /** Attach an owned record to a point; subsequent topology edits keep it aligned.
   * @param geometry @param index
   * @param value
   */
  setUserData(geometry: GeometryValue, index: number, value: UserData | null) {
    integerRange(index, 0, count(geometry, FORM_ATTRIBUTE_DOMAIN.POINTS) - 1);
    if(!geometry.userData) {
      geometry.userData = Array(count(geometry, FORM_ATTRIBUTE_DOMAIN.POINTS)).fill(null);
    }
    geometry.userData[index] = ((this.userDataStore.copy(value)) as UserData | null);
  }
  /** Deep-copy topology, attributes and user data into the same evaluation budget.
   * @param geometry */
  copy(geometry: GeometryValue): GeometryValue {
    const components = Object.values(geometry.attributes).flatMap(Object.values)
      .reduce((sum, attribute) => sum + attribute.size*attribute.values.length, 0);
    this.reserve(count(geometry, FORM_ATTRIBUTE_DOMAIN.POINTS), geometry.faces.length, components);

    const result: GeometryValue = structuredClone({...geometry, userData: undefined});
    if(geometry.userData) {
      result.userData = geometry.userData.map(value =>  ((this.userDataStore.copy(value)) as UserData | null));
    } else {
      delete result.userData;
    }
    return result;
  }
  /** Copy mesh data into editable point attributes and triangle topology.
   * @param mesh
   */
  fromMesh(mesh: MeshValue) {
    const geometry = this.create();
    for(let i = 0; i < mesh.positions.length; i+= 3) {
      const index = this.addPoint(geometry, mesh.positions.slice(i, i + 3));
      this.set(geometry, FORM_ATTRIBUTE_DOMAIN.POINTS, FORM_ATTRIBUTE.NORMAL, index, mesh.normals.slice(i, i + 3));
      this.set(geometry, FORM_ATTRIBUTE_DOMAIN.POINTS, FORM_ATTRIBUTE.UV, index, [mesh.uvs[i/3*2], mesh.uvs[i/3*2 + 1], 0]);
    }
    for(let i = 0; i < mesh.indices.length; i+= 3) {
      this.addFace(geometry, mesh.indices.slice(i, i + 3));
    }
    return geometry;
  }
  /** @param columns @param rows @param width @param depth */
  grid(columns: number, rows: number, width: number, depth: number) {
    integerRange(columns, 1, 256);
    integerRange(rows, 1, 256);
    if(!Number.isFinite(width) || !Number.isFinite(depth) || width <= 0 || depth <= 0) {
      throw new Error('Grid dimensions must be positive finite numbers.');
    }
    const geometry = this.create();
    for(let z = 0; z <= rows; z++) {
      for(let x = 0; x <= columns; x++) {
        const index = this.addPoint(geometry, [(x/columns - 0.5)*width, 0, (z/rows - 0.5)*depth]);
        this.set(geometry, FORM_ATTRIBUTE_DOMAIN.POINTS, FORM_ATTRIBUTE.UV, index, [x/columns, z/rows, 0]);
      }
    }
    for(let z = 0; z < rows; z++) {
      for(let x = 0; x < columns; x++) {
        const a = z*(columns + 1) + x, b = a + columns + 1;
        this.addFace(geometry, [a, b, a + 1]);
        this.addFace(geometry, [a + 1, b, b + 1]);
      }
    }
    return geometry;
  }
  /** Mutate geometry in script units. Rotation is XYZ Euler radians; normals use inverse scale.
   * @param geometry @param operation @param value
   */
  transform(geometry: GeometryValue, operation: 'translate' | 'rotate' | 'scale', value: number[]) {
    if(operation === FORM_ATTRIBUTE.SCALE && value.some(component => component === 0)) {
      throw new Error('scale components must be nonzero.');
    }
    const positions = ((geometry.attributes.points.position.values) as number[][]);
    if(operation === 'translate') {
      for(const position of positions) {
        for(let axis = 0; axis < 3; axis++) {
          position[axis]+= value[axis];
        }
        validatePosition(position);
      }
      return;
    }
    for(const position of positions) {
      const transformed = operation === 'rotate' ? rotateVector(position, value) : position.map((component, axis) => component*value[axis]);
      validatePosition(transformed);
      position.splice(0, 3, ...transformed);
    }
    const minimumScale = operation === FORM_ATTRIBUTE.SCALE ? Math.min(...value.map(Math.abs)) : 1;
    for(const domain of  (([FORM_ATTRIBUTE_DOMAIN.POINTS, FORM_ATTRIBUTE_DOMAIN.FACES, FORM_ATTRIBUTE_DOMAIN.CORNERS]) as Domain[])) {
      const normals = geometry.attributes[domain].normal;
      if(!normals) {
        continue;
      }
      for(const normal of  ((normals.values) as number[][])) {
        const transformed = operation === 'rotate' ? rotateVector(normal, value) : normal.map((component, axis) => component*minimumScale/value[axis]);
        const length = Math.hypot(...transformed);
        normal.splice(0, 3, ...(length > 0 ? transformed.map(component => component/length) : [0, 0, 0]));
      }
    }
    if(operation === FORM_ATTRIBUTE.SCALE && value[0]*value[1]*value[2] < 0) {
      for(const face of geometry.faces) {
        [face[1], face[2]] = [face[2], face[1]];
      }
      for(const attribute of Object.values(geometry.attributes.corners)) {
        for(let index = 0; index < attribute.values.length; index+= 3) {
          [attribute.values[index + 1], attribute.values[index + 2]] = [attribute.values[index + 2], attribute.values[index + 1]];
        }
      }
    }
  }
}

/** @param position */
function validatePosition(position: number[]) {
  if(!position.every(Number.isFinite) || position.some(component => Math.abs(component) > 1e6)) {
    throw new Error('Coordinates must be finite and within ±1,000,000.');
  }
}

/** Apply XYZ Euler rotation. @param value @param angles */
function rotateVector(value: number[], angles: number[]): number[] {
  const [rx, ry, rz] = angles;
  const cx = Math.cos(rx), sx = Math.sin(rx),
    cy = Math.cos(ry), sy = Math.sin(ry),
    cz = Math.cos(rz), sz = Math.sin(rz);
  const x1 = value[0],
    y1 = value[1]*cx - value[2]*sx,
    z1 = value[1]*sx + value[2]*cx;
  const x2 = x1*cy + z1*sy,
    y2 = y1,
    z2 = -x1*sy + z1*cy;
  return [x2*cz - y2*sz, x2*sz + y2*cz, z2];
}

/** @param geometry @param domain */
export function count(geometry: GeometryValue, domain: Domain) {
  return domain === FORM_ATTRIBUTE_DOMAIN.POINTS ? geometry.attributes.points.position.values.length : geometry.faces.length*(domain === FORM_ATTRIBUTE_DOMAIN.CORNERS ? 3 : 1);
}
/** Read a detached attribute value so scripts cannot bypass validation by mutating arrays.
 * @param geometry @param domain @param name @param index */
export function attributeAt(geometry: GeometryValue, domain: Domain, name: string, index: number): AttributeValue {
  integerRange(index, 0, count(geometry, domain) - 1);
  const attribute = Object.hasOwn(geometry.attributes[domain], name) ? geometry.attributes[domain][name] : undefined;
  if(!attribute) {
    throw new Error(`Unknown ${domain} attribute '${name}'.`);
  }
  return cloneAttribute(attribute.values[index]);
}
/** @param geometry @param index */
export function removeFace(geometry: GeometryValue, index: number) {
  integerRange(index, 0, geometry.faces.length - 1);
  geometry.faces.splice(index, 1);
  for(const attribute of Object.values(geometry.attributes.faces)) {
    attribute.values.splice(index, 1);
  }
  for(const attribute of Object.values(geometry.attributes.corners)) {
    attribute.values.splice(index*3, 3);
  }
}
/** Remove incident faces, compact attributes, and remap surviving topology. @param geometry @param index */
export function removePoint(geometry: GeometryValue, index: number) {
  integerRange(index, 0, count(geometry, FORM_ATTRIBUTE_DOMAIN.POINTS) - 1);
  for(let face = geometry.faces.length - 1; face >= 0; face--) {
    if(geometry.faces[face].includes(index)) {
      removeFace(geometry, face);
    } else {
      geometry.faces[face] = geometry.faces[face].map(point => point > index ? point - 1 : point);
    }
  }
  geometry.userData?.splice(index, 1);
  for(const attribute of Object.values(geometry.attributes.points)) {
    attribute.values.splice(index, 1);
  }
}
/** @param value */
export function cloneAttribute(value: AttributeValue): AttributeValue {
  return Array.isArray(value) ? [...value] : value;
}
/** @param size */
function zero(size: number): AttributeValue {
  return size === 1 ? 0 : Array(size).fill(0);
}
/** @param domain @param name @param value */
function validateAttribute(domain: Domain, name: string, value: AttributeValue) {
  if(!/^[A-Za-z_][A-Za-z_0-9]{0,63}$/.test(name) || ['__proto__', 'constructor', 'prototype'].includes(name)) {
    throw new Error('Attribute names require 1–64 identifier characters.');
  }
  const values = Array.isArray(value) ? value : [value];
  if(![1, 3, 4].includes(values.length) || !values.every(Number.isFinite)) {
    throw new Error('Attributes require finite scalars, XYZ vectors, or RGBA colors.');
  }
  const size = ([FORM_ATTRIBUTE.POSITION, FORM_ATTRIBUTE.NORMAL, FORM_ATTRIBUTE.UV, FORM_ATTRIBUTE.ROTATION, FORM_ATTRIBUTE.SCALE] as readonly string[]).includes(name) ? 3 : name === FORM_ATTRIBUTE.COLOR ? 4 : values.length;
  if(size !== values.length || (name === FORM_ATTRIBUTE.POSITION && domain !== FORM_ATTRIBUTE_DOMAIN.POINTS)) {
    throw new Error(`Attribute '${name}' requires ${size} components${name === FORM_ATTRIBUTE.POSITION ? ' on points' : ''}.`);
  }
  if(name === FORM_ATTRIBUTE.POSITION && values.some(value => Math.abs(value) > 1e6)) {
    throw new Error('Coordinates must be within ±1,000,000.');
  }
  if(name === FORM_ATTRIBUTE.COLOR && values.some(value => value < 0 || value > 1)) {
    throw new Error('Color attributes require components in 0–1.');
  }
}
