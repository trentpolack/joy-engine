// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { PARTICLE_VALUE_KIND } from './constants.ts';

export interface Token {
  text: string;
  line: number;
  column: number;
}
export interface ScriptBudget {
  remaining: number;
}
export interface Context {
  values: Record<string, number>;
  random: () => number;
  budget: ScriptBudget;
}
export type ValueKind = typeof PARTICLE_VALUE_KIND.SCALAR | typeof PARTICLE_VALUE_KIND.VECTOR | typeof PARTICLE_VALUE_KIND.COLOR;
export type ScriptValue = number | number[];
export interface Expression {
  kind: ValueKind;
  evaluate: (context: Context) => ScriptValue;
}
export interface ScriptReference {
  kind: ValueKind;
  fields: string[];
  writable: boolean;
}
export type Instruction = (context: Context) => void;

export const PARTICLE_FIELDS = Object.freeze([
  'x',
  'y',
  'z',
  'vx',
  'vy',
  'vz',
  'ax',
  'ay',
  'az',
  'drag',
  'size',
  'rotation',
  'spin',
  'r',
  'g',
  'b',
  'alpha',
  'lightIntensity',
  'lightRange',
  'lifetime',
  'u0',
  'u1',
  'u2',
  'u3',
]);
export const PARTICLE_INPUTS = Object.freeze([
  'dt',
  'age',
  't',
  'index',
  'time',
  'originX',
  'originY',
  'originZ',
  'parentVx',
  'parentVy',
  'parentVz',
  'pi',
  'tau',
]);

/** Source location survives transport to an editor's diagnostic view. */
export class ParticleScriptError extends Error {
  declare name: string;
  declare line: number;
  declare column: number;

  /** @param message @param token */
  constructor(message: string, token: Token) {
    super(`${message} (line ${token.line}, column ${token.column})`);
    this.name = 'ParticleScriptError';
    this.line = token.line;
    this.column = token.column;
  }
}

/** Compile an original numeric language. No source is evaluated as JavaScript.
 * @param source @param parameters
 * @param [containers] Named read-only property containers.
 */
export function compileParticleScript(source: string, parameters: readonly string[], containers: Record<string, ValueKind> = {}): readonly Instruction[] {
  return Object.freeze(new Parser(source, parameters, containers).block(false));
}

/** Run compiled instructions against an owned numeric record and shared step budget.
 * @param program @param context
 */
export function runParticleScript(program: readonly Instruction[], context: Context) {
  for(const instruction of program) {
    instruction(context);
  }
}

// Fixed field maps expose containers without granting JavaScript object access.
export const PARTICLE_CONTAINERS = Object.freeze({
  position: ['x', 'y', 'z'],
  velocity: ['vx', 'vy', 'vz'],
  acceleration: ['ax', 'ay', 'az'],
  color: ['r', 'g', 'b', 'alpha'],
  origin: ['originX', 'originY', 'originZ'],
  parentVelocity: ['parentVx', 'parentVy', 'parentVz'],
});

class Parser {
  declare tokens: Token[];
  declare cursor: number;
  declare depth: number;
  declare references: Map<string, ScriptReference>;
  declare shadowableContainers: Set<string>;

  /** @param source @param parameters @param containers */
  constructor(source: string, parameters: readonly string[], containers: Record<string, ValueKind>) {
    this.tokens = tokenize(source);
    this.cursor = 0;
    this.depth = 0;
    /** Compile-time names only; runtime records stay numeric. */
    this.references = new Map();
    for(const name of [...PARTICLE_FIELDS, ...PARTICLE_INPUTS]) {
      this.references.set(name, { kind: PARTICLE_VALUE_KIND.SCALAR, fields: [name], writable: PARTICLE_FIELDS.includes(name) });
    }
    for(const [name, fields] of Object.entries(PARTICLE_CONTAINERS)) {
      this.references.set(name, {
        kind: name === PARTICLE_VALUE_KIND.COLOR ? PARTICLE_VALUE_KIND.COLOR : PARTICLE_VALUE_KIND.VECTOR, fields,
        writable: name !== 'origin' && name !== 'parentVelocity',
      });
    }
    // Version-1 user names retain precedence over newly introduced container names.
    for(const name of parameters) {
      this.references.set(name, { kind: PARTICLE_VALUE_KIND.SCALAR, fields: [name], writable: false });
    }
    this.shadowableContainers = new Set(Object.keys(PARTICLE_CONTAINERS).filter(name => !parameters.includes(name) && !(name in containers)));
    for(const [name, kind] of Object.entries(containers)) {
      const fields = kind === PARTICLE_VALUE_KIND.VECTOR ? ['X', 'Y', 'Z'].map(axis => name + axis)
        : ['r', 'g', 'b', 'a'].map(channel => `${name}.${channel}`);
      this.references.set(name, { kind, fields, writable: false });
    }
  }

