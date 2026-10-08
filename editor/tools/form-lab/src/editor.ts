// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { FORM_ATTRIBUTE_DOMAIN, FORM_COMMAND, FORM_KEYWORD } from 'joy-engine/constants';
import type { StringStream } from '@codemirror/language';
import { createScriptEditor, StreamLanguage } from '@joy-games/joy-editor';

/** Each parser checkpoint owns its declaration set; edits rebuild subsequent highlighting. */
export const formLanguage = StreamLanguage.define({

  startState(): {
    parameters: Set<string>;
    declaringParameter: boolean;
    component: boolean;
} {
    return {
      parameters: new Set(),
      declaringParameter: false,
      component: false,
    };
  },
  copyState(state: { parameters: Set<string>; declaringParameter: boolean; component: boolean; }) {
    return { ...state, parameters: new Set(state.parameters) };
  },
  token(stream: StringStream, state: { parameters: Set<string>; declaringParameter: boolean; component: boolean; }) {
    if(stream.eatSpace()) {
      return null;
    }
    if(stream.match(/#.*|\/\/.*/)) {
      return 'comment';
    }
    if(stream.match(/"(?:[^"\\]|\\.)*(?:"|$)/)) {
      return 'string';
    }
    if(stream.match(/(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?/)) {
      return 'number';
    }
    if(stream.match(/@[A-Za-z_][A-Za-z_0-9]*/)) {
      return 'invalid';
    }
    const word = stream.match(/[A-Za-z_][A-Za-z_0-9]*/);
    if(word && typeof word !== 'boolean') {
      const name = word[0];
      if(state.component) {
        state.component = false;
        return 'propertyName';
      }
      if(state.declaringParameter) {
        state.parameters.add(name);
        state.declaringParameter = false;
        return 'variableName.special';
      }
      if(['true', 'false', 'null'].includes(name)) {
        return 'bool';
      }
      if(name === FORM_KEYWORD.PARAM) {
        state.declaringParameter = true;
        return 'keyword';
      }
      if(([
        FORM_KEYWORD.PROJECT,
        FORM_KEYWORD.META,
        FORM_KEYWORD.LET,
        FORM_KEYWORD.REPEAT,
        FORM_KEYWORD.AS,
        FORM_KEYWORD.IN,
        FORM_KEYWORD.WRANGLE,
        FORM_KEYWORD.IF,
        FORM_KEYWORD.WITH,
        FORM_ATTRIBUTE_DOMAIN.POINTS,
        FORM_ATTRIBUTE_DOMAIN.FACES,
        FORM_ATTRIBUTE_DOMAIN.CORNERS,
        FORM_ATTRIBUTE_DOMAIN.INSTANCES
      ] as readonly string[]).includes(name)) {
        return 'keyword';
      }
      if(state.parameters.has(name)) {
        return 'variableName.special';
      }
      if(
        ([
          FORM_COMMAND.POINT,
          FORM_COMMAND.BOX,
          FORM_COMMAND.SPHERE,
          FORM_COMMAND.TRIANGLE,
          FORM_COMMAND.SURFACE,
          FORM_COMMAND.COLOR,
          FORM_KEYWORD.USER_DATA,
          FORM_COMMAND.SET_USER_DATA,
          FORM_COMMAND.SEED,
          FORM_COMMAND.VECTOR,
          FORM_COMMAND.RGBA,
          FORM_COMMAND.UV,
          FORM_COMMAND.MATERIAL,
          FORM_COMMAND.NORMAL,
          FORM_COMMAND.MESH_BOX,
          FORM_COMMAND.MESH_SPHERE,
          FORM_COMMAND.MATERIAL_PBR,
          FORM_COMMAND.TRANSFORM_MESH,
          FORM_COMMAND.DEFORM,
          FORM_COMMAND.REVOLVE, FORM_COMMAND.TUBE, FORM_COMMAND.SMOOTH_NORMALS, FORM_COMMAND.INSTANCE, FORM_COMMAND.GEOMETRY, FORM_COMMAND.GRID, FORM_COMMAND.COPY, FORM_COMMAND.TRANSLATE, FORM_COMMAND.ROTATE, FORM_COMMAND.SCALE, FORM_COMMAND.ADD_POINT, FORM_COMMAND.ADD_FACE, FORM_COMMAND.REMOVE_POINT, FORM_COMMAND.REMOVE_FACE,
          FORM_COMMAND.NOISE, FORM_COMMAND.SMOOTHSTEP, FORM_COMMAND.LENGTH, FORM_COMMAND.NORMALIZE, FORM_COMMAND.DOT, FORM_COMMAND.CROSS, FORM_COMMAND.MIX,
          FORM_COMMAND.POINT_COUNT, FORM_COMMAND.FACE_COUNT, FORM_COMMAND.ATTRIBUTE, FORM_COMMAND.ADD_ATTRIBUTE, FORM_COMMAND.SET_ATTRIBUTE, FORM_COMMAND.SCATTER, FORM_COMMAND.OUTPUT, FORM_COMMAND.INSPECT
        ] as readonly string[]).includes(name)
      ) {
        return 'typeName';
      }
      return 'variableName';
    }
    state.component = stream.next() === '.';
    return 'operator';
  },
});

/** @param parent @param source @param onChange @param onRun @param onCursor */
export function createEditor(parent: HTMLElement, source: string, onChange: () => void, onRun: () => void, onCursor: (line: number, column: number) => void) {
  return createScriptEditor({
    parent,
    source,
    language: formLanguage,
    label: 'Form script editor',
    onChange,
    onRun,
    onCursor,
  });
}
