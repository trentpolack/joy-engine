// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { GeometryData } from 'joy-engine/form';

export type { GeometryData };

/** Preserve indexed faces and independent points in Wavefront OBJ. @param data */
export function exportObj(data: GeometryData) {
  const lines = ['# FORM LAB — Y-up, script units', 'o FormLab'];
  for(let i = 0; i < data.positions.length; i += 3) {
    lines.push(`v ${data.positions.slice(i, i + 3).join(' ')}`);
  }
  for(let i = 0; i < data.triangles.length; i += 3) {
    lines.push(
      `f ${data.triangles
        .slice(i, i + 3)
        .map((index) => index + 1)
        .join(' ')}`,
    );
  }
  for(const index of data.points) {
    lines.push(`p ${index + 1}`);
  }
  return lines.join('\n') + '\n';
}
/** ASCII PLY stores RGBA and faces; unreferenced vertices remain point samples. @param data */
export function exportPly(data: GeometryData) {
  const lines = [
    'ply',
    'format ascii 1.0',
    'comment FORM LAB — Y-up, script units',
    `element vertex ${data.positions.length / 3}`,
    'property float x',
    'property float y',
    'property float z',
    'property uchar red',
    'property uchar green',
    'property uchar blue',
    'property uchar alpha',
    `element face ${data.triangles.length / 3}`,
    'property list uchar int vertex_indices',
    'end_header',
  ];
  for(let i = 0; i < data.positions.length; i += 3) {
    lines.push(
      [
        ...data.positions.slice(i, i + 3),
        ...data.colors.slice(i, i + 3).map((value) => Math.round(value * 255)),
        Math.round(data.alphas[i / 3] * 255),
      ].join(' '),
    );
  }
  for(let i = 0; i < data.triangles.length; i += 3) {
    lines.push(`3 ${data.triangles.slice(i, i + 3).join(' ')}`);
  }
  return lines.join('\n') + '\n';
}