  current() {
    return this.tokens[this.cursor];
  }

  /** @param value */
  take(value: string) {
    if(this.current().text !== value) {
      return false;
    }
    this.cursor++;
    return true;
  }

  /** @param value */
  expect(value: string) {
    if(!this.take(value)) {
      throw new ParticleScriptError(`Expected '${value}', found '${this.current().text}'`, this.current());
    }
  }

  /** @param nested */
  block(nested: boolean): Instruction[] {
    if(++this.depth > 32) {
      throw new ParticleScriptError('Script nesting limit exceeded', this.current());
    }
    const instructions = [];
    const outerReferences = new Map(this.references);
    const outerShadowableContainers = new Set(this.shadowableContainers);
    while(this.current().text !== '<end>' && this.current().text !== '}') {
      if(this.take(';')) {
        continue;
      }
      instructions.push(this.statement());
    }
    if(nested) {
      this.expect('}');
    } else if(this.current().text !== '<end>') {
      throw new ParticleScriptError('Unexpected closing brace', this.current());
    }
    this.references = outerReferences;
    this.shadowableContainers = outerShadowableContainers;
    this.depth--;
    return instructions;
  }

  statement(): Instruction {
    const token = this.current();
    if(this.take('if')) {
      this.expect('(');
      const condition = this.expression();
      requireKind(condition, PARTICLE_VALUE_KIND.SCALAR, token);
      this.expect(')');
      this.expect('{');
      const yes = this.block(true);

      let no: Instruction[] = [];
      if(this.take('else')) {
        this.expect('{');
        no = this.block(true);
      }
      return context => runParticleScript(condition.evaluate(context) ? yes : no, context);
    }
    const declare = this.take('let');
    const nameToken = this.current();
    const name = nameToken.text;
    if(!/^[A-Za-z_][A-Za-z_0-9]*$/.test(name)) {
      throw new ParticleScriptError('Expected a field or local variable', nameToken);
    }
    this.cursor++;
    if(declare && ((this.references.has(name) && !this.shadowableContainers.has(name)) || ['let', 'if', 'else'].includes(name))) {
      throw new ParticleScriptError(`Name '${name}' is already defined`, nameToken);
    }
    const target = declare ? null : this.reference(nameToken);
    if(target && !target.writable) {
      throw new ParticleScriptError(`Field '${name}' is not writable`, nameToken);
    }
    const operator = this.current().text;
    if(!['=', '+=', '-=', '*=', '/='].includes(operator) || (declare && operator !== '=')) {
      throw new ParticleScriptError('Expected an assignment', this.current());
    }
    this.cursor++;
    const expression = this.expression();
    const reference = target ?? {
      kind: expression.kind,
      fields: expression.kind === PARTICLE_VALUE_KIND.SCALAR ? [name] : componentNames(expression.kind).map(component => `${name}.${component}`),
      writable: true,
    };
    if(operator === '=') {
      requireKind(expression, reference.kind, token);
    } else {
      const resultKind = binaryKind(operator[0], reference.kind, expression.kind, token);
      if(resultKind !== reference.kind) {
        throw new ParticleScriptError('Assignment changes the value type', token);
      }
    }
    if(declare) {
      this.references.set(name, reference);
      this.shadowableContainers.delete(name);
    }
    return context => {
      // Evaluate first so swaps and self-references cannot observe partial writes.
      const value = expression.evaluate(context);
      const result = operator === '=' ? value : calculate(operator[0], readReference(reference, context), value);
      const components = Array.isArray(result) ? result : [result];
      for(let index = 0; index < components.length; index++) {
        context.values[reference.fields[index]] = finite(components[index], token);
      }
    };
  }

