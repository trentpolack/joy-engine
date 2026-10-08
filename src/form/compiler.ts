// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { FORM_COMMAND, FORM_MATERIAL, FORM_NODE_KIND, FORM_PARAMETER_TYPE, FORM_VALUE_KIND } from './constants.ts';
import type { Expression, Statement, Token } from './parser.ts';
import type { ParameterValue } from './parameters.ts';
import type { ScriptValue } from './procedural-runtime.ts';
import type { Material } from './materials.ts';
import type { MeshValue, MaterialValue } from './resources.ts';
import { createSeededRandom } from '../core/random.ts';
import { meshBox, meshSphere, materialPbr, readMesh, transformMesh } from './resources.ts';
import { readMaterials } from './materials.ts';
import { parse, ScriptError } from './parser.ts';
import { dataValue, dataRecord } from './user-data.ts';
import { GeometryBuilder, integerRange } from './geometry.ts';
export type { Expression };
export type { Statement };
export type { ParameterValue };
export type { ScriptValue };
export type Scope = Map<string, ScriptValue>;
import { vectorArithmetic, vectorFunction, noise, smoothstep } from './procedural-math.ts';
import { ProceduralRuntime } from './procedural-runtime.ts';
import { createParameter, scalar, readParameterValue } from './parameters.ts';

/** Compile an isolated document into owned geometry. No host APIs or eval are exposed.
 * @param source @param [overrides]
 * @param [materials]
 */
export function compile(source: string, overrides: Record<string, ParameterValue> = {}, materials?: Material[]) {
  return compileFormScript(source).evaluate(overrides, materials);
}

/** Parse once, evaluate explicitly with a fresh parameter snapshot each time.
 * No timers, renderer, or host APIs are retained. Evaluation returns owned Y-up geometry;
 * lengths use script units and rotations radians. Errors never mutate earlier results.
 * Unknown overrides are ignored to allow scripts to remove parameters in the editor.
 * @param source
 */
export function compileFormScript(source: string) {
  const statements = parse(source);
  return Object.freeze({
    /** Evaluate with fresh geometry, random state, and allocation budgets.
     * @param [overrides]
     * @param [materials]
     */
    evaluate(overrides: Record<string, ParameterValue> = {}, materials?: Material[]) {
      const runtime = new Runtime(overrides, materials);
      runtime.execute(statements, new Map([
        ['pi', Math.PI], ['tau', Math.PI*2], ['e', Math.E]
      ]), true);
      runtime.procedural.finish();
      return runtime.geometry.data;
    }
  });
}

class Runtime {
  declare overrides: Record<string, ParameterValue>;
  declare geometry: GeometryBuilder;
  declare random: () => number;
  declare operations: number;
  declare resourceVertices: number;
  declare procedural: ProceduralRuntime;
  declare materialIndices: Map<MaterialValue, number>;

