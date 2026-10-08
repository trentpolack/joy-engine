// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { FORM_ATTRIBUTE, FORM_ATTRIBUTE_DOMAIN } from './constants.ts';
import type { GeometryValue, GeometryStore, AttributeValue } from './procedural-geometry.ts';
import { createSeededRandom } from '../core/random.ts';
import { integerRange } from './geometry.ts';
import { attributeAt } from './procedural-geometry.ts';
export type { GeometryValue };

/** Sample triangle area times a linearly interpolated nonnegative point weight.
 * Seed is local: sampling never changes the surrounding script's random stream.
 * @param store
 * @param source @param amount @param seed @param weight
 */
export function scatterGeometry(store: GeometryStore, source: GeometryValue, amount: number, seed: number, weight: string) {
  integerRange(amount, 0, 150000);
  const positions = source.attributes.points.position.values;
  const weights = weight ? source.attributes.points[weight] : null;
  if(weight && (!weights || weights.size !== 1 || weights.values.some(value => typeof value !== 'number' || value < 0))) {
    throw new Error('Scatter weight must name a nonnegative scalar point attribute.');
  }
  const result = store.create();
  const random = createSeededRandom(seed);
  let total = 0;
  const cumulative = source.faces.map(face => {
    const [a, b, c] = face.map(index =>  ((positions[index]) as number[]));
    const normal = cross(a, b, c);
    const density = weights ? face.reduce((sum, index) => sum +  ((weights.values[index]) as number), 0)/3 : 1;
    total+= Math.hypot(...normal)*0.5*density;
    if(!Number.isFinite(total)) {
      throw new Error('Scatter area/weight overflow.');
    }
    return total;
  });
  if(total === 0) {
    return result;
  }
  for(let i = 0; i < amount; i++) {
    const target = random()*total;
    let low = 0, high = cumulative.length - 1;
    while(low < high) {
      const middle = Math.floor((low + high)/2);
      if(cumulative[middle] <= target) {
        low = middle + 1;
      } else {
        high = middle;
      }
    }
    const face = source.faces[low];
    // A linear density is a mixture of barycentric coordinates. Sampling the
    // chosen component as Gamma(2), others Gamma(1), avoids rejection loops.
    const densities = face.map(index => weights ?  ((weights.values[index]) as number) : 1);
    const choice = random()*densities.reduce((a, b) => a + b, 0);
    const selected = choice < densities[0] ? 0 : choice < densities[0] + densities[1] ? 1 : 2;
    const barycentric = face.map((_, corner) => -Math.log(Math.max(Number.MIN_VALUE, random())) +
      (corner === selected ? -Math.log(Math.max(Number.MIN_VALUE, random())) : 0));
    const sum = barycentric.reduce((a, b) => a + b, 0);
    const blend = barycentric.map(value => value/sum);
    /** @param name */
    const interpolate = (name: string) : AttributeValue => {
      const values = face.map(index => attributeAt(source, FORM_ATTRIBUTE_DOMAIN.POINTS, name, index));
      if(typeof values[0] === 'number') {
        return values.map(value =>  ((value) as number)).reduce((sum, value, corner) => sum + value*blend[corner], 0);
      }
      return values[0].map((_, axis) => values.map(value =>  ((value) as number[])[axis]).reduce((sum, value, corner) => sum + value*blend[corner], 0));
    };
    const index = store.addPoint(result,  ((interpolate(FORM_ATTRIBUTE.POSITION)) as number[]));
    for(const name of Object.keys(source.attributes.points)) {
      if(name !== FORM_ATTRIBUTE.POSITION && name !== FORM_ATTRIBUTE.NORMAL) {
        const value = interpolate(name);
        // Convex interpolation can round alpha=1 a fraction above one.
        const bounded = name === FORM_ATTRIBUTE.COLOR && Array.isArray(value) ? value.map(component => Math.max(0, Math.min(1, component))) : value;
        store.set(result, FORM_ATTRIBUTE_DOMAIN.POINTS, name, index, bounded);
      }
    }
    const [a, b, c] = face.map(index =>  ((positions[index]) as number[]));
    const normal = cross(a, b, c), length = Math.hypot(...normal);
    store.set(result, FORM_ATTRIBUTE_DOMAIN.POINTS, FORM_ATTRIBUTE.NORMAL, index, normal.map(value => value/length));
  }
  return result;
}
/** @param a @param b @param c */
function cross(a: number[], b: number[], c: number[]) {
  const u = b.map((v, i) => v - a[i]), v = c.map((x, i) => x - a[i]);
  return [u[1]*v[2] - u[2]*v[1], u[2]*v[0] - u[0]*v[2], u[0]*v[1] - u[1]*v[0]];
}
