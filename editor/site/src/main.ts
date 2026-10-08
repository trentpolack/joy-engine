// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { BROWSER_EVENT } from 'joy-engine/constants';
import type { ProjectManifest } from 'joy-engine/project-manifest';
import { parseProjectManifest, JOY_PROJECT_MANIFEST } from 'joy-engine/project-manifest';
import '../css/workspace.css';

export type { ProjectManifest };

const elements = {
  open: requireElement('open-project', HTMLButtonElement),
  name: requireElement('project-name', HTMLElement),
  path: requireElement('project-path', HTMLElement),
  capabilities: requireElement('capabilities', HTMLElement),
  status: requireElement('status', HTMLElement),
  empty: requireElement('empty', HTMLElement),
  preview: requireElement('preview', HTMLIFrameElement),
  docked: requireElement('preview-docked', HTMLButtonElement),
  hidden: requireElement('preview-hidden', HTMLButtonElement),
  popout: requireElement('preview-popout', HTMLButtonElement)
};

let manifest: ProjectManifest | null = null;

let previewUrl: string | null = null;

elements.open.addEventListener(BROWSER_EVENT.CLICK, () => void openProject());
elements.docked.addEventListener(BROWSER_EVENT.CLICK, () => setPreviewMode('docked'));
elements.hidden.addEventListener(BROWSER_EVENT.CLICK, () => setPreviewMode('hidden'));
elements.popout.addEventListener(BROWSER_EVENT.CLICK, () => void popOutPreview());

if(window.joyEditor?.initialProject) {
  void openDesktopProject(window.joyEditor.initialProject);
}

async function openProject() {
  try {
    if(window.joyEditor) {
      const project = await window.joyEditor.openProject();
      if(project) {
        await openDesktopProject(project);
      }
      return;
    }

    if(!window.showDirectoryPicker) {
      throw new Error('This browser cannot select folders. Run npm run dev:editor for the desktop development host.');
    }

    const directory = await window.showDirectoryPicker({mode: 'read'});
    const file = await directory.getFileHandle(JOY_PROJECT_MANIFEST);
    const text = await (await file.getFile()).text();
    const parsed = parseProjectManifest(JSON.parse(text));
    const root = await descend(directory, parsed.preview.root);
    const entry = await root.getFileHandle(parsed.preview.entry);
    const url = URL.createObjectURL(await entry.getFile());
    applyProject(parsed, directory.name, url);
  } catch(error) {
    showError(error);
  }
}

/** 
 * Open a project from the desktop host. This is a separate code path from openProject() because the desktop host can provide a dependency tree and a preview URL, while the browser can only provide a self-contained build entry.
 * @param project
 */
async function openDesktopProject(project: {
    manifest: unknown;
    path: string;
    previewUrl: string;
}) {
  applyProject(parseProjectManifest(project.manifest), project.path, project.previewUrl);
}

/** 
 * Apply a project manifest to the UI.
 * @param next @param projectPath @param url
 */
function applyProject(next: ProjectManifest, projectPath: string, url: string) {
  manifest = next;
  previewUrl = url;
  elements.name.textContent = next.name;
  elements.path.textContent = projectPath;
  elements.capabilities.replaceChildren(...next.capabilities.map(capability => {
    const item = document.createElement('section');
    const features = Object.entries(capability.features).filter(([, enabled]) => enabled).map(([name]) => name).join(' · ');
    item.innerHTML = `<h2>${escapeText(capability.label)}</h2><p>${escapeText(capability.extensions.join(', '))}</p><small>${escapeText(features || 'No optional renderer features')}</small>`;
    return item;
  }));
  elements.preview.src = url;
  elements.preview.hidden = false;
  elements.empty.hidden = true;
  elements.popout.disabled = !window.joyEditor;
  elements.status.textContent = `${next.capabilities.length} project-owned ${next.capabilities.length === 1 ? 'adapter' : 'adapters'} available`;
  setPreviewMode('docked');
}

/** @param mode */
function setPreviewMode(mode: 'docked' | 'hidden') {
  const visible = mode === 'docked' && Boolean(manifest);
  elements.preview.hidden = !visible;
  elements.empty.hidden = visible || Boolean(manifest);
  elements.docked.setAttribute('aria-pressed', String(mode === 'docked'));
  elements.hidden.setAttribute('aria-pressed', String(mode === 'hidden'));
  document.body.dataset.preview = mode;
}

async function popOutPreview() {
  if(!window.joyEditor || !previewUrl) {
    return;
  }
  await window.joyEditor.popOutPreview();
  setPreviewMode('hidden');
  elements.status.textContent = 'Play preview opened in a separate window.';
}

/** Browser folder handles cannot serve a dependency tree, but can preview a self-contained build entry. */
/** @param directory @param relativePath */
async function descend(directory: FileSystemDirectoryHandle, relativePath: string) {
  let current = directory;
  for(const part of relativePath.split('/').filter(Boolean)) {
    current = await current.getDirectoryHandle(part);
  }
  return current;
}

/** @param error */
function showError(error: unknown) {
  elements.status.textContent = error instanceof Error ? error.message : String(error);
  elements.status.dataset.error = 'true';
}

/** @param id @param constructor */
function requireElement<T extends HTMLElement>(id: string, constructor: {
    new (): T;
}) {
  const value = document.getElementById(id);
  if(!(value instanceof constructor)) {
    throw new Error(`Missing #${id}.`);
  }
  return value;
}

/** @param value */
function escapeText(value: string) {
  const node = document.createElement('span');
  node.textContent = value;
  return node.innerHTML;
}