  /** Resolve only statically declared names and their valid component letters.
   * @param token */
  reference(token: Token): ScriptReference {
    const reference = this.references.get(token.text);
    if(!reference) {
      throw new ParticleScriptError(`Unknown numeric value '${token.text}' or field is not writable`, token);
    }
    if(!this.take('.')) {
      return reference;
    }
    const component = this.current();
    const index = componentNames(reference.kind).indexOf(component.text);
    if(index < 0) {
      throw new ParticleScriptError(`Invalid ${reference.kind} component '${component.text}'`, component);
    }
    this.cursor++;
    return { kind: PARTICLE_VALUE_KIND.SCALAR, fields: [reference.fields[index]], writable: reference.writable };
  }

  /** Compile precedence-ordered evaluators while retaining lazy boolean operands.
   * @param [minimum] */
  expression(minimum: number = 0): Expression {
    if(++this.depth > 64) {
      throw new ParticleScriptError('Expression nesting limit exceeded', this.current());
    }
    const token = this.current();
    this.cursor++;

    let left: Expression;
    if(['-', '+', '!'].includes(token.text)) {
      const right = this.expression(60);
      if(token.text === '!') {
        requireKind(right, PARTICLE_VALUE_KIND.SCALAR, token);
      }
      left = {
        kind: right.kind,
        evaluate: context => {
          const value = right.evaluate(context);
          if(token.text === '!') {
            return Number(!value);
          }
          return token.text === '-' ? calculate('*', value, -1) : value;
        }
      };
    } else if(token.text === '(') {
      left = this.expression();
      this.expect(')');
    } else if(/^(\d|\.\d)/.test(token.text)) {
      const value = finite(Number(token.text), token);
      left = { kind: PARTICLE_VALUE_KIND.SCALAR, evaluate: () => value };
    } else if(this.take('(')) {
      const args = [];
      if(!this.take(')')) {
        do {
          args.push(this.expression());
        } while(this.take(','));
        this.expect(')');
      }
      left = this.functionExpression(token, args);
    } else {
      const reference = this.reference(token);
      left = { kind: reference.kind, evaluate: context => readReference(reference, context) };
    }
    while((PRECEDENCE.get(this.current().text) ?? -1) >= minimum) {
      const operator = this.current();
      this.cursor++;
      const right = this.expression(((PRECEDENCE.get(operator.text)) as number) + (operator.text === '^' ? 0 : 1));
      const previous = left;
      const kind = binaryKind(operator.text, previous.kind, right.kind, operator);
      left = {
        kind,
        evaluate: context => {
          const value = previous.evaluate(context);
          if(operator.text === '&&') {
            return value ? Number(Boolean(right.evaluate(context))) : 0;
          }
          if(operator.text === '||') {
            return value ? 1 : Number(Boolean(right.evaluate(context)));
          }
          return calculate(operator.text, value, right.evaluate(context));
        }
      };
    }
    this.depth--;
    const result = left;
    return {
      kind: result.kind,
      evaluate: context => {
        const cost = result.kind === PARTICLE_VALUE_KIND.SCALAR ? 1 : componentNames(result.kind).length;
        context.budget.remaining-= cost;
        if(context.budget.remaining < 0) {
          throw new ParticleScriptError('Effect instruction budget exceeded', token);
        }
        const value = result.evaluate(context);
        if(Array.isArray(value)) {
          return value.map(component => finite(component, token));
        }
        return finite(value, token);
      }
    };
  }

