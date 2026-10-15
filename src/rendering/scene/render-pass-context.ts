// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type {
  Framebuffer,
  RenderPass
} from '@luma.gl/core';

/**
 * Pass-local column-major transforms and clip distances in world units.
 */
export interface PassCamera {
  readonly viewMatrix: readonly number[];
  readonly projectionMatrix: readonly number[];
  readonly viewProjectionMatrix: readonly number[];
  readonly nearPlane: number;
  readonly farPlane: number;
}

/**
 * Borrowed GPU handles and camera overrides for one recorded pass. This context owns no
 * resources; the caller ends the pass, and the target owner destroys its attachments.
 * Shadow cameras and intermediate targets never replace any part of the shared FrameView.
 * A null camera supports low-level callers that supply only custom packed shader uniforms.
 */
export interface RenderPassContext {
  readonly camera: PassCamera | null;
  readonly target: Framebuffer;
  readonly viewport: Readonly<{
    width: number;
    height: number;
  }>;
  readonly renderPass: RenderPass;
}

/**
 * Wrap a borrowed pass without cloning a FrameView or freezing mutable GPU resources.
 * @param renderPass Borrowed active pass; its caller remains responsible for ending it.
 * @param target Borrowed framebuffer whose physical dimensions define the pass viewport.
 * @param camera Borrowed immutable camera, or null for callers using packed shader uniforms.
 * @returns An owned frozen context borrowing the pass, framebuffer, and camera.
 */
export function createRenderPassContext(
  renderPass: RenderPass,
  target: Framebuffer,
  camera: PassCamera | null = null): RenderPassContext {
  return Object.freeze({
    camera,
    target,
    viewport: Object.freeze({
      width: target.width,
      height: target.height
    }),
    renderPass
  });
}
