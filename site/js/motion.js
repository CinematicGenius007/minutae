export const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)");

const tasks = new Set();
let frame = 0;
let previous = 0;

function tick(now) {
  // Capped, so nothing jumps ahead after a hidden tab.
  const dt = Math.min(Math.max(now - previous, 0), 100);
  previous = now;
  for (const task of tasks) {
    if (task(dt) === false) tasks.delete(task);
  }
  frame = tasks.size ? requestAnimationFrame(tick) : 0;
}

// Runs task(ms since last frame) until it returns false or is cancelled by the
// returned function. Re-adding a running task does nothing.
export function everyFrame(task) {
  tasks.add(task);
  if (!frame) {
    previous = performance.now();
    frame = requestAnimationFrame(tick);
  }
  return () => tasks.delete(task);
}

export function tween(duration, draw) {
  let elapsed = 0;
  return everyFrame((dt) => {
    elapsed += dt;
    const progress = Math.min(elapsed / duration, 1);
    draw(progress);
    return progress < 1;
  });
}

export const easeOut = (t) => 1 - (1 - t) ** 3;

export const clamp = (value, min = 0, max = 1) =>
  Math.min(max, Math.max(min, value));

export function formatTime(seconds) {
  const whole = Math.floor(seconds);
  return [whole / 60, whole % 60]
    .map((part) => String(Math.floor(part)).padStart(2, "0"))
    .join(":");
}

export function watchVisibility(element, onChange, threshold) {
  new IntersectionObserver(
    (entries) => onChange(entries.at(-1).isIntersecting),
    { threshold },
  ).observe(element);
}

export function whenVisible(elements, callback, options) {
  const observer = new IntersectionObserver((entries) => {
    if (!entries.some((entry) => entry.isIntersecting)) return;
    observer.disconnect();
    callback();
  }, options);
  for (const element of elements) observer.observe(element);
}