  /** @param token @param args */
  functionExpression(token: Token, args: Expression[]): Expression {
    if(token.text === PARTICLE_VALUE_KIND.VECTOR || token.text === 'rgba') {
      const kind = token.text === PARTICLE_VALUE_KIND.VECTOR ? PARTICLE_VALUE_KIND.VECTOR : PARTICLE_VALUE_KIND.COLOR;
      if(args.length !== componentNames(kind).length) {
        throw new ParticleScriptError(`Wrong argument count for ${token.text}`, token);
      }
      for(const argument of args) {
        requireKind(argument, PARTICLE_VALUE_KIND.SCALAR, token);
      }
      return { kind, evaluate: context => args.map(argument =>  ((argument.evaluate(context)) as number)) };
    }
    if(token.text === 'mix' && args.length === 3 && args[0].kind !== PARTICLE_VALUE_KIND.SCALAR) {
      requireKind(args[1], args[0].kind, token);
      requireKind(args[2], PARTICLE_VALUE_KIND.SCALAR, token);
      return {
        kind: args[0].kind,
        evaluate: context => {
          const a = args[0].evaluate(context);
          const b = args[1].evaluate(context);
          const t = args[2].evaluate(context);
          return calculate('+', a, calculate('*', calculate('-', b, a), t));
        }
      };
    }
    const entry = FUNCTIONS.get(token.text);
    if(!entry || !entry.arities.includes(args.length)) {
      throw new ParticleScriptError(`Unknown function or argument count: ${token.text}`, token);
    }
    for(const argument of args) {
      requireKind(argument, PARTICLE_VALUE_KIND.SCALAR, token);
    }
    return { kind: PARTICLE_VALUE_KIND.SCALAR, evaluate: context => entry.call(args.map(argument =>  ((argument.evaluate(context)) as number)), context.random) };
  }
}

/** @param kind */
function componentNames(kind: ValueKind) {
  if(kind === PARTICLE_VALUE_KIND.VECTOR) {
    return ['x', 'y', 'z'];
  }
  return kind === PARTICLE_VALUE_KIND.COLOR ? ['r', 'g', 'b', 'a'] : [];
}

/** @param expression @param kind @param token */
function requireKind(expression: Expression, kind: ValueKind, token: Token) {
  if(expression.kind !== kind) {
    throw new ParticleScriptError(`Expected ${kind}, found ${expression.kind}`, token);
  }
}

/** @param reference @param context */
function readReference(reference: ScriptReference, context: Context): ScriptValue {
  if(reference.kind === PARTICLE_VALUE_KIND.SCALAR) {
    return context.values[reference.fields[0]];
  }
  return reference.fields.map(field => context.values[field]);
}

/** @param operator @param left @param right @param token */
function binaryKind(operator: string, left: ValueKind, right: ValueKind, token: Token): ValueKind {
  if(left === PARTICLE_VALUE_KIND.SCALAR && right === PARTICLE_VALUE_KIND.SCALAR) {
    return PARTICLE_VALUE_KIND.SCALAR;
  }
  if(['+', '-'].includes(operator) && left === right) {
    return left;
  }
  if(['*', '/'].includes(operator) && right === PARTICLE_VALUE_KIND.SCALAR) {
    return left;
  }
  if(operator === '*' && left === PARTICLE_VALUE_KIND.SCALAR) {
    return right;
  }
  throw new ParticleScriptError(`Operator '${operator}' does not accept ${left} and ${right}`, token);
}

const PRECEDENCE = new Map(
  Object.entries({
    '||': 1,
    '&&': 2,
    '==': 3,
    '!=': 3,
    '<': 4,
    '>': 4,
    '<=': 4,
    '>=': 4,
    '+': 10,
    '-': 10,
    '*': 20,
    '/': 20,
    '%': 20,
    '^': 40,
  }),
);