  /** @param overrides @param materials */
  constructor(overrides: Record<string, ParameterValue>, materials: Material[] | undefined) {
    this.overrides = overrides;
    this.geometry = new GeometryBuilder();
    this.geometry.data.materials = readMaterials(materials);
    // Each evaluation starts its own random stream and resource/work budgets.
    this.random = createSeededRandom(1);
    this.operations = 0;
    this.resourceVertices = 0;
    this.procedural = new ProceduralRuntime(this.geometry, value => this.bindMaterial(value), amount => {
      if(!Number.isFinite(amount) || amount < 0 || this.operations + amount > 2000000) {
        throw new Error('Operation limit exceeded (2,000,000).');
      }
      this.operations+= amount;
    });

    this.materialIndices = new Map();
  }
  /** @param statements @param scope @param [topLevel] */
  execute(statements: Statement[], scope: Scope, topLevel: boolean = false) {
    for(const statement of statements) {
      this.spend(statement.token);
      try {
        if(statement.kind === FORM_NODE_KIND.PROJECT) {
          if(!topLevel || this.geometry.data.project) {
            throw new Error(
              'Declare project metadata once, outside repeat blocks.',
            );
          }
          this.geometry.data.project = statement.project ? {...statement.project} : null;
        } else if(statement.kind === FORM_NODE_KIND.WITH_USER_DATA) {
          const record = dataRecord(this.evaluate(defined(statement.expression), scope));
          const previousData = this.geometry.userData;
          this.geometry.userData = record;
          try {
            // Bindings belong to the block. Restore data even when a nested
            // command changes it or evaluation leaves through a diagnostic.
            this.execute(statement.body ?? [], new Map(scope));
          } finally {
            this.geometry.userData = previousData;
          }
        } else if(statement.kind === FORM_NODE_KIND.WRANGLE) {
          this.procedural.wrangle(statement.domain ?? '', this.evaluate(defined(statement.expression), scope), index => {
            const inner = new Map(scope);
            inner.set('index', index);
            if(statement.binding) {
              inner.set(statement.binding, {kind: FORM_VALUE_KIND.ELEMENT, index});
            }
            this.execute(statement.body ?? [], inner);
          });
        } else if(statement.kind === FORM_NODE_KIND.BINDING_ATTRIBUTE) {
          const binding = scope.get(statement.binding ?? '');
          if(typeof binding !== 'object' || Array.isArray(binding) || binding.kind !== FORM_VALUE_KIND.ELEMENT) {
            throw new Error(`Unknown wrangle binding '${statement.binding ?? ''}'.`);
          }
          this.procedural.write(statement.name ?? '', this.evaluate(defined(statement.expression), scope));
        } else if(statement.kind === FORM_NODE_KIND.IF) {
          if(scalar(this.evaluate(defined(statement.expression), scope)) !== 0) {
            this.execute(statement.body ?? [], new Map(scope));
          }
        } else if(statement.kind === FORM_NODE_KIND.LET) {
          scope.set(
            statement.name ?? '',
            this.evaluate(defined(statement.expression), scope),
          );
        } else if(statement.kind === FORM_NODE_KIND.PARAM) {
          if(!topLevel) {
            throw new Error(
              'Parameters must be declared outside repeat blocks.',
            );
          }
          const name = statement.name ?? '';
          if(
            this.geometry.data.parameters.some((item) => item.name === name)
          ) {
            throw new Error(`Duplicate parameter '${name}'.`);
          }
          const values = (statement.values ?? []).map((value) =>
            readParameterValue(this.evaluate(value, scope)),
          );
          const metadata = Object.fromEntries(
            Object.entries(statement.metadata ?? {}).map(
              ([key, expression]) => [key, readParameterValue(this.evaluate(expression, scope))],
            ),
          );
          const supplied = Object.hasOwn(this.overrides, name)
            ? this.overrides[name]
            : values[0];
          const parameter = createParameter(name, values, metadata, supplied);
          if(parameter.type === FORM_PARAMETER_TYPE.MESH && parameter.value.kind === FORM_VALUE_KIND.MESH) {
            this.trackMesh(parameter.value);
          }
          this.geometry.data.parameters.push(parameter);
          scope.set(name, parameter.value);
        } else if(statement.kind === FORM_NODE_KIND.REPEAT) {
          const count = scalar(
            this.evaluate(defined(statement.expression), scope),
          );
          integerRange(count, 0, 150000);
          for(let i = 0; i < count; i++) {
            this.spend(statement.token);
            const inner = new Map(scope);
            inner.set(statement.name ?? '', i);
            this.execute(statement.body ?? [], inner);
          }
        } else {
          this.command(defined(statement.expression), scope);
        }
      } catch (error) {
        if(error instanceof ScriptError) {
          throw error;
        }
        throw new ScriptError(
          error instanceof Error ? error.message : String(error),
          statement.token,
        );
      }
    }
  }
  /** @param expression @param scope */
  evaluate(expression: Expression, scope: Scope): ScriptValue {
    this.spend(expression.token);
    let value;
    switch (expression.kind) {
      case FORM_NODE_KIND.RECORD: {
        const record = Object.fromEntries(Object.entries(expression.fields ?? {}).map(([key, field]) =>
          [key, dataValue(this.evaluate(field, scope))]));
        return {kind: FORM_VALUE_KIND.DATA, value: this.geometry.userDataStore.copy(record)};
      }
      case FORM_NODE_KIND.ARRAY: {
        const items = (expression.args ?? []).map(item => dataValue(this.evaluate(item, scope)));
        return {kind: FORM_VALUE_KIND.DATA, value: this.geometry.userDataStore.copy(items)};
      }
      case FORM_NODE_KIND.LITERAL:
        return {kind: FORM_VALUE_KIND.DATA, value: expression.token.text === 'null' ? null : expression.token.text === 'true'};
      case FORM_NODE_KIND.TEXT:
        return expression.text ?? '';
      case FORM_NODE_KIND.NUMBER:
        value = expression.value ?? 0;
        break;
      case FORM_NODE_KIND.VARIABLE: {
        const name = expression.name ?? '';
        if(!scope.has(name)) {
          throw new ScriptError(
            `Unknown variable '${name}'.`,
            expression.token,
          );
        }
        value = scope.get(name) ?? 0;
        break;
      }
      case FORM_NODE_KIND.COMPONENT: {
        const parent = this.evaluate(defined(expression.left), scope);
        if(typeof parent === 'object' && !Array.isArray(parent) && parent.kind === FORM_VALUE_KIND.ELEMENT) {
          if(expression.name === 'index') {
            return parent.index;
          }
          return this.procedural.read(expression.name ?? '');
        }
        if(typeof parent === 'object' && !Array.isArray(parent) && parent.kind === FORM_VALUE_KIND.DATA) {
          const record = parent.value;
          const key = expression.name ?? '';
          if(record === null || typeof record !== 'object' || Array.isArray(record) || !Object.hasOwn(record, key)) {
            throw new ScriptError(`Unknown user data field '${key}'.`, expression.token);
          }
          const field = record[key];
          return typeof field === 'number' || typeof field === 'string' ? field : {kind: FORM_VALUE_KIND.DATA, value: field};
        }
        const keys =
          Array.isArray(parent) && parent.length === 3
            ? ['x', 'y', 'z']
            : ['r', 'g', 'b', 'a'];
        const index = keys.indexOf(expression.name ?? '');
        if(!Array.isArray(parent) || index < 0) {
          throw new ScriptError(
            'Invalid vector or color component.',
            expression.token,
          );
        }
        value = parent[index];
        break;
      }
      case FORM_NODE_KIND.UNARY: {
        const operand = this.evaluate(defined(expression.right), scope);
        const sign = expression.op === '-' ? -1 : 1;
        value = Array.isArray(operand) ? operand.map(component => sign*component) : sign*scalar(operand);
        break;
      }
      case FORM_NODE_KIND.BINARY: {
        const left = this.evaluate(defined(expression.left), scope),
          right = this.evaluate(defined(expression.right), scope);
        if(Array.isArray(left) || Array.isArray(right)) {
          value = vectorArithmetic(expression.op ?? '', left, right);
          break;
        }
        const a = scalar(left), b = scalar(right);
        switch (expression.op) {
          case '==':
            value = Number(a === b);
            break;
          case '!=':
            value = Number(a !== b);
            break;
          case '<':
            value = Number(a < b);
            break;
          case '>':
            value = Number(a > b);
            break;
          case '<=':
            value = Number(a <= b);
            break;
          case '>=':
            value = Number(a >= b);
            break;
          case '+':
            value = a + b;
            break;
          case '-':
            value = a - b;
            break;
          case '*':
            value = a * b;
            break;
          case '/':
            value = a / b;
            break;
          case '%':
            value = a % b;
            break;
          default:
            value = a ** b;
        }
        break;
      }
      case FORM_NODE_KIND.CALL: {
        const name = expression.name ?? '';
        if(name === FORM_COMMAND.DEFORM) {
          return this.deform(expression.args ?? [], scope);
        }
        const values = (expression.args ?? []).map(arg => this.evaluate(arg, scope));
        const vectorResult = vectorFunction(name, values);
        if(vectorResult !== undefined) {
          value = vectorResult;
          break;
        }
        const procedural = this.procedural.call(name, values);
        if(procedural !== undefined) {
          return procedural;
        }
        const resource = this.resource(name, values);
        if(resource) {
          return resource;
        }
        const args = values.map(scalar);
        if(name === FORM_COMMAND.VECTOR || name === FORM_COMMAND.RGBA) {
          arity(name, args, name === FORM_COMMAND.VECTOR ? 3 : 4);
          value = args;
        } else if(name === 'rand') {
          arity(name, args, 0);
          value = this.random();
        } else {
          const fn = FUNCTIONS[name];
          if(!fn) {
            throw new ScriptError(
              `Unknown function '${name}'.`,
              expression.token,
            );
          }
          arity(name, args, fn.count);
          value = fn.run(...args);
        }
        break;
      }
      default:
        throw new ScriptError(
          'Expected a numeric expression.',
          expression.token,
        );
    }
    if(typeof value === 'string' || (typeof value === 'object' && !Array.isArray(value))) {
      return value;
    }
    if(
      !(Array.isArray(value)
        ? value.every(Number.isFinite)
        : Number.isFinite(value))
    ) {
      throw new ScriptError(
        'Expression must produce a finite number.',
        expression.token,
      );
    }
    return value;
  }
  /** @param expression @param scope */
  command(expression: Expression, scope: Scope) {
    if(expression.kind !== FORM_NODE_KIND.CALL) {
      throw new Error('Expected a geometry command.');
    }
    const name = expression.name ?? '';
    const expressions = expression.args ?? [];
    if(name === FORM_COMMAND.MATERIAL) {
      if(expressions.length > 1) {
        throw new Error('material expects one material value/name, or no arguments.');
      }
      const argument = expressions[0];
      if(argument && (argument.kind !== FORM_NODE_KIND.VARIABLE || scope.has(argument.name ?? ''))) {
        this.geometry.materialIndex = this.bindMaterial(this.evaluate(argument, scope));
        return;
      }
      const materialName = argument?.name ?? FORM_MATERIAL.DEFAULT;
      const index = this.geometry.data.materials.findIndex(material => material.name === materialName);
      if(index < 0) {
        throw new Error(`Unknown material '${materialName}'. Add it in Materials or import MaterialX.`);
      }
      this.geometry.materialIndex = index;
      return;
    }
    if(name === FORM_COMMAND.SURFACE) {
      this.surface(expressions, scope);
      return;
    }
    const values = expressions.map((arg) => this.evaluate(arg, scope));
    if(name === FORM_COMMAND.USER_DATA && values.length <= 1) {
      this.geometry.userData = values.length ? dataRecord(values[0]) : null;
      return;
    }
    if(this.procedural.call(name, values) !== undefined) {
      return;
    }
    if(name === FORM_COMMAND.INSTANCE) {
      this.instance(values);
      return;
    }
    if(name === FORM_COMMAND.UV) {
      const uv = values.map(scalar);
      arity(name, uv, 2);
      this.geometry.uv = uv;
      return;
    }
    if(
      name === FORM_COMMAND.COLOR &&
      values.length === 1 &&
      Array.isArray(values[0]) &&
      values[0].length === 4
    ) {
      this.geometry.color = values[0].map((value) =>
        Math.max(0, Math.min(1, value)),
      );
      return;
    }
    const args = values.map(scalar);
    const geometry = this.geometry;
    switch (name) {
      case FORM_COMMAND.NORMAL: {
        if(args.length === 0) {
          geometry.normal = [0, 0, 0];
          break;
        }
        arity(name, args, 3);
        const scale = Math.max(...args.map(Math.abs));
        if(scale === 0) {
          throw new Error('normal expects a nonzero direction; use normal() for flat shading.');
        }
        // Scale before normalizing so large finite script values cannot overflow.
        const direction = args.map(value => value / scale);
        const length = Math.hypot(...direction);
        geometry.normal = direction.map(value => value / length);
        break;
      }
      case FORM_COMMAND.COLOR:
        if(args.length !== 3 && args.length !== 4) {
          throw new Error('color expects RGB, RGBA, or a color value.');
        }
        geometry.color = [...args.slice(0, 3), args[3] ?? 1].map((value) =>
          Math.max(0, Math.min(1, value)),
        );
        break;
      case FORM_COMMAND.SEED:
        arity(name, args, 1);
        this.random = createSeededRandom(args[0]);
        break;
      case FORM_COMMAND.POINT:
        arity(name, args, 3);
        geometry.data.points.push(geometry.vertex(args[0], args[1], args[2]));
        geometry.data.pointMaterials.push(geometry.materialIndex);
        break;
      case FORM_COMMAND.BOX:
        arity(name, args, 6);
        geometry.box(args);
        break;
      case FORM_COMMAND.SPHERE:
        if(args.length !== 4 && args.length !== 5) {
          throw new Error('sphere expects 4 or 5 arguments.');
        }
        geometry.sphere(args);
        break;
      case FORM_COMMAND.TRIANGLE: {
        arity(name, args, 9);
        const a = geometry.vertex(args[0], args[1], args[2]);
        const b = geometry.vertex(args[3], args[4], args[5]);
        const c = geometry.vertex(args[6], args[7], args[8]);
        geometry.face(a, b, c);
        break;
      }
      default:
        throw new Error(`Unknown command '${name}'.`);
    }
  }
  /** Create resource expressions without emitting geometry. @param name @param values */
  resource(name: string, values: ScriptValue[]): ScriptValue | null {
    if(name === FORM_COMMAND.MATERIAL_PBR) {
      arity(name, values, 3);
      const color = values[0];
      if(!Array.isArray(color) || color.length !== 4) {
        throw new Error('materialPbr expects RGBA color, metallic, roughness.');
      }
      return materialPbr(color, scalar(values[1]), scalar(values[2]));
    }
    let mesh;
    if(name === FORM_COMMAND.MESH_BOX) {
      arity(name, values, 3);
      mesh = meshBox(scalar(values[0]), scalar(values[1]), scalar(values[2]));
    } else if(name === FORM_COMMAND.MESH_SPHERE) {
      if(values.length !== 1 && values.length !== 2) {
        throw new Error('meshSphere expects radius and optional segments.');
      }
      mesh = meshSphere(scalar(values[0]), values.length === 2 ? scalar(values[1]) : 20);
    } else if(name === FORM_COMMAND.TRANSFORM_MESH) {
      arity(name, values, 4);
      mesh = transformMesh(requireMesh(values[0]), vector(values[1]), vector(values[2]), vector(values[3]));
    } else {
      return null;
    }
    return this.trackMesh(mesh);
  }

