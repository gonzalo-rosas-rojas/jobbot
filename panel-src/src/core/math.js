// The two small numeric rules written once. clamp: scene/engine.js, scene/poses.js, stage/sky.js, stage/skyline.js and the volume of
// sound/index.js. wrapHour: scene/clock.js, scene/poses.js, stage/stage.js, stage/sky.js and stage/dial.js.

/** `x` held between `a` and `b` (the unit interval when they are not given). */
export const clamp = (x, a = 0, b = 1) => Math.min(b, Math.max(a, x));

/** An hour of the day (14.5 is 14:30), or any number of hours, put on the 24-hour ring: [0, 24), also for negative values. */
export const wrapHour = (hours) => ((hours % 24) + 24) % 24;
