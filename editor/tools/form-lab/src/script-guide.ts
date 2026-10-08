// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { BROWSER_EVENT } from 'joy-engine/constants';
import type { EditorView } from '@codemirror/view';

/** Runnable starters use unique bindings and output names when appended to a study. */
export const RECIPES = [
  {
    title: 'Revolve a profile',
    description: 'Make a pot, tower, or turned prop from radius/height points. The profile can be inspected and edited like any geometry.',
    code: `let $id_profile = geometry()
addPoint($id_profile, vector(0,0,0))
addPoint($id_profile, vector(2,0,0))
addPoint($id_profile, vector(2.5,2,0))
addPoint($id_profile, vector(1,4,0))
let $id = revolve($id_profile, 32)
output("$id", $id)`
  },
  {
    title: 'Sweep a path',
    description: 'Generate a cable or pipe around ordered XYZ points. Returns ordinary geometry with smooth normals and corner UVs.',
    code: `let $id_path = geometry()
repeat 41 as i {
  let t = i/40*tau
  addPoint($id_path, vector(cos(t)*3, i*0.12, sin(t)*3))
}
let $id = tube($id_path, 0.3, 12)
output("$id", $id)`
  },
  {
    title: 'Surface + attribute field',
    description: 'A reusable grid, noise displacement, and a custom density attribute. Try density in the viewport’s attribute view.',
    code: `let $id = grid(24, 24, 12, 12)
addAttribute($id, "points", "density", 0)
color($id, rgba(1,1,1,1))
wrangle points in $id as element {
  let h = noise(element.position.x*0.3, element.position.z*0.3, 7777)
  element.position = element.position + vector(0, h*3, 0)
  element.density = smoothstep(-0.5, 0.5, h)
  element.color = mix(rgba(0.12,0.28,0.31,1), rgba(0.5,0.7,0.5,1), element.density)
}
output("$id", $id)`
  },
  {
    title: 'Build topology from scratch',
    description: 'Create empty geometry, add points, and connect a face. Face winding controls the surface normal.',
    code: `let $id = geometry()
let $id_a = addPoint($id, vector(-2,0,-2))
let $id_b = addPoint($id, vector(2,0,-2))
let $id_c = addPoint($id, vector(0,0,2))
addFace($id, $id_a, $id_c, $id_b)
addAttribute($id, "faces", "region", 0)
wrangle faces in $id as element { element.region = element.index }
output("$id", $id)`
  },
  {
    title: 'Assembly from generated points',
    description: 'A ring of logical instances. Change the source mesh, placement formula, or scale attributes independently.',
    code: `let $id = geometry()
repeat 24 as i {
  let angle = i/24*tau
  addPoint($id, vector(cos(angle)*6, 0, sin(angle)*6))
}
addAttribute($id, "points", "rotation", vector(0,0,0))
addAttribute($id, "points", "scale", vector(1,1,1))
wrangle points in $id as element {
  element.rotation = vector(0, -element.index/24*tau, 0)
  element.scale = vector(1, 1 + sin(element.index*0.7)*0.5, 1)
}
let $id_assembly = instances(meshBox(0.5,2,0.5), $id,
  materialPbr(rgba(0.93,0.42,0.22,1), 0.2, 0.5))
output("$id", $id_assembly)`
  },
  {
    title: 'Shape a primitive',
    description: 'Start from a sphere, deform it with a field, and flatten its base. A small building block for procedural rock and terrain studies.',
    code: `let $id = sphere(2, 16)
scale($id, vector(1.2, 0.8, 1))
rotate($id, vector(0, 0.25, 0))
color($id, rgba(1,1,1,1))
wrangle points in $id as element {
  let n = noise(element.position.x*0.8, element.position.y*0.8, element.position.z*0.8 + 7777)
  element.position = element.position*(1 + n*0.3)
  element.position = vector(element.position.x, max(-1.3, element.position.y), element.position.z)
  element.color = mix(rgba(0.16,0.22,0.28,1), rgba(0.5,0.56,0.62,1), (n + 1)/2)
}
output("$id", $id)`
  }
];

/** Append complete recipes without replacing source or touching runtime parameters.
 * @param parent @param editor
 * @param dialog @param signal
 */
export function connectScriptGuide(parent: HTMLElement, editor: EditorView, dialog: HTMLDialogElement, signal: AbortSignal) {
  const label = document.createElement('label');
  label.className = 'guide-search';
  label.textContent = 'FIND A STARTING POINT';
  const search = document.createElement('input');
  search.type = 'search';
  search.placeholder = 'Search recipes: surface, attributes, topology, assembly…';
  search.setAttribute('aria-label', 'Search procedural recipes');
  label.append(search);
  const list = document.createElement('div');
  list.className = 'recipe-grid';
  const cards = RECIPES.map(recipe => {
    const card = document.createElement('section'), heading = document.createElement('h3');
    heading.textContent = recipe.title;
    const description = document.createElement('p');
    description.textContent = recipe.description;
    const code = document.createElement('pre');
    code.textContent = recipe.code.replaceAll('$id', 'study');
    const button = document.createElement('button');
    button.textContent = 'Append recipe ↗';
    button.setAttribute('aria-label', `Append ${recipe.title}`);
    button.addEventListener(BROWSER_EVENT.CLICK, () => {
      const source = editor.state.doc.toString();
      let serial = 1;
      while(source.includes(`recipe_${serial}`)) {
        serial+= 1;
      }
      const addition = `\n\n# ${recipe.title.toUpperCase()}\n${recipe.code.replaceAll('$id', `recipe_${serial}`)}\n`;
      editor.dispatch({changes: {from: source.length, insert: addition}, selection: {anchor: source.length + 2}, scrollIntoView: true});
      dialog.close();
      editor.focus();
    }, {signal});
    card.append(heading, description, code, button);
    list.append(card);
    return {card, search: `${recipe.title} ${recipe.description} ${recipe.code}`.toLowerCase()};
  });
  const empty = document.createElement('p');
  empty.textContent = 'No matching recipes. Try geometry, field, or assembly.';
  empty.hidden = true;
  search.addEventListener(BROWSER_EVENT.INPUT, () => {
    const query = search.value.trim().toLowerCase();
    let shown = 0;
    for(const entry of cards) {
      entry.card.hidden = !entry.search.includes(query);
      shown+= Number(!entry.card.hidden);
    }
    empty.hidden = shown > 0;
  }, {signal});
  parent.append(label, list, empty);
}
