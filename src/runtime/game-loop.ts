// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

export interface GameLoopOptions {
  update: (seconds: number) => void;
  render: (timeMilliseconds: number, interpolation: number) => void;
  updatesPerSecond?: number;
}

const DEFAULT_UPDATE_RATE = 60;
const MAX_FRAME_TIME_SECONDS = 0.25;

/**
 * Run simulation at a fixed rate and render once per animation frame.
 * Update receives elapsed seconds; render receives the frame timestamp in milliseconds and the remaining fraction of a simulation step for interpolation.
 * @param options Configuration options for the game loop.
 * @returns A function to stop the game loop.
 */
export function startGameLoop({ update, render, updatesPerSecond = DEFAULT_UPDATE_RATE }: GameLoopOptions): () => void {
  const fixedStepSeconds = 1/updatesPerSecond;
  let previousTime = performance.now();
  let accumulatedTime = 0;
  let animationFrameId = 0;

  function frame(currentTime: number): void {
    const elapsedSeconds = Math.min((currentTime - previousTime)/1000, MAX_FRAME_TIME_SECONDS);

    previousTime = currentTime;
    accumulatedTime+= elapsedSeconds;

    while(accumulatedTime >= fixedStepSeconds) {
      update(fixedStepSeconds);
      accumulatedTime-= fixedStepSeconds;
    }

    render(currentTime, accumulatedTime/fixedStepSeconds);
    animationFrameId = requestAnimationFrame(frame);
  }

  animationFrameId = requestAnimationFrame(frame);

  return () => cancelAnimationFrame(animationFrameId);
}
