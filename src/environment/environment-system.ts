// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

const DEFAULT_OPTIONS = Object.freeze({
  timeOfDay: 0.35,
  dayLengthSeconds: 1200,
  latitudeDegrees: 42,
  axialTiltDegrees: 23.44,
  dayOfYear: 172,
  northRadians: 0,
  timeScale: 1,
  wind: Object.freeze({ directionRadians: 0, speed: 4, gustStrength: 0.25, gustPeriodSeconds: 8 }),
  atmosphere: Object.freeze({ rayleigh: 1, mie: 1, ozone: 1, density: 1, fogDensity: 0.002 }),
  clouds: Object.freeze({ coverage: 0.45, density: 0.7, altitude: 1800, thickness: 700, shadowStrength: 0.45 }),
  lightShafts: Object.freeze({ intensity: 0.65, decay: 0.94, exposure: 0.2 }),
  ambientOcclusion: Object.freeze({ intensity: 1, radius: 7 })
});

export type WindOptions = {
  directionRadians?: number;
  speed?: number;
  gustStrength?: number;
  gustPeriodSeconds?: number;
};
export type AtmosphereOptions = {
  rayleigh?: number;
  mie?: number;
  ozone?: number;
  density?: number;
  fogDensity?: number;
};
export type CloudOptions = {
  coverage?: number;
  density?: number;
  altitude?: number;
  thickness?: number;
  shadowStrength?: number;
};
export type LightShaftOptions = {
  intensity?: number;
  decay?: number;
  exposure?: number;
};
export type AmbientOcclusionOptions = {
  intensity?: number;
  radius?: number;
};
export type EnvironmentOptions = {
  timeOfDay?: number;
  dayLengthSeconds?: number;
  latitudeDegrees?: number;
  axialTiltDegrees?: number;
  dayOfYear?: number;
  northRadians?: number;
  timeScale?: number;
  wind?: WindOptions;
  atmosphere?: AtmosphereOptions;
  clouds?: CloudOptions;
  lightShafts?: LightShaftOptions;
  ambientOcclusion?: AmbientOcclusionOptions;
};
export type NormalizedEnvironmentOptions = {
  timeOfDay: number;
  dayLengthSeconds: number;
  latitudeDegrees: number;
  axialTiltDegrees: number;
  dayOfYear: number;
  northRadians: number;
  timeScale: number;
  wind: Required<WindOptions>;
  atmosphere: Required<AtmosphereOptions>;
  clouds: Required<CloudOptions>;
  lightShafts: Required<LightShaftOptions>;
  ambientOcclusion: Required<AmbientOcclusionOptions>;
};
export type AtmosphereLight = {
  direction: readonly [
      number,
      number,
      number
  ];
  color: readonly [
      number,
      number,
      number
  ];
  intensity: number;
  angularRadius: number;
  kind: 'sun' | 'moon';
};
export type EnvironmentFrame = {
  elapsedSeconds: number;
  timeOfDay: number;
  sun: AtmosphereLight;
  moon: AtmosphereLight;
  primaryLight: AtmosphereLight;
  skylight: Readonly<{
      color: readonly [
          number,
          number,
          number
      ];
      intensity: number;
  }>;
  wind: Readonly<{
      direction: readonly [
          number,
          number,
          number
      ];
      velocity: readonly [
          number,
          number,
          number
      ];
      speed: number;
  }>;
  atmosphere: Readonly<Required<AtmosphereOptions>>;
  clouds: Readonly<Required<CloudOptions>>;
  lightShafts: Readonly<Required<LightShaftOptions>>;
  ambientOcclusion: Readonly<Required<AmbientOcclusionOptions>>;
};

/**
 * Advance deterministic world environment state independently from rendering resources.
 * Time values are seconds, directions use a Y-up world space, and colors are linear RGB.
 * The instance owns mutable clock state; returned frames are immutable snapshots.
 */