const FUNCTIONS: Map<string, {
    arities: number[];
    call: (args: number[], random: () => number) => number;
}> = new Map();
for(const name of  (([
  'sin',
  'cos',
  'tan',
  'abs',
  'sqrt',
  'floor',
  'ceil',
  'round',
  'exp',
  'log',
]) as const)) {
  FUNCTIONS.set(name, { arities: [1], call: (args) => Math[name](args[0]) });
}
for(const name of  ((['min', 'max', 'pow', 'atan2']) as const)) {
  FUNCTIONS.set(name, {
    arities: [2],
    call: (args) => Math[name](args[0], args[1]),
  });
}
FUNCTIONS.set('clamp', {
  arities: [3],
  call: ([value, min, max]) => Math.max(min, Math.min(max, value)),
});
FUNCTIONS.set('mix', { arities: [3], call: ([a, b, t]) => a + (b - a) * t });
FUNCTIONS.set('smoothstep', {
  arities: [3],
  call: ([a, b, x]) => {
    const t = Math.max(0, Math.min(1, (x - a) / (b - a)));
    return t * t * (3 - 2 * t);
  },
});
FUNCTIONS.set('rand', {
  arities: [0, 2],
  call: (args, random) => (args.length ? args[0] + random() * (args[1] - args[0]) : random()),
});
FUNCTIONS.set('noise', {
  arities: [1],
  call: ([x]) => {
    const cell = Math.floor(x);
    const t = x - cell;
    const hash = (n: number) => {
      const value = Math.sin(n * 127.1 + 311.7) * 43758.5453;
      return (value - Math.floor(value)) * 2 - 1;
    };
    return hash(cell) + (hash(cell + 1) - hash(cell)) * t * t * (3 - 2 * t);
  },
});

/** Component arithmetic uses copied numeric tuples; no object references escape the VM.
 * @param op @param a @param b */
function calculate(op: string, a: ScriptValue, b: ScriptValue): ScriptValue {
  if(Array.isArray(a)) {
    return a.map((value, index) =>  ((calculate(op, value, Array.isArray(b) ? b[index] : b)) as number));
  }
  if(Array.isArray(b)) {
    return b.map(value =>  ((calculate(op, a, value)) as number));
  }
  switch (op) {
    case '+':
      return a + b;
    case '-':
      return a - b;
    case '*':
      return a * b;
    case '/':
      return a / b;
    case '%':
      return a % b;
    case '^':
      return a ** b;
    case '<':
      return Number(a < b);
    case '>':
      return Number(a > b);
    case '<=':
      return Number(a <= b);
    case '>=':
      return Number(a >= b);
    case '==':
      return Number(a === b);
    case '!=':
      return Number(a !== b);
    default:
      throw new Error('Invalid compiled operator');
  }
}
/** @param value @param token */
function finite(value: number, token: Token) {
  if(!Number.isFinite(value) || Math.abs(value) > 1e9) {
    throw new ParticleScriptError('Numeric result must be finite and within ±1 billion', token);
  }
  return value;
}
/** @param source */
function tokenize(source: string): Token[] {
  if(source.length > 12000) {
    throw new ParticleScriptError('Script exceeds 12,000 characters', {
      text: '',
      line: 1,
      column: 1,
    });
  }
  const pattern =
    /(?:\d+(?:\.\d*)?|\.\d+)(?:e[+-]?\d+)?|[A-Za-z_][A-Za-z_0-9]*|(?:[+\-*/=!<>]=|&&|\|\|)|[+\-*/%^=<>!().,{};]/iy;

  const tokens: Token[] = [];
  let offset = 0,
    line = 1,
    column = 1;
  while(offset < source.length) {
    const char = source[offset];
    if(char === '#' || source.slice(offset, offset + 2) === '//') {
      while(offset < source.length && source[offset] !== '\n') {
        offset++;
        column++;
      }
      continue;
    }
    if(/\s/.test(char)) {
      if(char === '\n') {
        line++;
        column = 1;
      } else {
        column++;
      }
      offset++;
      continue;
    }
    pattern.lastIndex = offset;
    const match = pattern.exec(source);
    if(!match) {
      throw new ParticleScriptError(`Unexpected character '${char}'`, {
        text: char,
        line,
        column,
      });
    }
    tokens.push({ text: match[0], line, column });
    if(tokens.length > 2000) {
      throw new ParticleScriptError('Script exceeds 2,000 tokens', tokens[tokens.length - 1]);
    }
    offset += match[0].length;
    column += match[0].length;
  }
  tokens.push({ text: '<end>', line, column });
  return tokens;
}
