// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { FORM_KEYWORD, FORM_NODE_KIND } from './constants.ts';
import { validateDataKey } from './user-data.ts';
export interface Token {
  text: string;
  line: number;
  column: number;
}
export interface Expression {
  kind: string;
  token: Token;
  fields?: Record<string, Expression>;
  value?: number;
  text?: string;
  name?: string;
  op?: string;
  left?: Expression;
  right?: Expression;
  args?: Expression[];
}
export interface ProjectMetadata {
  name: string;
  info?: string;
  title?: string;
}
export interface Statement {
  kind: string;
  token: Token;
  project?: ProjectMetadata;
  projectNameToken?: Token;
  name?: string;
  binding?: string;
  domain?: string;
  expression?: Expression;
  values?: Expression[];
  body?: Statement[];
  metadata?: Record<string, Expression>;
}

/** Diagnostic with one-based source coordinates for authoring tools. */
export class ScriptError extends Error {
  declare line: number;
  declare column: number;

  /** @param message @param token */
  constructor(message: string, token: Token) {
    super(message);
    this.line = token.line;
    this.column = token.column;
  }
}

/** Parse source without executing JavaScript. Every node retains its source location.
 * @param source */
export function parse(source: string): Statement[] {
  return new Parser(tokenize(source)).block(false);
}

class Parser {
  declare tokens: Token[];
  declare cursor: number;
  declare depth: number;

  /** @param tokens */
  constructor(tokens: Token[]) {
    this.tokens = tokens;
    this.cursor = 0;
    this.depth = 0;
  }
  current() {
    return this.tokens[this.cursor];
  }
  /** @param text */
  take(text: string) {
    if(this.current().text !== text) {
      return false;
    }
    this.cursor++;
    return true;
  }
  /** @param text */
  expect(text: string) {
    if(!this.take(text)) {
      throw new ScriptError(
        `Expected '${text}', found '${this.current().text}'.`,
        this.current(),
      );
    }
  }
  name() {
    const token = this.current();
    if(!/^[a-zA-Z_][a-zA-Z_0-9]*$/.test(token.text)) {
      throw new ScriptError('Expected a variable name.', token);
    }
    this.cursor++;
    return token.text;
  }
  /** @param nested */
  block(nested: boolean): Statement[] {
    if(++this.depth > 48) {
      throw new ScriptError('Nesting limit exceeded.', this.current());
    }
    const statements = [];
    while(this.current().text !== '<end>' && this.current().text !== '}') {
      if(this.take(';')) {
        continue;
      }
      statements.push(this.statement());
    }
    if(nested) {
      this.expect('}');
    } else if(this.current().text === '}') {
      throw new ScriptError("Unexpected '}'.", this.current());
    }
    this.depth--;
    return statements;
  }

