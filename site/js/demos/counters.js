import {
  clamp,
  easeOut,
  formatTime,
  reducedMotion,
  tween,
  whenVisible,
} from "../motion.js";
import { setText } from "../ui.js";

function rollTimestamp(display) {
  const from = display.dataset.rollFrom;
  const to = display.textContent.trim();

  tween(560, (progress) => {
    display.textContent = Array.from(to, (digit, i) => {
      if (digit === ":" || digit === from[i]) return digit;
      const delay = i * 0.08;
      const local = easeOut(clamp((progress - delay) / (1 - delay)));
      return Math.round(Number(from[i]) + (digit - from[i]) * local);
    }).join("");
  });
}

function countDurations(list) {
  const durations = [...list.querySelectorAll(".speaker-dur-display")].map(
    (element) => {
      const [minutes, seconds] = element.textContent.split(":");
      return { element, total: minutes * 60 + Number(seconds) };
    },
  );

  tween(680, (progress) => {
    for (const { element, total } of durations) {
      setText(element, formatTime(Math.round(total * easeOut(progress))));
    }
  });
}

export default function counters() {
  const onFirstView = (selector, threshold, run) => {
    for (const element of document.querySelectorAll(selector)) {
      whenVisible(
        [element],
        () => {
          if (!reducedMotion.matches) run(element);
        },
        { threshold },
      );
    }
  };
  onFirstView(".chapter-time-display", 0.35, rollTimestamp);
  onFirstView("#speaker-list", 0.3, countDurations);
}