export class EnvironmentSystem {
  declare configuration: Readonly<NormalizedEnvironmentOptions>;
  declare elapsedSeconds: number;
  declare timeOfDay: number;

  /**
   * Validate and copy environment policy into an independently owned simulation.
   * @param [options]
   */
  constructor(options: EnvironmentOptions = {}) {
    assertRecord(options, 'Environment options');
    assertKnownKeys(options, ['timeOfDay', 'dayLengthSeconds', 'latitudeDegrees', 'axialTiltDegrees', 'dayOfYear', 'northRadians', 'timeScale', 'wind', 'atmosphere', 'clouds', 'lightShafts', 'ambientOcclusion']);
    this.configuration = normalizeOptions(options);
    this.elapsedSeconds = 0;
    this.timeOfDay = wrapDay(this.configuration.timeOfDay);
  }

  /**
   * Advance the environment clock by real elapsed seconds and return its new frame.
   * @param deltaSeconds Nonnegative real elapsed time in seconds.
   */
  step(deltaSeconds: number): Readonly<EnvironmentFrame> {
    assertFiniteRange(deltaSeconds, 0, Infinity, 'deltaSeconds');
    this.elapsedSeconds+= deltaSeconds;
    if(this.configuration.dayLengthSeconds > 0) {
      this.timeOfDay = wrapDay(this.timeOfDay + (deltaSeconds*this.configuration.timeScale)/this.configuration.dayLengthSeconds);
    }
    return this.sample();
  }

  /**
   * Set normalized local solar time without changing accumulated wind time.
   * @param timeOfDay Normalized time in the half-open [0, 1) day interval.
   */
  setTimeOfDay(timeOfDay: number) {
    assertFiniteRange(timeOfDay, 0, 1, 'timeOfDay');
    if(timeOfDay === 1) {
      throw new RangeError('timeOfDay must be less than 1.');
    }
    this.timeOfDay = timeOfDay;
  }

  /**
   * Produce an immutable environment frame without advancing simulation.
   */
  sample(): Readonly<EnvironmentFrame> {
    const sunDirection = calculateSunDirection(this.timeOfDay, this.configuration);
    const sunHeight = sunDirection[1];
    const daylight = smoothstep(-0.08, 0.12, sunHeight);
    const twilight = smoothstep(-0.2, 0.03, sunHeight)*(1 - smoothstep(0.03, 0.35, sunHeight));
    const sun = createLight('sun', sunDirection, mixColor([1.35, 0.35, 0.08], [1, 0.96, 0.86], daylight), daylight*5, 0.00465);
    const moonDirection = ((sunDirection.map(value => -value)) as [
    number,
    number,
    number
]);
    const moon = createLight('moon', moonDirection, [0.24, 0.32, 0.55], (1 - daylight)*0.12, 0.00436);
    const primaryLight = daylight >= 0.05 ? sun : moon;
    const wind = calculateWind(this.elapsedSeconds, this.configuration.wind);
    const skyColor = addColor(
      mixColor([0.008, 0.012, 0.035], [0.2, 0.42, 0.8], daylight),
      scaleColor([0.8, 0.16, 0.035], twilight*0.35)
    );
    return Object.freeze({
      elapsedSeconds: this.elapsedSeconds,
      timeOfDay: this.timeOfDay,
      sun,
      moon,
      primaryLight,
      skylight: Object.freeze({color: Object.freeze(skyColor), intensity: 0.03 + daylight*0.42}),
      wind,
      atmosphere: this.configuration.atmosphere,
      clouds: this.configuration.clouds,
      lightShafts: this.configuration.lightShafts,
      ambientOcclusion: this.configuration.ambientOcclusion
    });
  }
}

