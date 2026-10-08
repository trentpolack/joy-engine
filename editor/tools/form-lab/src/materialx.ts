// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { FORM_MATERIAL, MATERIAL_ALPHA_MODE } from 'joy-engine/constants';
import type { Material } from 'joy-engine/form';
import { readMaterials } from 'joy-engine/form';

export type { Material };

const INPUTS = new Map([
  ['base_color', 'color3'],
  ['metallic', 'float'],
  ['roughness', 'float'],
  ['alpha', 'float'],
  ['alpha_mode', 'integer'],
  ['alpha_cutoff', 'float'],
  ['emissive', 'color3'],
]);
const ALPHA_MODES = [MATERIAL_ALPHA_MODE.OPAQUE, MATERIAL_ALPHA_MODE.MASK, MATERIAL_ALPHA_MODE.BLEND];

/** Export constant gltf_pbr surface materials as MaterialX 1.39 XML. @param materials */
export function exportMaterialX(materials: Material[]) {
  const validated = readMaterials(materials);
  if(validated.some(material => Object.values(material.textures).some(source => source !== null))) {
    throw new Error('MaterialX export cannot preserve textures. Save a .formlab project or export GLB instead.');
  }
  const lines = ['<?xml version="1.0"?>', '<materialx version="1.39">'];
  const usedNames = new Set(validated.map((material) => material.name));
  for(const [index, material] of validated.entries()) {
    let shaderName = `FormLabShader${index}`;
    while(usedNames.has(shaderName)) {
      shaderName = `_${shaderName}`;
    }
    usedNames.add(shaderName);
    lines.push(`  <gltf_pbr name="${shaderName}" type="surfaceshader">`);
    lines.push(input('base_color', 'color3', material.baseColor.slice(0, 3)));
    lines.push(input('metallic', 'float', material.metallic));
    lines.push(input('roughness', 'float', material.roughness));
    lines.push(input('alpha', 'float', material.baseColor[3]));
    lines.push(input('alpha_mode', 'integer', ALPHA_MODES.indexOf(material.alphaMode)));
    lines.push(input('alpha_cutoff', 'float', material.alphaCutoff));
    lines.push(input('emissive', 'color3', material.emissive));
    lines.push('  </gltf_pbr>');
    lines.push(`  <surfacematerial name="${material.name}" type="material">`);
    lines.push(`    <input name="surfaceshader" type="surfaceshader" nodename="${shaderName}"/>`);
    lines.push('  </surfacematerial>');
  }
  lines.push('</materialx>');
  return `${lines.join('\n')}\n`;
}

/** Import FORM LAB's constant gltf_pbr MaterialX subset. @param text */
export function importMaterialX(text: string): Material[] {
  if(typeof text !== 'string') {
    throw new Error('MaterialX source must be text.');
  }
  if(typeof DOMParser === 'undefined') {
    throw new Error('MaterialX import requires browser DOMParser support.');
  }
  if(/<!DOCTYPE|<!ENTITY/i.test(text)) {
    throw new Error('MaterialX DOCTYPE and entity declarations are not supported.');
  }
  const document = new DOMParser().parseFromString(text, 'application/xml');
  if(document.getElementsByTagName('parsererror').length > 0) {
    throw new Error('MaterialX XML is malformed.');
  }
  const root = document.documentElement;
  if(root?.tagName !== 'materialx') {
    throw new Error('Expected a MaterialX document.');
  }
  assertNoSignificantText(root);
  assertAttributes(root, new Set(['version', 'xmlns']));
  if(root.getAttribute('version') !== '1.39') {
    throw new Error(`Unsupported MaterialX version '${root.getAttribute('version') ?? ''}'.`);
  }

  const shaders = new Map();
  const materialElements = [];
  const globalNames = new Set();
  for(const element of childElements(root)) {
    const name = requireName(element);
    if(globalNames.has(name)) {
      throw new Error(`MaterialX node names must be globally unique; '${name}' is duplicated.`);
    }
    globalNames.add(name);
    if(element.tagName === 'gltf_pbr') {
      const shader = readShader(element);
      shaders.set(name, shader);
    } else if(element.tagName === 'surfacematerial') {
      materialElements.push(element);
    } else {
      throw new Error(`Unsupported MaterialX node '${element.tagName}'.`);
    }
  }

  const authoredDefault = materialElements.some((element) => element.getAttribute('name') === FORM_MATERIAL.DEFAULT);
  const materials = materialElements.map((element) => readMaterial(element, shaders));
  const referencedShaders = new Set(
    materialElements.map((element) => childElements(element)[0].getAttribute('nodename')),
  );
  for(const shaderName of shaders.keys()) {
    if(!referencedShaders.has(shaderName)) {
      throw new Error(`MaterialX shader '${shaderName}' is unreferenced and cannot be imported.`);
    }
  }
  if(materials.length === 0) {
    throw new Error('MaterialX document contains no surface materials.');
  }
  const validated = readMaterials(materials);
  return authoredDefault ? validated : validated.filter((material) => material.name !== FORM_MATERIAL.DEFAULT);
}

/** @param name @param type @param value */
function input(name: string, type: string, value: number | number[]) {
  const serialized = Array.isArray(value) ? value.join(', ') : String(value);
  return `    <input name="${name}" type="${type}" value="${serialized}"/>`;
}

