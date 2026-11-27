import {
  everyFrame,
  formatTime,
  reducedMotion,
  watchVisibility,
} from "../motion.js";
import { announce, setText } from "../ui.js";

const CHAR_MS = 38;
const BREATH_MS = 120;
const REPLY_MS = 450;
const READ_MS = 2400;
const FIRST_HOLD_MS = 1800;
const BAR_MS = 110;
const START_SECONDS = 12 * 60 + 4;

// [them, you], after the exchange written in the HTML.
const MORE_EXCHANGES = [
  [
    "Should we check the performance\nbudget before we lock the date?",
    "Good call. Bob can review it\nbefore Thursday's sync.",
  ],
  [
    "And the new onboarding ships in the\nsame release?",
    "Yes, everything goes out together\nin the May drop.",
  ],
  [
    "Anything else before we wrap up the\nQ2 sync?",
    "That's all. The summary will be in\nthe folder in a second.",
  ],
];

function timing([them, you]) {
  const reply = them.length * CHAR_MS + BREATH_MS + REPLY_MS;
  return { reply, done: reply + you.length * CHAR_MS + BREATH_MS };
}

function barHeight(speaking, step) {
  if (!speaking) return 2;
  if (Math.sin(step * 0.27) < -0.78) return 4;
  const loudness =
    Math.abs(Math.sin(step * 0.42)) * 0.55 +
    Math.abs(Math.sin(step * 1.13 + 1.4)) * 0.35 +
    Math.abs(Math.cos(step * 2.17)) * 0.1;
  return Math.round(10 + loudness * 42);
}

function waveform(selector) {
  const path = document.querySelector(selector);
  const heights = [...path.getAttribute("d").matchAll(/v(\d+)/g)].map((match) =>
    Number(match[1]),
  );

  return function push(height) {
    heights.pop();
    heights.unshift(height);
    const bars = heights.map(
      (h, i) => `M${2 + i * 9} ${Math.round(32 - h / 2)}v${h}`,
    );
    path.setAttribute("d", bars.join(""));
  };
}

export default function tape() {
  const band = document.getElementById("hero-tape");
  const toggle = band.querySelector(".tape-toggle");
  const clock = document.getElementById("tape-time");
  const lines = [...band.querySelectorAll(".tape-line")];
  const exchanges = [lines.map((line) => line.textContent), ...MORE_EXCHANGES];
  const pushThem = waveform("#wave-them path");
  const pushYou = waveform("#wave-you path");
  const restart = () => timing(exchanges[0]).done + READ_MS - FIRST_HOLD_MS;

  let exchange = 0;
  let time = restart();
  let elapsed = 0;
  let sinceBar = 0;
  let step = 0;
  let visible = false;
  let paused = false;
  let stop;

  const write = (them, you) => {
    setText(lines[0], them);
    setText(lines[1], you);
  };

  function frame(dt) {
    elapsed += dt;
    time += dt;
    sinceBar += dt;
    if (time >= timing(exchanges[exchange]).done + READ_MS) {
      exchange = (exchange + 1) % exchanges.length;
      time = 0;
    }

    const [them, you] = exchanges[exchange];
    const { reply, done } = timing(exchanges[exchange]);
    const typed = (line, since) =>
      line.slice(0, Math.max(0, Math.floor(since / CHAR_MS)));
    write(typed(them, time), typed(you, time - reply));
    setText(clock, formatTime(START_SECONDS + elapsed / 1000));

    if (sinceBar >= BAR_MS) {
      sinceBar -= BAR_MS;
      step += 2;
      const opening = elapsed < FIRST_HOLD_MS;
      const themTalking = opening
        ? elapsed < FIRST_HOLD_MS / 2
        : time < reply - REPLY_MS;
      const youTalking = opening ? !themTalking : time >= reply && time < done;
      pushThem(barHeight(themTalking, step));
      pushYou(barHeight(youTalking, step + 5));
    }
  }

  function run() {
    const live = visible && !paused && !reducedMotion.matches;
    band.classList.toggle("is-live", live);
    stop?.();
    if (live) stop = everyFrame(frame);
  }

  toggle.addEventListener("click", () => {
    paused = !paused;
    toggle.setAttribute(
      "aria-label",
      `${paused ? "Resume" : "Pause"} live recording preview`,
    );
    announce(`Recording preview ${paused ? "paused" : "resumed"}.`);
    run();
  });

  reducedMotion.addEventListener("change", () => {
    if (reducedMotion.matches) {
      exchange = 0;
      time = restart();
      elapsed = 0;
      write(...exchanges[0]);
      clock.textContent = formatTime(START_SECONDS);
    }
    run();
  });

  watchVisibility(
    band,
    (isVisible) => {
      visible = isVisible;
      run();
    },
    0.05,
  );
}
