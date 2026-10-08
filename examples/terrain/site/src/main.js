
import { BROWSER_EVENT, GPU_BACKEND, GPU_TEXTURE_FORMAT, KEY_CODE } from 'joy-engine/constants';
// Copyright (c) 2026 Trent Polack. Licensed under the MIT License.
import {
  createGpuCanvasDevice,
  gpuBackendFromQuery,
  loadTerrain,
  sampleTerrain,
  TerrainRenderer,
  OrbitCamera,
  requireElement
} from 'joy-engine';
import { TerrainCameraControls } from '../../src/terrain-camera-controls.js';
import { TERRAIN_CONFIG } from '../../src/config/terrain-config.js';
const canvas = requireElement(document,'#world',HTMLCanvasElement);
const stats = requireElement(document,'#stats',HTMLElement);
const toleranceInput = requireElement(document,'#tolerance',HTMLInputElement);
const diagnosticInput = requireElement(document,'#diagnostic',HTMLSelectElement);
const pinInput = requireElement(document,'#pin',HTMLInputElement);
const tourInput = requireElement(document,'#tour',HTMLInputElement);
const camera = new OrbitCamera();
const events = new AbortController();
const controls = new TerrainCameraControls(canvas,camera,TERRAIN_CONFIG.cameraControls);
const CAMERA_PRESET_KEYS = Object.freeze({ground:KEY_CODE.KEY_G,overview:KEY_CODE.KEY_O});
/** @type {import('@luma.gl/core').Device|null} */
let device = null;
/** @type {TerrainRenderer|null} */
let renderer = null;
/** @type {Awaited<ReturnType<typeof loadTerrain>>|null} */
let asset = null;
let disposed = false, frameRequest = 0, previousTime = 0;
let cpuDrawMilliseconds = 0, frameCount = 0;

// Presets remain available outside flight; text/config controls retain keyboard focus.
window.addEventListener(BROWSER_EVENT.KEYDOWN,event => {
  if(event.target instanceof HTMLElement && event.target.closest('aside')) { return; }
  if(event.code === CAMERA_PRESET_KEYS.ground) { ground(); }
  if(event.code === CAMERA_PRESET_KEYS.overview) { overview(); }
},{signal:events.signal});
window.addEventListener(BROWSER_EVENT.PAGEHIDE,event => { if(!event.persisted) { dispose(); } },{signal:events.signal});
requireElement(document,'#ground',HTMLButtonElement).addEventListener(BROWSER_EVENT.CLICK,ground,{signal:events.signal});
requireElement(document,'#overview',HTMLButtonElement).addEventListener(BROWSER_EVENT.CLICK,overview,{signal:events.signal});
if(import.meta.hot) { import.meta.hot.dispose(dispose); }
void initialize();

async function initialize() {
  try {
    asset = await loadTerrain(new URL(TERRAIN_CONFIG.manifest,document.baseURI));
    device = await createGpuCanvasDevice(canvas,{backend:gpuBackendFromQuery(location.search),optionalFeatures:['timestamp-query'],debug:false});
    if(disposed) { device.destroy(); return; }
    renderer = new TerrainRenderer(device,asset);
    stats.textContent = 'Validating terrain shaders…';
    if(import.meta.env.DEV) { console.info('Terrain shaders created; awaiting validation.'); }
    await renderer.ready();
    if(disposed) { return; }
    overview();
    document.body.dataset.backend = device.type;
    document.body.dataset.ready = 'true';
    if(import.meta.env.DEV) { console.info(`Terrain Lab ready: ${device.type}; ${asset.patches.length} resident patches.`); }
    frameRequest = requestAnimationFrame(frame);
  } catch(error) {
    const failure = requireElement(document,'#failure',HTMLElement);
    failure.hidden = false; failure.textContent = `Terrain startup failed\n${String(error)}`;
    dispose();
  }
}

/** @param {number} timestampMilliseconds */
function frame(timestampMilliseconds) {
  if(!device || !renderer || !asset || disposed) { return; }
  const seconds = Math.min(0.05,(timestampMilliseconds - previousTime)/1000 || 0);
  previousTime = timestampMilliseconds;
  controls.update(seconds);
  // Flight owns orientation while held; the automatic tour resumes on release.
  if(tourInput.checked && !controls.flying) { camera.yaw+= seconds*0.2; }
  const ratio = Math.min(TERRAIN_CONFIG.maxPixelRatio,devicePixelRatio);
  const width = Math.round(innerWidth*ratio), height = Math.round(innerHeight*ratio);
  if(canvas.width !== width || canvas.height !== height) {
    device.canvasContext?.setDrawingBufferSize(width,height);
  }
  const view = {eye:camera.basis().eye,projectionScale:innerHeight/(2*Math.tan(TERRAIN_CONFIG.cameraControls.verticalFovRadians/2)),tolerance:Number(toleranceInput.value),target:camera.target,targetRadius:pinInput.checked ? TERRAIN_CONFIG.targetRadius : 0};
  renderer.prepare(camera.matrix(innerWidth/innerHeight,device.type === GPU_BACKEND.WEBGPU),view,Number(diagnosticInput.value));
  const start = performance.now();
  const pass = device.beginRenderPass({framebuffer:device.canvasContext?.getCurrentFramebuffer({depthStencilFormat:GPU_TEXTURE_FORMAT.DEPTH24PLUS}),clearColor:TERRAIN_CONFIG.backgroundColor,clearDepth:1,...renderer.timing.begin(timestampMilliseconds)});
  renderer.draw(pass); pass.end(); device.submit();
  void renderer.timing.end();
  cpuDrawMilliseconds = performance.now() - start;
  frameCount+= 1;
  if(frameCount === 1 || frameCount % 15 === 0) {
    const s = renderer.stats;
    stats.textContent = `${device.type.toUpperCase()} · ${s.patches} patches · ${s.triangles.toLocaleString()} triangles (${s.triangleCountKind})\nCPU prepare ${s.cpuMilliseconds.toFixed(2)} ms · submit ${cpuDrawMilliseconds.toFixed(2)} ms\nGPU ${s.gpuMilliseconds === null ? s.gpuTiming : s.gpuMilliseconds.toFixed(2) + ' ms render pass'} · GPU resident ${(s.residentGpuBytes/1048576).toFixed(2)} MiB\nCPU resident ${(s.residentCpuBytes/1048576).toFixed(2)} MiB · frame upload ${s.frameUploadBytes.toLocaleString()} B`;
  }
  frameRequest = requestAnimationFrame(frame);
}
function overview() {
  controls.stopFlight();
  camera.target = [...TERRAIN_CONFIG.target]; camera.radius = 1500;
  if(asset) { camera.target[1] = sampleTerrain(asset,camera.target[0],camera.target[2]) + 12; }
  camera.distance = TERRAIN_CONFIG.orbitDistance; camera.pitch = TERRAIN_CONFIG.orbitPitch; camera.yaw = TERRAIN_CONFIG.orbitYaw;
}
function ground() { controls.stopFlight(); camera.distance = TERRAIN_CONFIG.nearGroundDistance; camera.pitch = TERRAIN_CONFIG.nearGroundPitch; }
function dispose() {
  if(disposed) { return; }
  disposed = true; cancelAnimationFrame(frameRequest); events.abort();
  controls.destroy();
  renderer?.destroy(); device?.destroy(); renderer = null; device = null;
}