/** @param element */
function readShader(element: Element) {
  assertAttributes(element, new Set(['name', 'type', 'doc', 'uiname']));
  if(element.getAttribute('type') !== 'surfaceshader') {
    throw new Error('gltf_pbr nodes must have the surfaceshader type.');
  }
  const values = new Map();
  for(const child of childElements(element)) {
    if(child.tagName !== 'input') {
      throw new Error(`Unsupported MaterialX shader child '${child.tagName}'.`);
    }
    if(child.hasAttribute('nodename') || child.hasAttribute('nodegraph') || child.hasAttribute('interfacename')) {
      throw new Error(`MaterialX input '${child.getAttribute('name') ?? ''}' must be a constant value without a connection.`);
    }
    assertAttributes(child, new Set(['name', 'type', 'value', 'doc', 'uiname', 'uifolder']));
    const name = child.getAttribute('name') ?? '';
    const expectedType = INPUTS.get(name);
    if(expectedType === undefined) {
      throw new Error(`Unsupported gltf_pbr input '${name}'.`);
    }
    if(child.getAttribute('type') !== expectedType) {
      throw new Error(`MaterialX input '${name}' must have type '${expectedType}'.`);
    }
    if(!child.hasAttribute('value')) {
      throw new Error(`MaterialX input '${name}' must be a constant value without a connection.`);
    }
    if(values.has(name)) {
      throw new Error(`Duplicate MaterialX input '${name}'.`);
    }
    values.set(name, parseValue(child.getAttribute('value') ?? '', expectedType, name));
  }
  return {
    baseColor: values.get('base_color') ?? [1, 1, 1],
    metallic: values.get('metallic') ?? 1,
    roughness: values.get('roughness') ?? 1,
    alpha: values.get('alpha') ?? 1,
    alphaMode: values.get('alpha_mode') ?? 0,
    alphaCutoff: values.get('alpha_cutoff') ?? 0.5,
    emissive: values.get('emissive') ?? [0, 0, 0],
  };
}

/** @param element @param shaders */
function readMaterial(element: Element, shaders: Map<string | null, ReturnType<typeof readShader>>) {
  assertAttributes(element, new Set(['name', 'type', 'doc', 'uiname']));
  if(element.getAttribute('type') !== 'material') {
    throw new Error('MaterialX surface materials must have type material.');
  }
  const children = childElements(element);
  if(children.length !== 1 || children[0].tagName !== 'input') {
    throw new Error('Each MaterialX surface material must contain one shader connection.');
  }
  const connection = children[0];
  assertAttributes(connection, new Set(['name', 'type', 'nodename', 'doc']));
  if(connection.getAttribute('name') !== 'surfaceshader' || connection.getAttribute('type') !== 'surfaceshader') {
    throw new Error('Unsupported MaterialX surface material input.');
  }
  const shaderName = connection.getAttribute('nodename');
  const shader = shaders.get(shaderName);
  if(shader === undefined) {
    throw new Error(`MaterialX material references missing shader '${shaderName}'.`);
  }
  const alphaMode = ALPHA_MODES[Number(shader.alphaMode)];
  if(alphaMode === undefined || !Number.isInteger(shader.alphaMode)) {
    throw new Error('MaterialX alpha_mode must be 0, 1, or 2.');
  }
  return {
    name: element.getAttribute('name'),
    baseColor: [...shader.baseColor, shader.alpha],
    metallic: shader.metallic,
    roughness: shader.roughness,
    emissive: shader.emissive,
    alphaMode,
    alphaCutoff: shader.alphaCutoff,
  };
}

/** @param text @param type @param name */
function parseValue(text: string, type: string, name: string) {
  const parts = text.split(',').map((part) => part.trim());
  const expectedLength = type === 'color3' ? 3 : 1;
  const numberSyntax = /^[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?$/;
  const hasInvalidNumber = parts.some((part) => !numberSyntax.test(part));
  if(parts.length !== expectedLength || hasInvalidNumber) {
    throw new Error(`MaterialX input '${name}' has an invalid ${type} value.`);
  }
  const components = parts.map(Number);
  if(components.some((value) => !Number.isFinite(value))) {
    throw new Error(`MaterialX input '${name}' has an invalid ${type} value.`);
  }
  if(type === 'integer' && !/^[+-]?\d+$/.test(parts[0])) {
    throw new Error(`MaterialX input '${name}' has an invalid integer value.`);
  }
  return expectedLength === 1 ? components[0] : components;
}

/** @param element @param allowed */
function assertAttributes(element: Element, allowed: Set<string>) {
  for(const attribute of Array.from(element.attributes)) {
    if(!allowed.has(attribute.name)) {
      throw new Error(`Unsupported attribute '${attribute.name}' on MaterialX <${element.tagName}>.`);
    }
  }
}

/** @param element */
function requireName(element: Element) {
  const name = element.getAttribute('name');
  if(name === null || name.length === 0) {
    throw new Error(`MaterialX <${element.tagName}> requires a name.`);
  }
  return name;
}

/** @param element */
function assertNoSignificantText(element: Element) {
  for(const node of Array.from(element.childNodes)) {
    const hasSignificantText = (node.nodeType === 3 || node.nodeType === 4)
      && (node.nodeValue ?? '').trim().length > 0;
    if(hasSignificantText) {
      throw new Error(`Unsupported text content in MaterialX <${element.tagName}>.`);
    }
    if(node.nodeType === 1) {
      assertNoSignificantText(((node) as Element));
    } else if(![3, 4, 8].includes(node.nodeType)) {
      throw new Error(`Unsupported XML node in MaterialX <${element.tagName}>.`);
    }
  }
}

/** @param element */
function childElements(element: Element): Element[] {
  return Array.from(element.childNodes).filter(

    (node) : node is Element => node.nodeType === 1,
  );
}