  statement(): Statement {
    const token = this.current();
    if(this.take(FORM_KEYWORD.PROJECT)) {
      return this.project(token);
    }
    if(this.take(FORM_KEYWORD.WITH)) {
      this.expect(FORM_KEYWORD.USER_DATA);
      this.expect('(');
      const expression = this.expression();
      this.expect(')');
      this.expect('{');
      return {kind: FORM_NODE_KIND.WITH_USER_DATA, token, expression, body: this.block(true)};
    }
    if(this.take(FORM_KEYWORD.WRANGLE)) {
      const domain = this.name();
      this.expect(FORM_KEYWORD.IN);
      const expression = this.expression();
      this.expect(FORM_KEYWORD.AS);
      const binding = this.name();
      this.expect('{');
      return {kind: FORM_NODE_KIND.WRANGLE, token, domain, expression, binding, body: this.block(true)};
    }
    if(this.take(FORM_KEYWORD.IF)) {
      const expression = this.expression();
      this.expect('{');
      return {kind: FORM_NODE_KIND.IF, token, expression, body: this.block(true)};
    }
    if(this.take(FORM_KEYWORD.LET)) {
      const name = this.name();
      this.expect('=');
      return { kind: FORM_NODE_KIND.LET, token, name, expression: this.expression() };
    }
    if(this.take(FORM_KEYWORD.PARAM)) {
      const name = this.name();
      this.expect('=');
      const values = [this.expression()];
      // Retain positional declarations when opening existing studies.
      if(this.take(',')) {
        values.push(this.expression());
        for(let i = 0; i < 2; i++) {
          this.expect(',');
          values.push(this.expression());
        }
        return { kind: FORM_NODE_KIND.PARAM, token, name, values };
      }

      const metadata: Record<string, Expression> = Object.create(null);
      if(this.take(FORM_KEYWORD.META)) {
        this.expect('(');
        if(!this.take(')')) {
          do {
            const key = this.name();
            if(!['min', 'max', 'step'].includes(key)) {
              throw new ScriptError(
                `Unknown parameter metadata '${key}'.`,
                this.current(),
              );
            }
            if(Object.hasOwn(metadata, key)) {
              throw new ScriptError(
                `Duplicate metadata '${key}'.`,
                this.current(),
              );
            }
            this.expect('=');
            metadata[key] = this.expression();
          } while(this.take(','));
          this.expect(')');
        }
      }
      return { kind: FORM_NODE_KIND.PARAM, token, name, values, metadata };
    }
    if(this.take(FORM_KEYWORD.REPEAT)) {
      const expression = this.expression();
      this.expect(FORM_KEYWORD.AS);
      const name = this.name();
      this.expect('{');
      return {
        kind: FORM_NODE_KIND.REPEAT,
        token,
        name,
        expression,
        body: this.block(true),
      };
    }
    if(this.tokens[this.cursor + 1]?.text === '.' && this.tokens[this.cursor + 3]?.text === '=') {
      const binding = this.name();
      this.expect('.');
      const name = this.name();
      this.expect('=');
      return {kind: FORM_NODE_KIND.BINDING_ATTRIBUTE, token, binding, name, expression: this.expression()};
    }
    return { kind: FORM_NODE_KIND.CALL, token, expression: this.expression() };
  }
  /** Parse document text separately from numeric expressions. @param token */
  project(token: Token): Statement {
    this.expect(FORM_KEYWORD.META);
    this.expect('(');

    const fields: Record<string, string> = Object.create(null);

    let projectNameToken: Token | undefined;
    do {
      const keyToken = this.current();
      const key = this.name();
      if(!['name', 'info', 'title'].includes(key)) {
        throw new ScriptError(`Unknown project metadata '${key}'.`, keyToken);
      }
      if(Object.hasOwn(fields, key)) {
        throw new ScriptError(`Duplicate project metadata '${key}'.`, keyToken);
      }
      this.expect('=');
      const valueToken = this.current();
      let value;
      try {
        value = JSON.parse(valueToken.text);
      } catch {
        throw new ScriptError(
          'Project metadata requires double-quoted text.',
          valueToken,
        );
      }
      if(typeof value !== 'string') {
        throw new ScriptError(
          'Project metadata requires double-quoted text.',
          valueToken,
        );
      }
      if(value.length > (key === 'info' ? 160 : 120)) {
        throw new ScriptError('Project metadata text is too long.', valueToken);
      }
      if(key === 'name' && (!value.trim() || /[\r\n]/.test(value))) {
        throw new ScriptError(
          'Project name requires nonempty, single-line text.',
          valueToken,
        );
      }
      fields[key] = value;
      if(key === 'name') {
        projectNameToken = valueToken;
      }
      this.cursor++;
    } while(this.take(','));
    this.expect(')');
    if(!projectNameToken) {
      throw new ScriptError('Project metadata requires a name.', token);
    }
    return {
      kind: FORM_NODE_KIND.PROJECT,
      token,
      project: { ...fields, name: fields.name },
      projectNameToken,
    };
  }
  /** @param [minimum] */
  expression(minimum: number = 0): Expression {
    if(++this.depth > 64) {
      throw new ScriptError(
        'Expression nesting limit exceeded.',
        this.current(),
      );
    }
    const token = this.current();
    this.cursor++;

    let left: Expression;
    if(token.text === '{') {
      const fields = Object.create(null);
      if(!this.take('}')) {
        do {
          const keyToken = this.current();
          let key;
          try {
            if(keyToken.text.startsWith('"')) {
              key = JSON.parse(keyToken.text);
              this.cursor++;
            } else {
              key = this.name();
            }
            validateDataKey(key);
          } catch(error) {
            throw new ScriptError(error instanceof Error ? error.message : String(error), keyToken);
          }
          if(Object.hasOwn(fields, key)) {
            throw new ScriptError(`Duplicate user data field '${key}'.`, keyToken);
          }
          this.expect(':');
          fields[key] = this.expression();
        } while(this.take(','));
        this.expect('}');
      }
      left = {kind: FORM_NODE_KIND.RECORD, token, fields};
    } else if(token.text === '[') {
      const args = [];
      if(!this.take(']')) {
        do {
          args.push(this.expression());
        } while(this.take(','));
        this.expect(']');
      }
      left = {kind: FORM_NODE_KIND.ARRAY, token, args};
    } else if(['true', 'false', 'null'].includes(token.text)) {
      left = {kind: FORM_NODE_KIND.LITERAL, token};
    } else if(token.text.startsWith('"')) {
      let text;
      try {
        text = JSON.parse(token.text);
      } catch {
        throw new ScriptError('Invalid quoted text.', token);
      }
      left = {kind: FORM_NODE_KIND.TEXT, token, text};
    } else if(token.text === '-' || token.text === '+') {
      left = {
        kind: FORM_NODE_KIND.UNARY,
        token,
        op: token.text,
        right: this.expression(30),
      };
    } else if(token.text === '(') {
      left = this.expression();
      this.expect(')');
    } else if(/^(\d|\.\d)/.test(token.text)) {
      left = { kind: FORM_NODE_KIND.NUMBER, token, value: Number(token.text) };
    } else if(/^[a-zA-Z_]/.test(token.text)) {
      if(this.take('(')) {
        const args = [];
        if(!this.take(')')) {
          do {
            args.push(this.expression());
          } while(this.take(','));
          this.expect(')');
        }
        left = { kind: FORM_NODE_KIND.CALL, token, name: token.text, args };
      } else {
        left = { kind: FORM_NODE_KIND.VARIABLE, token, name: token.text };
      }
    } else {
      throw new ScriptError(
        `Expected an expression, found '${token.text}'.`,
        token,
      );
    }
    while(this.take('.')) {
      left = { kind: FORM_NODE_KIND.COMPONENT, token, left, name: this.name() };
    }

    const precedence: Record<string, number> = { '==': 5, '!=': 5, '<': 5, '>': 5, '<=': 5, '>=': 5, '+': 10, '-': 10, '*': 20, '/': 20, '%': 20, '^': 40 };
    while((precedence[this.current().text] ?? -1) >= minimum) {
      const operator = this.current();
      this.cursor++;
      const right = this.expression(
        precedence[operator.text] + (operator.text === '^' ? 0 : 1),
      );
      left = {
        kind: FORM_NODE_KIND.BINARY,
        token: operator,
        op: operator.text,
        left,
        right,
      };
    }
    this.depth--;
    return left;
  }
}