/** @param options */
function normalizeOptions(options: EnvironmentOptions): Readonly<NormalizedEnvironmentOptions> {
  const wind = ((mergeRecord(DEFAULT_OPTIONS.wind, options.wind, 'wind')) as Required<WindOptions>);
  const atmosphere = ((mergeRecord(DEFAULT_OPTIONS.atmosphere, options.atmosphere, 'atmosphere')) as Required<AtmosphereOptions>);
  const clouds = ((mergeRecord(DEFAULT_OPTIONS.clouds, options.clouds, 'clouds')) as Required<CloudOptions>);
  const lightShafts = ((mergeRecord(DEFAULT_OPTIONS.lightShafts, options.lightShafts, 'lightShafts')) as Required<LightShaftOptions>);
  const ambientOcclusion = ((mergeRecord(DEFAULT_OPTIONS.ambientOcclusion, options.ambientOcclusion, 'ambientOcclusion')) as Required<AmbientOcclusionOptions>);
  const result = { ...DEFAULT_OPTIONS, ...options, wind, atmosphere, clouds, lightShafts, ambientOcclusion };
  assertFiniteRange(result.timeOfDay, 0, 1, 'timeOfDay');
  if(result.timeOfDay === 1) {
    throw new RangeError('timeOfDay must be less than 1.');
  }
  assertFiniteRange(result.dayLengthSeconds, 0, Infinity, 'dayLengthSeconds');
  assertFiniteRange(result.latitudeDegrees, -90, 90, 'latitudeDegrees');
  assertFiniteRange(result.axialTiltDegrees, -45, 45, 'axialTiltDegrees');
  assertFiniteRange(result.dayOfYear, 1, 366, 'dayOfYear');
  assertFiniteRange(result.northRadians, -Infinity, Infinity, 'northRadians');
  assertFiniteRange(result.timeScale, 0, Infinity, 'timeScale');
  validateRanges(result);
  return  ((Object.freeze(result)) as Readonly<NormalizedEnvironmentOptions>);
}

/** @param defaults @param supplied @param name */
function mergeRecord(defaults: Readonly<Record<string, number>>, supplied: Record<string, number> | undefined, name: string) {
  if(supplied === undefined) {
    return defaults;
  }
  assertRecord(supplied, name);
  assertKnownKeys(supplied, Object.keys(defaults));
  return Object.freeze({...defaults, ...supplied});
}

/** @param options */
function validateRanges(options: Readonly<NormalizedEnvironmentOptions>) {
  assertFiniteRange(options.wind.speed, 0, Infinity, 'wind.speed');
  assertFiniteRange(options.wind.directionRadians, -Infinity, Infinity, 'wind.directionRadians');
  assertFiniteRange(options.wind.gustStrength, 0, 1, 'wind.gustStrength');
  assertFiniteRange(options.wind.gustPeriodSeconds, Number.MIN_VALUE, Infinity, 'wind.gustPeriodSeconds');
  for(const name of  ((['rayleigh', 'mie', 'ozone', 'density', 'fogDensity']) as (keyof AtmosphereOptions)[])) {
    assertFiniteRange(options.atmosphere[name], 0, Infinity, `atmosphere.${name}`);
  }
  for(const name of  ((['coverage', 'density', 'shadowStrength']) as (keyof CloudOptions)[])) {
    assertFiniteRange(options.clouds[name], 0, 1, `clouds.${name}`);
  }
  assertFiniteRange(options.clouds.altitude, 0, Infinity, 'clouds.altitude');
  assertFiniteRange(options.clouds.thickness, Number.MIN_VALUE, Infinity, 'clouds.thickness');
  assertFiniteRange(options.lightShafts.intensity, 0, Infinity, 'lightShafts.intensity');
  assertFiniteRange(options.lightShafts.decay, 0, 1, 'lightShafts.decay');
  assertFiniteRange(options.lightShafts.exposure, 0, Infinity, 'lightShafts.exposure');
  assertFiniteRange(options.ambientOcclusion.intensity, 0, Infinity, 'ambientOcclusion.intensity');
  assertFiniteRange(options.ambientOcclusion.radius, Number.MIN_VALUE, Infinity, 'ambientOcclusion.radius');
}

