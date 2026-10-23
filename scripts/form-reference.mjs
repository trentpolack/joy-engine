#!/usr/bin/env node
// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Keeps the FORM language reference (docs/rust-form.md) in step with the engine's language catalog.
//
// The keyword list and the built-in function table in the guide are generated from `FORM_KEYWORDS` and `BUILTINS` in
// `crates/joy-form/src/language_information.rs`, the same catalog the evaluator, completion, and hover text use. Run
// `node scripts/form-reference.mjs` to check the guide and `node scripts/form-reference.mjs --write` to regenerate the
// marked sections after editing the catalog.

import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const ENGINE_ROOT = new URL('../', import.meta.url);
const CATALOG_PATH = new URL('crates/joy-form/src/language_information.rs', ENGINE_ROOT);
const GUIDE_PATH = new URL('docs/rust-form.md', ENGINE_ROOT);

// Generated sections sit between these Markdown comments so prose around them stays hand-written.
const KEYWORDS_SECTION = 'form-keywords';
const BUILTINS_SECTION = 'form-builtins';

// Words a Markdown heading leaves in lowercase when they are not the first word.
const MINOR_HEADING_WORDS = new Set(['a', 'an', 'and', 'as', 'by', 'for', 'in', 'of', 'on', 'or', 'the', 'to']);

/**
 * Read the keyword list (with each keyword's hover documentation) and grouped built-in functions from the catalog's
 * Rust source.
 * @param {string} source - Contents of `language_information.rs`.
 * @returns {{keywords: string[], keywordDocumentation: Record<string, string>, groups: {title: string, builtins: {name: string, signature: string, summary: string}[]}[]}}
 */
export function readCatalog(source) {
  const keywordList = source.match(/pub const FORM_KEYWORDS: \[&str; \d+\] = \[([\s\S]*?)\];/);
  if(!keywordList) {
    throw new Error('FORM_KEYWORDS was not found in the catalog source.');
  }
  const keywords = [...keywordList[1].matchAll(/"([^"]+)"/g)].map(match => match[1]);

  const table = source.match(/pub const BUILTINS: &\[BuiltinDescription\] = &\[([\s\S]*?)\n\];/);
  if(!table) {
    throw new Error('BUILTINS was not found in the catalog source.');
  }

  // Each group starts at a `// ---- Title.` comment; each entry is `builtin("name", "signature", "summary")`,
  // possibly wrapped across lines by rustfmt.
  /** @type {{title: string, builtins: {name: string, signature: string, summary: string}[]}[]} */
  const groups = [];
  const entryPattern = /\/\/ ---- ([^\n]+?)\.\n|builtin\(\s*"((?:[^"\\]|\\.)*)",\s*"((?:[^"\\]|\\.)*)",\s*"((?:[^"\\]|\\.)*)",?\s*\)/g;
  for(const match of table[1].matchAll(entryPattern)) {
    if(match[1] !== undefined) {
      groups.push({ title: headingCase(match[1]), builtins: [] });
      continue;
    }
    if(groups.length === 0) {
      throw new Error(`Built-in ${match[2]} precedes the first group comment.`);
    }
    groups[groups.length - 1].builtins.push({
      name: unescapeRust(match[2]),
      signature: unescapeRust(match[3]),
      summary: unescapeRust(match[4])
    });
  }
  return { keywords, keywordDocumentation: readKeywordDocumentation(source, keywords), groups };
}

/**
 * Read the language catalog from this engine checkout, for tools outside the engine (such as the VS Code extension)
 * that present the same keywords, documentation, and built-in functions as FORM EDITOR's completion and hover.
 * @returns {ReturnType<typeof readCatalog>}
 */
export function readFormCatalog() {
  return readCatalog(readFileSync(CATALOG_PATH, 'utf8'));
}

/**
 * Read each keyword's hover text from `keyword_documentation`'s match arms (`"if" | "else" => "…"`), falling back to its
 * catch-all arm.
 * @param {string} source - Contents of `language_information.rs`.
 * @param {string[]} keywords
 * @returns {Record<string, string>}
 */
