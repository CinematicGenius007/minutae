// Live input waveform drawn on a canvas from `recording://levels` (~30 Hz).
// Levels are bucketed into one bar per BAR_MS of recording time and kept in a ring buffer;
// drawing is rAF-throttled to ~30 fps and only when something changed. Colours come from
// tokens (--ink, --onair, --line-strong) so it follows the theme.
import { useEffect, useRef } from "react";
import { useLevels } from "../../hooks";
import s from "./Live.module.css";

const BAR_W = 3;
const GAP = 3;
const STEP = BAR_W + GAP;
const BAR_MS = 100;
const LIVE_TAIL = 3;
const FUTURE = 12;
const CAP = 1024;
const FRAME_MS = 1000 / 30;

export function LiveWaveform({ capturing, height = 64 }: { capturing: boolean; height?: number }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const buf = useRef({ data: new Float32Array(CAP), head: 0, count: 0, bucket: -1, peak: 0 });
  const dirty = useRef(true);
  const capturingRef = useRef(capturing);
  capturingRef.current = capturing;

  useLevels((f) => {
    const b = buf.current;
    const level = Math.max(f.mic, f.system);
    const bucket = Math.floor(f.elapsedMs / BAR_MS);
    if (b.bucket === -1) b.bucket = bucket;
    if (bucket !== b.bucket) {
      b.data[b.head] = b.peak;
      b.head = (b.head + 1) % CAP;
      b.count = Math.min(CAP, b.count + 1);
      b.bucket = bucket;
      b.peak = 0;
      dirty.current = true;
    }
    b.peak = Math.max(b.peak, level);
  }, capturing);

  useEffect(() => {
    dirty.current = true;
  }, [capturing]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    let raf = 0;
    let last = 0;
    let width = 0;

    const resize = () => {
      const dpr = window.devicePixelRatio || 1;
      width = canvas.clientWidth;
      canvas.width = Math.round(width * dpr);
      canvas.height = Math.round(height * dpr);
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      dirty.current = true;
    };
    const ro = new ResizeObserver(resize);
    ro.observe(canvas);
    resize();

    // Theme switches change the tokens; redraw.
    const mo = new MutationObserver(() => (dirty.current = true));
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onScheme = () => (dirty.current = true);
    mq.addEventListener("change", onScheme);

    const bar = (x: number, h: number) => {
      const y = (height - h) / 2;
      ctx.beginPath();
      ctx.roundRect(x, y, BAR_W, h, 1.5);
      ctx.fill();
    };

    const draw = (t: number) => {
      raf = requestAnimationFrame(draw);
      if (!dirty.current || t - last < FRAME_MS) return;
      last = t;
      dirty.current = false;
      const css = getComputedStyle(canvas);
      const ink = css.getPropertyValue("--ink").trim();
      const onair = css.getPropertyValue("--onair").trim();
      const faint = css.getPropertyValue("--line-strong").trim();
      ctx.clearRect(0, 0, width, height);
      const slots = Math.max(0, Math.floor((width + GAP) / STEP));
      const b = buf.current;
      const shown = Math.min(b.count, Math.max(0, slots - FUTURE));
      for (let i = 0; i < shown; i++) {
        const idx = (b.head - shown + i + CAP) % CAP;
        const v = Math.min(1, Math.max(0, b.data[idx] ?? 0));
        const h = Math.max(4, Math.round(Math.sqrt(v) * height));
        ctx.fillStyle = capturingRef.current && i >= shown - LIVE_TAIL ? onair : ink;
        bar(i * STEP, h);
      }
      ctx.fillStyle = faint;
      for (let i = shown; i < Math.min(slots, shown + FUTURE); i++) bar(i * STEP, 6);
    };
    raf = requestAnimationFrame(draw);
    return () => {
      cancelAnimationFrame(raf);
      ro.disconnect();
      mo.disconnect();
      mq.removeEventListener("change", onScheme);
    };
  }, [height]);

  return (
    <canvas
      ref={canvasRef}
      className={s.wave}
      style={{ height }}
      role="img"
      aria-label={capturing ? "Live audio level" : "Audio level, paused"}
    />
  );
}
