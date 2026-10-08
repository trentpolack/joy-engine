// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import { mkdir, readdir, readFile, rm, writeFile } from 'node:fs/promises';
import { dirname, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseSync } from 'vite';

// Paths resolve from the engine package root so the generator runs identically
// inside this monorepo and in a standalone joy-engine checkout.
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const output = resolve(root, 'docs/api');
const groups = new Map();

await mkdir(output, { recursive: true });
await collectEntry('src/index.ts', 'joy-engine', 'core');
await collectEntry('src/project/project-manifest.ts', 'joy-engine/project-manifest', 'project');

// Pages are regenerated in full; remove ones whose subsystem no longer exports anything.
for(const file of await readdir(output)) {
  if(file.endsWith('.md') && file !== 'README.md' && !groups.has(file.slice(0, -'.md'.length))) {
    await rm(resolve(output, file));
  }
}

for(const [group, entries] of groups) {
  const content = `# ${title(group)} API\n[API index](README.md) · [Engine guide](../README.md)\n\nGenerated from public exports, TypeScript declarations and source documentation with \`npm run docs:api\`. Implementations remain the source of truth; private methods are omitted.\n\n${entries.join('\n')}`;
  await writeFile(resolve(output, `${group}.md`), `${content.trimEnd()}\n`);
}
await writeFile(resolve(output, 'README.md'), `# Joy Engine API reference
[Joy Engine README](../../README.md) · [Engine guide](../README.md)

The current public runtime surface is exported by [src/index.ts](../../src/index.ts). Import from \`joy-engine\`, not internal files. Development tooling, the project manifest and data-only constants have separate entry points; editor and Electron hosts live in Joy Editor, so games never import authoring code. Source links below point to the current checked-in implementation; generated signatures and contracts come from native TypeScript declarations and their documentation. Types can be imported with \`import type\` in TypeScript or referenced through \`import('joy-engine').TypeName\` in checked JavaScript.

## By subsystem
${[...groups.keys()].map(group => `- [${title(group)}](${group}.md)`).join('\n')}

## Units and ownership
| API family | Units and lifecycle |
| --- | --- |
| Core and geometry | Caller-defined world coordinates; radians; linear RGBA unless stated otherwise. Geometry appenders mutate caller-owned arrays. |
| Motion | Velocity, acceleration, and drag are per fixed game tick. Functions mutate the supplied body. |
| ParticleEffect | Seconds, XYZ world coordinates, exponential drag coefficient per second. The instance owns simulation; the immutable definition is borrowed. Destroy on expiry/reset/teardown. |
| Rendering | Geometry is world-space; backing dimensions are pixels; colors are linear HDR before display resolve. Resource creators own cleanup. |
| Browser and runtime | Event timestamps are milliseconds; the fixed game loop schedules application updates. Keep returned stop/unbind callbacks. |

## Typical particle integration
\`\`\`js
import { compileParticleEffect, ParticleEffect, PARTICLE_XY_BASIS, appendParticleEffect } from 'joy-engine';
import assetSource from './assets/joyfx/explosion.joyfx?raw';

const asset = JSON.parse(assetSource);

const definition = compileParticleEffect(asset);
const effect = new ParticleEffect(definition, { seed: 42, position: { x: 100, y: 200 } });
effect.step(); // Called by the owner's 60 Hz simulation update.
const transparent = [];
appendParticleEffect(transparent, effect, PARTICLE_XY_BASIS);
// Sort for the scene camera, then submit transparent through the scene renderer.
effect.destroy(); // At expiry or owner teardown.
\`\`\`

For textures, profiling, script syntax and supported limits, use the [particle guide](../particles.md). For rendering initialization and display processing, use the [rendering guide](../overview.md) and [postprocessor guide](../postprocessing.md).

## Shader assets
The \`joy-engine/shaders/*\` package export exposes reusable authored WGSL/GLSL assets for raw-source loading. Scene-specific programs live in each project's \`shaders/\` directory and are imported locally. See [shader ownership](../shaders.md).

## Maintenance
Run \`npm run docs:api\` whenever exports, public types or documentation change. The generator resolves local re-exports, fails when an exported symbol is missing, and includes public class methods, native interfaces and type aliases. External re-exports are named with their package source. This is a source-derived reference, not a separately versioned API promise.
`);
console.log(`Documented ${groups.size} public API groups.`);

