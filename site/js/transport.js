import { clamp, everyFrame, reducedMotion } from "./motion.js";
import { announce, setText } from "./ui.js";

const PIXELS_PER_SECOND = 175;

export default function transport() {
  const slot = document.querySelector(".transport-slot");
  const bar = slot.querySelector(".transport");
  const play = bar.querySelector(".transport-play");
  const clock = document.getElementById("transport-time");
  const segments = [...bar.querySelectorAll(".transport-seg")];
  const chapters = segments.map((segment) =>
    document.querySelector(segment.hash),
  );
  const closer = document.querySelector(".closer");

  const maxScroll = () => document.documentElement.scrollHeight - innerHeight;
  const jumpTo = (top) => scrollTo({ top, behavior: "instant" });

  function update() {
    bar.classList.toggle("is-docked", slot.getBoundingClientRect().bottom <= 0);

    const y = scrollY;
    const atBottom = y >= maxScroll() - 24;
    // Capped, so a short chapter jumped to is still the current one.
    const readingLine = y + Math.min(innerHeight * 0.35, 160);
    const starts = chapters.map((chapter, i) => (i ? chapter.offsetTop : 0));
    const ends = [...starts.slice(1), closer.offsetTop];
    const active = atBottom
      ? segments.length - 1
      : starts.findLastIndex((start) => readingLine >= start);

    segments.forEach((segment, i) => {
      // At the top, "start" is drawn full.
      let progress = clamp((readingLine - starts[i]) / (ends[i] - starts[i]));
      if (y < 16) progress = Number(i === 0);
      else if (atBottom) progress = 1;
      segment.style.setProperty("--progress", progress.toFixed(4));
      if (i === active) segment.setAttribute("aria-current", "location");
      else segment.removeAttribute("aria-current");
    });
    setText(clock, atBottom ? "33:00" : segments[active].dataset.time);
    return false;
  }

  const refresh = () => everyFrame(update);
  addEventListener("scroll", refresh, { passive: true });
  addEventListener("resize", refresh, { passive: true });
  refresh(); // the page may open part way down

  let stop = null;

  function setPlaying(playing) {
    play.toggleAttribute("data-playing", playing);
    play.setAttribute(
      "aria-label",
      playing ? "Pause call walkthrough" : "Play through the call",
    );
  }

  function pause() {
    if (!stop) return;
    stop();
    stop = null;
    setPlaying(false);
  }

  function start() {
    if (scrollY >= maxScroll() - 8) jumpTo(0);

    // Reduced motion: step one chapter per press instead.
    if (reducedMotion.matches) {
      const next = chapters.find((chapter) => chapter.offsetTop > scrollY + 40);
      jumpTo(next?.offsetTop ?? maxScroll());
      return;
    }

    setPlaying(true);
    announce("Playing through the call.");
    let y = scrollY; // kept here because scrollY is rounded
    stop = everyFrame((dt) => {
      const end = maxScroll();
      y = Math.min(end, y + (PIXELS_PER_SECOND * dt) / 1000);
      jumpTo(y);
      if (y >= end - 1) {
        pause();
        announce("Reached the end of the call.");
      }
    });
  }

  play.addEventListener("click", () => (stop ? pause() : start()));

  // Any input cancels playing, except pressing play: its click toggles.
  addEventListener("wheel", pause, { passive: true });
  addEventListener("pointerdown", (event) => {
    if (!play.contains(event.target)) pause();
  });
  addEventListener("keydown", (event) => {
    const pressesPlay =
      event.target === play && (event.key === " " || event.key === "Enter");
    if (!pressesPlay) pause();
  });
  // A screen reader may click without a pointer or key event.
  for (const segment of segments) segment.addEventListener("click", pause);
}