/** @param timeOfDay @param options */
function calculateSunDirection(timeOfDay: number, options: Readonly<NormalizedEnvironmentOptions>): [
    number,
    number,
    number
] {
  const latitude = options.latitudeDegrees*Math.PI/180;
  const declination = options.axialTiltDegrees*Math.PI/180*Math.sin((2*Math.PI*(options.dayOfYear - 81))/365);
  const hourAngle = (timeOfDay - 0.5)*2*Math.PI;
  const elevation = Math.asin(Math.sin(latitude)*Math.sin(declination) + Math.cos(latitude)*Math.cos(declination)*Math.cos(hourAngle));
  const azimuth = Math.atan2(-Math.sin(hourAngle), Math.tan(declination)*Math.cos(latitude) - Math.sin(latitude)*Math.cos(hourAngle)) + options.northRadians;
  const horizontal = Math.cos(elevation);
  return [Math.sin(azimuth)*horizontal, Math.sin(elevation), Math.cos(azimuth)*horizontal];
}

/** @param elapsedSeconds @param options */
function calculateWind(elapsedSeconds: number, options: Readonly<Required<WindOptions>>) {
  const phase = elapsedSeconds/options.gustPeriodSeconds;
  const gust = (Math.sin(phase*2*Math.PI) + Math.sin(phase*Math.PI*0.73 + 1.7)*0.5)/1.5;
  const speed = Math.max(0, options.speed*(1 + gust*options.gustStrength));
  const direction = (([Math.sin(options.directionRadians), 0, Math.cos(options.directionRadians)]) as [
    number,
    number,
    number
]);
  const velocity = ((direction.map(value => value*speed)) as [
    number,
    number,
    number
]);
  return Object.freeze({direction: Object.freeze(direction), velocity: Object.freeze(velocity), speed});
}

/** @param kind @param direction @param color @param intensity @param angularRadius */
function createLight(kind: 'sun' | 'moon', direction: [
    number,
    number,
    number
], color: [
    number,
    number,
    number
], intensity: number, angularRadius: number): AtmosphereLight {
  return Object.freeze({kind, direction: Object.freeze(direction), color: Object.freeze(color), intensity, angularRadius});
}

/** @param a @param b @param amount */
function mixColor(a: readonly [
    number,
    number,
    number
], b: readonly [
    number,
    number,
    number
], amount: number): [
    number,
    number,
    number
] {
  return [a[0] + (b[0] - a[0])*amount, a[1] + (b[1] - a[1])*amount, a[2] + (b[2] - a[2])*amount];
}

/** @param a @param b */
function addColor(a: readonly [
    number,
    number,
    number
], b: readonly [
    number,
    number,
    number
]): [
    number,
    number,
    number
] {
  return [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
}

/** @param color @param scale */
function scaleColor(color: readonly [
    number,
    number,
    number
], scale: number): [
    number,
    number,
    number
] {
  return [color[0]*scale, color[1]*scale, color[2]*scale];
}

/** @param minimum @param maximum @param value */
function smoothstep(minimum: number, maximum: number, value: number) {
  const normalized = Math.max(0, Math.min(1, (value - minimum)/(maximum - minimum)));
  return normalized*normalized*(3 - 2*normalized);
}

/** @param value */
function wrapDay(value: number) {
  return ((value%1) + 1)%1;
}

/** @param value @param name */
function assertRecord(value: unknown, name: string) {
  if(!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new TypeError(`${name} must be a record.`);
  }
}

/** @param value @param keys */
function assertKnownKeys(value: Record<string, unknown>, keys: string[]) {
  const unknown = Object.keys(value).find(key => !keys.includes(key));
  if(unknown) {
    throw new TypeError(`Unknown environment option: ${unknown}.`);
  }
}

/** @param value @param minimum @param maximum @param name */
function assertFiniteRange(value: number, minimum: number, maximum: number, name: string) {
  if(!Number.isFinite(value) || value < minimum || value > maximum) {
    throw new RangeError(`${name} must be finite and between ${minimum} and ${maximum}.`);
  }
}