/** Resolve public declarations without executing GPU or browser imports. */
async function collectEntry(path, entryPoint, localGroup) {
  const entry = await readSource(path);
  for(const statement of entry.program.body) {
    if(statement.type === 'ExportAllDeclaration' && statement.source.value.startsWith('.')) {
      await collectEntry(relative(root, resolve(dirname(entry.filename), statement.source.value)), entryPoint, localGroup);
      continue;
    }
    if(statement.type !== 'ExportNamedDeclaration') {
      continue;
    }
    if(statement.source) {
      const sourcePath = statement.source.value;
      if(!sourcePath.startsWith('.')) {
        for(const element of statement.specifiers) {
          const group = sourcePath.startsWith('@luma.gl/') ? 'rendering' : localGroup;
          append(group, `## ${element.exported.name}\nRe-exported from \`${sourcePath}\` through \`${entryPoint}\`. The underlying package owns its API.\n`);
        }
        continue;
      }
      const source = await readSource(relative(root, resolve(dirname(entry.filename), sourcePath)));
      const group = relative(resolve(root, 'src'), source.filename).split('/')[0];
      for(const element of statement.specifiers) {
        const declaration = findDeclaration(source, element.local.name);
        addDeclaration(statement.exportKind === 'type' ? 'types' : group, entryPoint, element.exported.name, source, declaration);
      }
    } else if(statement.declaration?.id) {
      const declaration = statement.declaration;
      addDeclaration(isTypeDeclaration(declaration) ? 'types' : localGroup, entryPoint, declaration.id.name, entry, declaration);
    }
  }

}

/** Parse using the repository's existing Vite parser; no new dependency. */
async function readSource(path) {
  const filename = resolve(root, path);
  const text = await readFile(filename, 'utf8');
  const parsed = parseSync(filename, text);
  if(parsed.errors.length) {
    throw new Error(`Cannot document ${filename}: ${parsed.errors.map(error => error.message).join(', ')}`);
  }
  return { ...parsed, filename, text };
}

/** Fail on stale exports rather than silently omitting part of the API. */
function findDeclaration(source, name) {
  for(const statement of source.program.body) {
    const node = statement.declaration ?? statement;
    if(node.id?.name === name || node.declarations?.some(item => item.id.name === name)) {
      return node;
    }
  }
  throw new Error(`Cannot document ${name} in ${source.filename}`);
}

/** Render signatures and public methods; leave implementation bodies in source. */
function addDeclaration(group, entryPoint, name, source, node) {
  let content = `## ${name}\nImport from \`${entryPoint}\`. [Source](${relative(output, source.filename)})\n\n`;
  const language = source.filename.endsWith('.ts') ? 'ts' : 'js';
  if(isTypeDeclaration(node)) {
    content+= `\`\`\`ts\n${source.text.slice(node.start, node.end).trim()}\n\`\`\`\n${comments(source, node)}\n`;
  } else if(node.type === 'ClassDeclaration') {
    content += comments(source, node);
    for(const member of node.body.body) {
      const doc = comments(source, member);
      if(!member.value?.body || member.accessibility === 'private' || member.accessibility === 'protected' || doc.includes('**private**')) {
        continue;
      }
      const signature = source.text.slice(member.start, member.value.body.start).trim();
      content += `### ${member.key.name}\n\`\`\`${language}\n${signature}\n\`\`\`\n${doc}\n`;
    }
  } else {
    const signature = node.body ? source.text.slice(node.start, node.body.start).trim() : `const ${name}`;
    content += `\`\`\`${language}\n${signature}\n\`\`\`\n${comments(source, node)}\n`;
  }
  append(group, content);
}

/** Native contracts have no runtime value, but their full members belong in the API reference. */
function isTypeDeclaration(node) {
  return node.type === 'TSTypeAliasDeclaration' || node.type === 'TSInterfaceDeclaration';
}

/** A declaration owns only the adjacent JSDoc, allowing an export keyword. */
function comments(source, node) {
  const comment = source.comments.findLast(comment => comment.type === 'Block' && comment.value.startsWith('*') && comment.end <= node.start && /^(?:\s|export)*$/.test(source.text.slice(comment.end, node.start)));
  return comment ? formatComment(comment.value) : '';
}

/** Preserve contract types verbatim; turn JSDoc tags into readable list items. */
function formatComment(value) {
  const text = value.replace(/^\*/, '').replace(/^\s*\* ?/gm, '').trim();
  const [prose, ...tags] = text.split(/\s*@(?=\w)/);
  let result = prose.trim() ? `${prose.replace(/\s*\n\s*/g, ' ')}\n\n` : '';
  for(const tag of tags) {
    const [, kind, detail] = tag.match(/^(\w+)\s*([\s\S]*)$/);
    const formattedDetail = formatTagDetail(detail.trim().replace(/\s*\n\s*/g, ' '));
    result += `- **${kind}**${formattedDetail ? ` ${formattedDetail}` : ''}\n`;
  }
  return result;
}
/** Keep generic types out of Markdown's HTML parser, including nested records. */
function formatTagDetail(detail) {
  if(!detail.startsWith('{')) {
    return detail;
  }
  let depth = 0;
  for(let index = 0; index < detail.length; index++) {
    if(detail[index] === '{') {
      depth++;
    } else if(detail[index] === '}') {
      depth--;
      if(depth === 0) {
        return `\`${detail.slice(1, index)}\`${detail.slice(index + 1)}`;
      }
    }
  }
  return detail;
}

/** Preserve public export order within each subsystem's generated page. */
function append(group, content) {
  if(!groups.has(group)) {
    groups.set(group, []);
  }
  groups.get(group).push(content);
}
/** Convert a source-directory key into its documentation heading. */
function title(group) {
  return group === 'dcc' ? 'DCC interchange' : group[0].toUpperCase() + group.slice(1);
}
