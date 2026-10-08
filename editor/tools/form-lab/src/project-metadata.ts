// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { parse } from 'joy-engine/form';

/** Return a minimal editor change that makes the script own the document name.
 * Existing metadata formatting and other fields remain untouched. Scripts without
 * metadata get a declaration. Invalid syntax throws so the source is never guessed at.
 * @param source @param name
 */
export function projectNameChange(source: string, name: string): {
    from: number;
    to: number;
    insert: string;
} {
  const declaration = `project meta(name=${JSON.stringify(name)})\n`;
  parse(declaration);
  const projects = parse(source).filter(
    (statement) => statement.kind === 'project',
  );
  if(projects.length > 1) {
    throw new Error('Declare project metadata only once before renaming.');
  }
  const token = projects[0]?.projectNameToken;
  if(!token) {
    return { from: 0, to: 0, insert: declaration };
  }
  const lines = source.split('\n');
  let from = token.column - 1;
  for(let index = 0; index < token.line - 1; index++) {
    from += lines[index].length + 1;
  }
  return { from, to: from + token.text.length, insert: JSON.stringify(name) };
}