/** @param source */
function tokenize(source: string): Token[] {
  if(source.length > 200000) {
    throw new ScriptError('Script size limit is 200,000 characters.', {
      text: '',
      line: 1,
      column: 1,
    });
  }

  const tokens: Token[] = [];
  let offset = 0,
    line = 1,
    column = 1;
  const pattern =
    /"(?:[^"\\\r\n]|\\.)*"|(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?|[A-Za-z_][A-Za-z_0-9]*|==|!=|<=|>=|[<>+\-*/%^=(),{};.\[\]:]/y;
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
    if(char === '@') {
      throw new ScriptError(
        "@attribute syntax is no longer supported. Use 'wrangle points in geometry as point' and 'point.name'; declare fields first with addAttribute().",
        {text: char, line, column}
      );
    }
    pattern.lastIndex = offset;
    const match = pattern.exec(source);
    if(!match) {
      throw new ScriptError(`Unexpected character '${char}'.`, {
        text: char,
        line,
        column,
      });
    }
    tokens.push({ text: match[0], line, column });
    if(tokens.length > 40000) {
      throw new ScriptError('Token limit exceeded.', tokens[tokens.length - 1]);
    }
    offset += match[0].length;
    column += match[0].length;
  }
  tokens.push({ text: '<end>', line, column });
  return tokens;
}
