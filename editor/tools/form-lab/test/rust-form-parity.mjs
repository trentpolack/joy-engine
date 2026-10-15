// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Differential check of the Rust `joy-form` evaluator against the TypeScript FORM runtime.
//
//   cargo build --release -p joy-form --bin joy-form-evaluate     (from the engine root)
//   npm run test:rust-parity -- [--binary path] [--output directory] [--reference-only]
//
// Every corpus case is evaluated twice by TypeScript and twice by the native evaluator over its
// JSON-lines protocol. Successful results must agree on every GeometryData field (see
// `compareData`); rejections must agree on acceptance and one-based line/column. Evidence is
// written to `--output`, by default the ignored `artifacts/rust-form-parity` folder.

import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { access, mkdir, readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { compile, compileFormScript, defaultMaterial, materialPbr, meshBox } from 'joy-engine/form';
import { collectCorpus, compareData, NUMERIC_TOLERANCE } from './rust-form-corpus.mjs';

const TOOL_ROOT = fileURLToPath(new URL('../', import.meta.url));
const ENGINE_ROOT = fileURLToPath(new URL('../../../../', import.meta.url));
const DEFAULT_BINARY = resolve(ENGINE_ROOT, 'target/release/joy-form-evaluate');
const DEFAULT_OUTPUT = resolve(TOOL_ROOT, 'artifacts/rust-form-parity');
const BUILD_COMMAND = 'cargo build --release -p joy-form --bin joy-form-evaluate';
const MAX_NATIVE_OUTPUT_BYTES = 128*1024*1024;
const NATIVE_TIMEOUT_MILLISECONDS = 30000;
const DIAGNOSTIC_PREVIEW_CHARACTERS = 1500;

// Process exit codes beyond plain failure.
const EXIT_FAILED = 1;
const EXIT_NATIVE_MISSING = 2;

const RUN_MODE = Object.freeze({
  REFERENCE_ONLY: 'reference-only',
  DIFFERENTIAL: 'typescript-native-differential',
  NATIVE_MISSING: 'reference-only-native-binary-missing'
});

/**
 * @typedef {object} ParitySettings
 * @property {boolean} referenceOnly Evaluate only TypeScript and record reference evidence.
 * @property {string} binary Absolute path of the `joy-form-evaluate` executable.
 * @property {string} output Absolute evidence directory.
 */

/**
 * Run the comparison, write evidence, and set the process exit code.
 */
async function main() {
  const settings = readArguments(process.argv.slice(2));
  await mkdir(settings.output, { recursive: true });

  // Validate the comparator and reference before trusting any native result.
  verifyComparator();
  const corpus = await collectCorpus();
  const referenceOwnership = verifyReferenceOwnership();

  // Locate the native evaluator; reference evidence is still produced without it.
  const nativeAvailable = !settings.referenceOnly && await isReadable(settings.binary);
  const nativeBinaryHash = nativeAvailable ? await fileHash(settings.binary) : null;
  await writeFile(resolve(settings.output, 'form-corpus.json'), JSON.stringify(corpus, null, 2));

  // Evaluate every case.
  const rows = [];
  for(const item of corpus) {
    const row = evaluateCase(item, nativeAvailable ? settings.binary : null);
    rows.push(row);
    console.log(`${row.pass ? 'PASS' : 'FAIL'} ${item.id}${row.differences.length ? `: ${row.differences.join(' | ')}` : ''}`);
  }

  // Record evidence.
  const failures = rows.filter(row => !row.pass);
  const nativeBinaryUnchanged = !nativeAvailable || nativeBinaryHash === await fileHash(settings.binary);
  const evidence = {
    engine_base: readEngineBase(),
    mode: settings.referenceOnly ? RUN_MODE.REFERENCE_ONLY : nativeAvailable ? RUN_MODE.DIFFERENTIAL : RUN_MODE.NATIVE_MISSING,
    numeric_tolerance: { absolute: NUMERIC_TOLERANCE, relative: NUMERIC_TOLERANCE, integer_indices: 'exact', shape_order_strings_booleans_nulls: 'exact' },
    deterministic_policy: 'Every reference and native request is evaluated twice; reruns require exact complete result equality, excluding evaluation_ms.',
    error_policy: 'Rejection and one-based line/column must match; both diagnostic messages are retained without requiring identical wording.',
    timing_note: 'Two observations per case, not a benchmark or speed claim. TypeScript compile and native evaluation_ms exclude process startup and output transport.',
    native_binary: settings.referenceOnly ? null : settings.binary,
    native_binary_sha256: nativeBinaryHash,
    native_binary_unchanged_during_run: nativeAvailable ? nativeBinaryUnchanged : null,
    counts: { total: rows.length, passed: rows.length - failures.length, failed: failures.length, groups: groupCounts(corpus) },
    reference_ownership: referenceOwnership,
    ownership_limit: 'CLI JSON transport cannot prove Rust buffer ownership; the joy-form crate tests own evaluation-isolation assertions.',
    cases: rows
  };
  const filename = settings.referenceOnly ? 'form-reference-results.json' : 'form-diff-results.json';
  await writeFile(resolve(settings.output, filename), JSON.stringify(evidence, null, 2));
  if(!settings.referenceOnly) {
    await writeFile(resolve(settings.output, 'form-diff-failures.json'), JSON.stringify(failures, null, 2));
    await writeFile(resolve(settings.output, 'form-compatibility-summary.md'), compatibilitySummary(evidence));
  }
  console.log(JSON.stringify({ mode: evidence.mode, ...evidence.counts, reference_ownership: referenceOwnership, evidence: resolve(settings.output, filename) }, null, 2));

  // Report the outcome.
  if(!settings.referenceOnly && !nativeAvailable) {
    console.error(`Native evaluator missing: ${settings.binary}. Run \`${BUILD_COMMAND}\` from the engine root, or pass --binary; no cases were excluded.`);
    process.exitCode = EXIT_NATIVE_MISSING;
    return;
  }
  if(!nativeBinaryUnchanged) {
    console.error('Native evaluator changed during the run; rerun against one fixed binary before accepting the evidence.');
  }
  if(failures.length || referenceOwnership.some(check => !check.pass) || !nativeBinaryUnchanged) {
    process.exitCode = EXIT_FAILED;
  }
}

/**
 * Evaluate one case with TypeScript (twice) and, when available, the native binary (twice).
 * @param {import('./rust-form-corpus.mjs').CorpusCase} item
 * @param {string | null} binary Native evaluator, or null for reference-only rows.
 * @returns {Record<string, any>} Evidence row; `pass` and `differences` summarize it.
 */
function evaluateCase(item, binary) {
  const request = { source: item.source, overrides: item.overrides, ...(item.materials === undefined ? {} : { materials: item.materials }) };
  const before = structuredClone(request);
  const reference = evaluateReference(request);
  const referenceAgain = evaluateReference(request);
  const referenceDeterministic = exactEqual(reference.result, referenceAgain.result);
  const callerInputsUnchanged = exactEqual(request, before);

  const issues = [];
  if(reference.result.ok !== (item.expect === 'ok')) {
    issues.push(`Reference fixture expectation differs: declared ${item.expect}, TypeScript returned ${reference.result.ok ? 'ok' : 'error'}.`);
  }
  if(!referenceDeterministic) {
    issues.push('TypeScript rerun differs exactly.');
  }
  if(!callerInputsUnchanged) {
    issues.push('TypeScript evaluation mutated its caller inputs.');
  }

  const row = {
    id: item.id,
    group: item.group,
    origin: item.origin,
    expect: item.expect,
    reference: summary(reference.result),
    reference_ms: [reference.milliseconds, referenceAgain.milliseconds],
    reference_deterministic: referenceDeterministic,
    caller_inputs_unchanged: callerInputsUnchanged
  };
  if(binary) {
    compareNative(row, issues, reference.result, evaluateNative(binary, request));
  }
  row.pass = issues.length === 0;
  row.differences = issues;
  return row;
}

/**
 * Add native results and their comparison to an evidence row.
 * @param {Record<string, any>} row Mutated in place.
 * @param {string[]} issues Mutated in place.
 * @param {Record<string, any>} expected TypeScript result.
 * @param {{failure?: string, results?: Record<string, any>[]}} native
 */
function compareNative(row, issues, expected, native) {
  if(native.failure || !native.results) {
    issues.push(native.failure ?? 'Native evaluator returned no results.');
    return;
  }
  const [first, second] = native.results;
  row.native = summary(first);
  row.native_ms = [first.evaluation_ms, second.evaluation_ms];
  row.native_deterministic = exactEqual(withoutTiming(first), withoutTiming(second));
  if(!row.native_deterministic) {
    issues.push('Native rerun differs exactly.');
  }

  const difference = compareResult(expected, first);
  row.comparison = {
    acceptance: expected.ok === first.ok,
    geometry_data: expected.ok && first.ok ? difference === null : null,
    diagnostic_location: !expected.ok && !first.ok ? difference === null : null
  };
  if(difference) {
    issues.push(difference);
  }
}

/**
 * Evaluate with the TypeScript reference, normalizing failures to the native protocol shape.
 * @param {{source: string, overrides: Record<string, unknown>, materials?: unknown[]}} request
 */
function evaluateReference(request) {
  const started = performance.now();
  try {
    const data = compile(request.source, request.overrides, request.materials);
    return { result: { ok: true, data }, milliseconds: performance.now() - started };
  } catch(error) {
    const result = { ok: false, message: error.message, line: error.line ?? 1, column: error.column ?? 1 };
    return { result, milliseconds: performance.now() - started };
  }
}

/**
 * Send the same request twice to a fresh native process and parse both response lines.
 * @param {string} binary
 * @param {object} request
 * @returns {{failure?: string, results?: Record<string, any>[]}}
 */
function evaluateNative(binary, request) {
  try {
    const line = `${JSON.stringify(request)}\n`;
    const output = execFileSync(binary, {
      cwd: ENGINE_ROOT,
      input: line + line,
      encoding: 'utf8',
      maxBuffer: MAX_NATIVE_OUTPUT_BYTES,
      timeout: NATIVE_TIMEOUT_MILLISECONDS
    });
    const lines = output.trim().split('\n');
    if(lines.length !== 2) {
      return { failure: `Native protocol returned ${lines.length} lines for two requests.` };
    }
    const results = lines.map(text => JSON.parse(text));
    if(results.some(result => typeof result.ok !== 'boolean')) {
      return { failure: 'Native protocol result is missing its boolean ok discriminator.' };
    }
    return { results };
  } catch(error) {
    const details = String(error.stderr || error.message).slice(0, DIAGNOSTIC_PREVIEW_CHARACTERS);
    return { failure: `Native execution/protocol failed: ${details}` };
  }
}

/**
 * @param {Record<string, any>} expected TypeScript result.
 * @param {Record<string, any>} actual Native result.
 * @returns {string | null} First difference, or null when the results agree under the policy.
 */
function compareResult(expected, actual) {
  if(expected.ok !== actual.ok) {
    const describe = result => result.ok ? 'ok' : `${result.line}:${result.column} ${result.message}`;
    return `Result differs: TypeScript=${describe(expected)}; Rust=${describe(actual)}`;
  }
  if(expected.ok) {
    return compareData(expected.data, actual.data);
  }
  if(typeof actual.message !== 'string' || !actual.message.length) {
    return 'Rust rejection did not include a nonempty diagnostic message.';
  }
  if(expected.line !== actual.line || expected.column !== actual.column) {
    return `Diagnostic location differs: TypeScript=${expected.line}:${expected.column} ${expected.message}; Rust=${actual.line}:${actual.column} ${actual.message}`;
  }
  return null;
}

/** @param {Record<string, any>} result @returns {Record<string, any>} The result without timing. */
function withoutTiming(result) {
  const { evaluation_ms: ignored, ...stable } = result;
  return stable;
}

/** @param {string} path @returns {Promise<boolean>} */
async function isReadable(path) {
  try {
    await access(path);
    return true;
  } catch {
    return false;
  }
}

/** @param {string} path @returns {Promise<string>} SHA-256 hex digest. */
async function fileHash(path) {
  return createHash('sha256').update(await readFile(path)).digest('hex');
}

/** Read the engine commit for provenance; scratch checkouts without git report unknown. */
function readEngineBase() {
  try {
    return execFileSync('git', ['rev-parse', 'HEAD'], { cwd: ENGINE_ROOT, encoding: 'utf8' }).trim();
  } catch {
    return 'unknown';
  }
}

/** @param {unknown} expected @param {unknown} actual @returns {boolean} Deep strict equality. */
function exactEqual(expected, actual) {
  try {
    assert.deepEqual(expected, actual);
    return true;
  } catch {
    return false;
  }
}

/** @param {Record<string, any>} result @returns {Record<string, any>} Counts and a hash, not the full output. */
function summary(result) {
  if(!result.ok) {
    return { ok: false, message: result.message, line: result.line, column: result.column };
  }
  const data = result.data;
  return {
    ok: true,
    vertices: data?.positions?.length/3,
    triangles: data?.triangles?.length/3,
    points: data?.points?.length,
    materials: data?.materials?.length,
    parameters: data?.parameters?.length,
    outputs: data?.outputs?.length ?? 0,
    serialized_sha256: createHash('sha256').update(JSON.stringify(data) ?? 'undefined').digest('hex')
  };
}

/** Prove the comparator accepts tolerance-level noise and rejects every structural difference. */
function verifyComparator() {
  assert.equal(compareData({ positions: [1, 2, 3] }, { positions: [1 + 1e-9, 2, 3] }), null);
  assert.ok(compareData({ positions: [1, 2, 3] }, { positions: [1 + 1e-6, 2, 3] }));
  assert.ok(compareData({ indices: [0, 1, 2] }, { indices: [0, 1 + 1e-9, 2] }));
  assert.ok(compareData({ faces: [[0, 1, 2]] }, { faces: [[0, 1, 2 + 1e-9]] }));
  assert.ok(compareData({ positions: [0, 1, 2] }, { positions: [0, 2, 1] }));
  assert.ok(compareData({ project: null }, {}));
  assert.ok(compareData({ materials: [{ name: 'Default' }] }, { materials: [{ name: 'Changed' }] }));
  assert.ok(compareData({ userData: [null] }, { userData: [] }));
  assert.ok(compareData({ outputs: [] }, { outputs: {} }));
}

/**
 * Confirm the TypeScript reference itself isolates caller inputs and earlier outputs, so a
 * native mismatch cannot be blamed on reference aliasing.
 * @returns {{name: string, pass: boolean, difference?: string}[]}
 */
function verifyReferenceOwnership() {
  const checks = [];
  const check = (name, action) => {
    try {
      action();
      checks.push({ name, pass: true });
    } catch(error) {
      checks.push({ name, pass: false, difference: String(error.message).slice(0, 1000) });
    }
  };
  const source = `project meta(name="Owned recipe")
param origin = vector(0,0,0) meta(min=-10,max=10,step=0.1)
param piece = meshBox(1,1,1)
param finish = materialPbr(rgba(0.2,0.4,0.6,1),0,0.8)
instance(piece,origin,vector(0,0,0),vector(1,1,1),finish)`;
  const program = compileFormScript(source);
  const overrides = { origin: [1, 2, 3], piece: meshBox(0.5, 0.75, 1), finish: materialPbr([0.3, 0.6, 0.9, 1], 0.1, 0.7) };
  const palette = [defaultMaterial()];
  const inputs = structuredClone({ overrides, palette });
  const first = program.evaluate(overrides, palette);
  const saved = structuredClone(first);
  check('evaluation-does-not-mutate-caller-resources', () => assert.deepEqual({ overrides, palette }, inputs));

  overrides.origin[0] = 8;
  overrides.piece.positions[0] = 0.125;
  palette[0].baseColor[0] = 0.25;
  const changed = program.evaluate(overrides, palette);
  check('caller-and-later-evaluation-do-not-mutate-earlier-output', () => {
    assert.deepEqual(first, saved);
    assert.notDeepEqual(changed.positions, first.positions);
  });

  first.project.name = 'Mutated result';
  first.positions[0] = 999;
  first.parameters.find(parameter => parameter.name === 'piece').value.positions[0] = 888;
  first.materials[0].baseColor[0] = 0.9;
  check('returned-resource-mutation-does-not-affect-next-evaluation', () => assert.deepEqual(program.evaluate(overrides, palette), changed));
  check('failed-overrides-do-not-poison-parsed-reference-program', () => {
    assert.throws(() => program.evaluate({ origin: [1, 2] }));
    assert.deepEqual(program.evaluate(overrides, palette), changed);
  });
  return checks;
}

/** @param {{group: string}[]} corpus @returns {Record<string, number>} Case count per group. */
function groupCounts(corpus) {
  const groups = {};
  for(const item of corpus) {
    groups[item.group] = (groups[item.group] ?? 0) + 1;
  }
  return groups;
}

/**
 * Render the human-readable evidence summary.
 * @param {Record<string, any>} evidence
 * @returns {string} Markdown.
 */
function compatibilitySummary(evidence) {
  const rows = evidence.cases;
  const successes = rows.filter(row => row.reference.ok);
  const errors = rows.filter(row => !row.reference.ok);
  const count = (items, predicate) => items.filter(predicate).length;
  const ratio = (passed, total) => total ? `${passed}/${total}` : '—';
  const lines = [
    '# FORM TypeScript and Rust Compatibility',
    `Mode: ${evidence.mode}. Engine base: \`${evidence.engine_base}\`. Native binary SHA-256: \`${evidence.native_binary_sha256 ?? 'unavailable'}\`. Binary unchanged during evaluation: ${evidence.native_binary_unchanged_during_run ?? 'not evaluated'}.`,
    '',
    '| Corpus | Cases | Acceptance parity | Complete GeometryData | Error location |',
    '| --- | ---: | ---: | ---: | ---: |'
  ];
  for(const group of Object.keys(evidence.counts.groups)) {
    const items = rows.filter(row => row.group === group);
    const accepted = items.filter(row => row.reference.ok);
    const rejected = items.filter(row => !row.reference.ok);
    lines.push(`| ${group} | ${items.length} | ${count(items, row => row.comparison?.acceptance)}/${items.length} | ${ratio(count(items, row => row.comparison?.geometry_data), accepted.length)} | ${ratio(count(items, row => row.comparison?.diagnostic_location), rejected.length)} |`);
  }
  lines.push(
    '',
    `${evidence.counts.passed}/${rows.length} cases pass every assertion; ${evidence.counts.failed} cases fail. No cases are excluded.`,
    '',
    `- Successful evaluation coverage: ${successes.length} cases. Every numeric leaf is compared at absolute and relative tolerance 1e-8. Topology indices, material indices, field sets, array lengths and order, strings, booleans, and nulls are exact.`,
    `- Rejection coverage: ${errors.length} cases. Rejection and one-based source line and column must agree. Both messages are retained; their wording may differ.`,
    `- Exact deterministic reruns: TypeScript ${count(rows, row => row.reference_deterministic)}/${rows.length}; native ${count(rows, row => row.native_deterministic)}/${rows.length}. Timing is excluded.`,
    `- Reference caller ownership checks: ${count(evidence.reference_ownership, row => row.pass)}/${evidence.reference_ownership.length}; caller inputs unchanged for ${count(rows, row => row.caller_inputs_unchanged)}/${rows.length} cases.`,
    `- ${evidence.ownership_limit}`,
    '- Coverage is a fixed compatibility corpus, not proof for every accepted program. Timings are observations, not benchmark claims.',
    ''
  );
  return lines.join('\n');
}

/**
 * Parse command-line options.
 * @param {string[]} values Arguments after the script path.
 * @returns {ParitySettings}
 */
function readArguments(values) {
  const settings = { referenceOnly: false, binary: DEFAULT_BINARY, output: DEFAULT_OUTPUT };
  for(let index = 0; index < values.length; index++) {
    const value = values[index];
    if(value === '--reference-only') {
      settings.referenceOnly = true;
    } else if((value === '--binary' || value === '--output') && values[index + 1]) {
      settings[value === '--binary' ? 'binary' : 'output'] = resolve(values[++index]);
    } else {
      throw new Error(`Unknown or incomplete argument ${value}. Usage: npm run test:rust-parity -- [--reference-only] [--binary path] [--output directory]`);
    }
  }
  return settings;
}

await main();