  /** Geometry creation budget includes intermediate resources, not just final output.
   * @param mesh
   */
  trackMesh(mesh: MeshValue) {
    this.resourceVertices+= mesh.positions.length/3;
    if(this.resourceVertices > 150000) {
      throw new Error('Mesh resource vertex limit exceeded (150,000 per evaluation).');
    }
    return mesh;
  }

  /** Immutable per-vertex expressions can use x/y/z/u/v/index; normals become flat.
   * @param expressions @param scope
   */
  deform(expressions: Expression[], scope: Scope) {
    arity('deform', expressions, 4);
    const source = requireMesh(this.evaluate(expressions[0], scope));
    const result = this.trackMesh(readMesh(source));
    const inner = new Map(scope);
    for(let i = 0; i < source.positions.length/3; i++) {
      inner.set('x', source.positions[i*3]);
      inner.set('y', source.positions[i*3 + 1]);
      inner.set('z', source.positions[i*3 + 2]);
      inner.set('u', source.uvs[i*2]);
      inner.set('v', source.uvs[i*2 + 1]);
      inner.set('index', i);
      for(let axis = 0; axis < 3; axis++) {
        result.positions[i*3 + axis] = scalar(this.evaluate(expressions[axis + 1], inner));
      }
    }
    result.normals.fill(0);
    return readMesh(result);
  }

