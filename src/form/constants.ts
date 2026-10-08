// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/**
 * FORM parser node discriminants consumed by the evaluator.
 */
export const FORM_NODE_KIND = Object.freeze({
  WITH_USER_DATA: 'withUserData',
  WRANGLE: 'wrangle',
  IF: 'if',
  LET: 'let',
  REPEAT: 'repeat',
  BINDING_ATTRIBUTE: 'bindingAttribute',
  RECORD: 'record',
  LITERAL: 'literal',
  VARIABLE: 'variable',
  COMPONENT: 'component',
  UNARY: 'unary',
  BINARY: 'binary',
  CALL: 'call',
  PARAM: 'param',
  PROJECT: 'project',
  ARRAY: 'array',
  TEXT: 'text',
  NUMBER: 'number'
} as const);

/**
 * FORM language keywords.
 */
export const FORM_KEYWORD = Object.freeze({
  PROJECT: 'project',
  USER_DATA: 'userData',
  WRANGLE: 'wrangle',
  IN: 'in',
  AS: 'as',
  IF: 'if',
  LET: 'let',
  REPEAT: 'repeat',
  WITH: 'with',
  PARAM: 'param',
  META: 'meta'
} as const);

/**
 * FORM runtime value kinds.
 */
export const FORM_VALUE_KIND = Object.freeze({
  GEOMETRY: 'geometry',
  INSTANCES: 'instances',
  MESH: 'mesh',
  MATERIAL: 'material',
  VECTOR: 'vector',
  COLOR: 'color',
  DATA: 'data',
  ELEMENT: 'element'
} as const);

/**
 * Attribute domains available to wrangles and geometry operations.
 */
export const FORM_ATTRIBUTE_DOMAIN = Object.freeze({
  POINTS: 'points',
  FACES: 'faces',
  CORNERS: 'corners',
  INSTANCES: 'instances'
} as const);

/**
 * Public FORM parameter types.
 */
export const FORM_PARAMETER_TYPE = Object.freeze({
  SCALAR: 'scalar',
  COLOR: 'color',
  VECTOR: 'vector',
  MESH: 'mesh',
  MATERIAL: 'material'
} as const);

/**
 * FORM built-in commands dispatched by the evaluator.
 */
export const FORM_COMMAND = Object.freeze({
  GRID: 'grid',
  NOISE: 'noise',
  SMOOTHSTEP: 'smoothstep',
  POINT: 'point',
  BOX: 'box',
  SPHERE: 'sphere',
  TRIANGLE: 'triangle',
  SURFACE: 'surface',
  COLOR: 'color',
  USER_DATA: 'userData',
  SET_USER_DATA: 'setUserData',
  SEED: 'seed',
  VECTOR: 'vector',
  RGBA: 'rgba',
  UV: 'uv',
  MATERIAL: 'material',
  NORMAL: 'normal',
  MESH_BOX: 'meshBox',
  MESH_SPHERE: 'meshSphere',
  MATERIAL_PBR: 'materialPbr',
  TRANSFORM_MESH: 'transformMesh',
  DEFORM: 'deform',
  REVOLVE: 'revolve',
  TUBE: 'tube',
  SMOOTH_NORMALS: 'smoothNormals',
  INSTANCE: 'instance',
  INSTANCES: 'instances',
  GEOMETRY: 'geometry',
  COPY: 'copy',
  TRANSLATE: 'translate',
  ROTATE: 'rotate',
  SCALE: 'scale',
  ADD_POINT: 'addPoint',
  ADD_FACE: 'addFace',
  REMOVE_POINT: 'removePoint',
  REMOVE_FACE: 'removeFace',
  LENGTH: 'length',
  NORMALIZE: 'normalize',
  DOT: 'dot',
  CROSS: 'cross',
  MIX: 'mix',
  POINT_COUNT: 'pointCount',
  FACE_COUNT: 'faceCount',
  ATTRIBUTE: 'attribute',
  ADD_ATTRIBUTE: 'addAttribute',
  SET_ATTRIBUTE: 'setAttribute',
  SCATTER: 'scatter',
  OUTPUT: 'output',
  INSPECT: 'inspect'
} as const);

/**
 * Name of the default FORM material.
 */
export const FORM_MATERIAL = Object.freeze({
  DEFAULT: 'Default'
} as const);

/**
 * Built-in geometry attribute names, independent of similarly named commands.
 */
export const FORM_ATTRIBUTE = Object.freeze({
  POSITION: 'position',
  NORMAL: 'normal',
  UV: 'uv',
  ROTATION: 'rotation',
  SCALE: 'scale',
  COLOR: 'color'
} as const);