function readKeywordDocumentation(source, keywords) {
  const body = source.match(/fn keyword_documentation\(keyword: &str\) -> &'static str \{([\s\S]*?)\n\}/);
  if(!body) {
    throw new Error('keyword_documentation was not found in the catalog source.');
  }
  /** @type {Record<string, string>} */
  const documentation = {};
  let fallback = '';
  const armPattern = /((?:"[^"]+"\s*\|\s*)*"[^"]+"|_)\s*=>\s*\{?\s*"((?:[^"\\]|\\.)*)"/g;
  for(const match of body[1].matchAll(armPattern)) {
    const text = unescapeRust(match[2]);
    if(match[1] === '_') {
      fallback = text;
      continue;
    }
    for(const keyword of match[1].matchAll(/"([^"]+)"/g)) {
      documentation[keyword[1]] = text;
    }
  }
  for(const keyword of keywords) {
    documentation[keyword] ??= fallback;
  }
  return documentation;
}

/**
 * Render the generated Markdown for each marked section.
 * @param {ReturnType<typeof readCatalog>} catalog
 * @returns {Map<string, string>} Section name to its generated body.
 */
export function renderSections(catalog) {
  const keywords = catalog.keywords.map(keyword => `\`${keyword}\``).join(', ');

  const rows = [];
  for(const group of catalog.groups) {
    rows.push(`| **${group.title}** | |`);
    for(const builtin of group.builtins) {
      rows.push(`| \`${escapeCell(builtin.signature)}\` | ${escapeCell(builtin.summary)} |`);
    }
  }
  const table = ['| Function | Summary |', '| --- | --- |', ...rows].join('\n');

  return new Map([
    [KEYWORDS_SECTION, keywords],
    [BUILTINS_SECTION, table]
  ]);
}

/**
 * Replace every marked section of `guide` with its generated body.
 * @param {string} guide - The guide's Markdown.
 * @param {Map<string, string>} sections
 * @returns {string} The updated Markdown.
 */
export function updateGuide(guide, sections) {
  let updated = guide;
  for(const [name, body] of sections) {
    const start = `<!-- ${name}:start -->`;
    const end = `<!-- ${name}:end -->`;
    const startIndex = updated.indexOf(start);
    const endIndex = updated.indexOf(end);
    if(startIndex < 0 || endIndex < startIndex) {
      throw new Error(`The guide is missing its ${start} … ${end} markers.`);
    }
    updated = `${updated.slice(0, startIndex + start.length)}\n${body}\n${updated.slice(endIndex)}`;
  }
  return updated;
}

/**
 * The guide as it should read for the current catalog.
 * @returns {{current: string, expected: string}}
 */
export function generatedGuide() {
  const catalog = readCatalog(readFileSync(CATALOG_PATH, 'utf8'));
  const current = readFileSync(GUIDE_PATH, 'utf8');
  return { current, expected: updateGuide(current, renderSections(catalog)) };
}

/**
 * Title-case a catalog group comment for a Markdown heading, keeping minor words lowercase.
 * @param {string} text
 * @returns {string}
 */
function headingCase(text) {
  const words = text.split(' ');
  const cased = [];
  for(const [index, word] of words.entries()) {
    if(index > 0 && MINOR_HEADING_WORDS.has(word)) {
      cased.push(word);
      continue;
    }
    cased.push(word[0].toUpperCase() + word.slice(1));
  }
  return cased.join(' ');
}

/**
 * Undo the escapes a Rust string literal in the catalog may use.
 * @param {string} text
 * @returns {string}
 */
function unescapeRust(text) {
  return text.replace(/\\n/g, '\n').replace(/\\(["\\])/g, '$1');
}

/**
 * Keep table cells on one row: a pipe would end the cell.
 * @param {string} text
 * @returns {string}
 */
function escapeCell(text) {
  return text.replaceAll('|', '\\|');
}

/**
 * Check the guide, or rewrite it with `--write`.
 */
function main() {
  const { current, expected } = generatedGuide();
  if(process.argv.includes('--write')) {
    if(current !== expected) {
      writeFileSync(GUIDE_PATH, expected);
      console.log('Updated the FORM reference from the language catalog.');
    } else {
      console.log('The FORM reference already matches the language catalog.');
    }
    return;
  }
  if(current !== expected) {
    console.error('docs/rust-form.md is out of date with the FORM language catalog. Run: node scripts/form-reference.mjs --write');
    process.exit(1);
  }
  console.log('The FORM reference matches the language catalog.');
}

if(process.argv[1] === fileURLToPath(import.meta.url)) {
  main();
}