  /** Bake a logical instance into the current preview/export geometry. Source resources remain unchanged.
   * @param values
   */
  instance(values: ScriptValue[]) {
    if(![1, 2, 4, 5].includes(values.length)) {
      throw new Error('instance expects mesh, optional position, rotation + scale, and material.');
    }
    const source = requireMesh(values[0]);
    const geometry = this.geometry;
    if(geometry.data.positions.length + source.positions.length > 150000*3 ||
        geometry.data.triangles.length + source.indices.length > 180000*3) {
      throw new Error('Instance output exceeds the geometry limit.');
    }
    const mesh = values.length === 1 ? source : transformMesh(source,
      vector(values[1]), values.length >= 4 ? vector(values[2]) : [0, 0, 0],
      values.length >= 4 ? vector(values[3]) : [1, 1, 1]);
    const saved = {normal: geometry.normal, uv: geometry.uv, materialIndex: geometry.materialIndex};
    const first = geometry.data.positions.length/3;
    if(values.length === 5) {
      geometry.materialIndex = this.bindMaterial(values[4]);
    }
    try {
      for(let i = 0; i < mesh.positions.length/3; i++) {
        geometry.normal = mesh.normals.slice(i*3, i*3 + 3);
        geometry.uv = mesh.uvs.slice(i*2, i*2 + 2);
        geometry.vertex(mesh.positions[i*3], mesh.positions[i*3 + 1], mesh.positions[i*3 + 2]);
      }
      for(let i = 0; i < mesh.indices.length; i+= 3) {
        geometry.face(first + mesh.indices[i], first + mesh.indices[i + 1], first + mesh.indices[i + 2]);
      }
    } finally {
      Object.assign(geometry, saved);
    }
  }

  /** Bind each material value once; generated names cannot collide with document materials.
   * @param value
   */
  bindMaterial(value: ScriptValue) {
    if(typeof value !== 'object' || Array.isArray(value) || value.kind !== FORM_VALUE_KIND.MATERIAL) {
      throw new Error('Expected a material value.');
    }
    const previous = this.materialIndices.get(value);
    if(previous !== undefined) {
      return previous;
    }
    const materials = this.geometry.data.materials;
    let suffix = materials.length;
    let name = `ScriptMaterial_${suffix}`;
    while(materials.some(material => material.name === name)) {
      name = `ScriptMaterial_${++suffix}`;
    }
    const next = readMaterials([...materials, {...value.material, name}]);
    this.geometry.data.materials = next;
    const index = next.length - 1;
    this.materialIndices.set(value, index);
    return index;
  }

  /** @param expressions @param scope */
  surface(expressions: Expression[], scope: Scope) {
    arity('surface', expressions, 5);
    const [columns, rows, width, depth] = expressions
      .slice(0, 4)
      .map((value) => scalar(this.evaluate(value, scope)));
    integerRange(columns, 1, 256);
    integerRange(rows, 1, 256);
    if(width <= 0 || depth <= 0) {
      throw new Error('Surface dimensions must be positive.');
    }
    const first = this.geometry.data.positions.length / 3;
    const inner = new Map(scope);
    for(let j = 0; j <= rows; j++) {
      for(let i = 0; i <= columns; i++) {
        const u = i / columns,
          v = j / rows,
          x = (u - 0.5) * width,
          z = (v - 0.5) * depth;
        inner.set('x', x);
        inner.set('z', z);
        inner.set('u', u);
        inner.set('v', v);
        this.geometry.uv = [u, v];
        this.geometry.vertex(
          x,
          scalar(this.evaluate(expressions[4], inner)),
          z,
        );
      }
    }
    for(let j = 0; j < rows; j++) {
      for(let i = 0; i < columns; i++) {
        const a = first + j * (columns + 1) + i,
          b = a + columns + 1;
        this.geometry.face(a, b, a + 1);
        this.geometry.face(a + 1, b, b + 1);
      }
    }
  }
  /** @param token */
  spend(token: Token) {
    if(++this.operations > 2000000) {
      throw new ScriptError('Operation limit exceeded (2,000,000).', token);
    }
  }
}

const FUNCTIONS: Record<string, {
    count: number;
    run: (...values: number[]) => number;
}> = {
  noise: {count: 3, run: noise},
  smoothstep: {count: 3, run: smoothstep},
  sin: { count: 1, run: Math.sin },
  cos: { count: 1, run: Math.cos },
  tan: { count: 1, run: Math.tan },
  abs: { count: 1, run: Math.abs },
  sqrt: { count: 1, run: Math.sqrt },
  floor: { count: 1, run: Math.floor },
  ceil: { count: 1, run: Math.ceil },
  round: { count: 1, run: Math.round },
  exp: { count: 1, run: Math.exp },
  log: { count: 1, run: Math.log },
  min: { count: 2, run: Math.min },
  max: { count: 2, run: Math.max },
  pow: { count: 2, run: Math.pow },
  atan2: { count: 2, run: Math.atan2 },
  clamp: { count: 3, run: (x, a, b) => Math.max(a, Math.min(b, x)) },
  mix: { count: 3, run: (a, b, t) => a + (b - a) * t },
};
/** @param name @param args @param count */
function arity(name: string, args: unknown[], count: number) {
  if(args.length !== count) {
    throw new Error(
      `${name} expects ${count} arguments; received ${args.length}.`,
    );
  }
}
/** @param expression */
function defined(expression: Expression | undefined): Expression {
  if(!expression) {
    throw new Error('Incomplete expression.');
  }
  return expression;
}

/** @param value */
function requireMesh(value: ScriptValue): MeshValue {
  if(typeof value !== 'object' || Array.isArray(value) || value.kind !== FORM_VALUE_KIND.MESH) {
    throw new Error('Expected a mesh value.');
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
