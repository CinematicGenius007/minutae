import { formatTime, reducedMotion, whenVisible } from "../motion.js";
import { announce } from "../ui.js";

export default function prompt() {
  const card = document.getElementById("prompt-card");
  const title = card.querySelector(".prompt-title");
  const body = card.querySelector(".prompt-body");
  const timer = card.querySelector(".prompt-timer");
  const [record, dismiss] = card.querySelectorAll(".prompt-btn");
  const idle = { title: title.textContent, body: body.textContent };
  let ticking = 0;
  let comeBack = 0;

  function show(recording) {
    clearTimeout(comeBack);
    clearInterval(ticking);
    card.inert = false;
    card.dataset.state = recording ? "recording" : "idle";
    title.textContent = recording ? "Recording" : idle.title;
    body.textContent = recording
      ? "Demo only — nothing is actually being recorded."
      : idle.body;
    timer.textContent = formatTime(0);
    timer.hidden = !recording;
    record.textContent = recording ? "Stop" : "Record";
    dismiss.hidden = recording;

    if (recording) {
      let seconds = 0;
      ticking = setInterval(() => {
        timer.textContent = formatTime(++seconds);
      }, 1000);
    }
  }

  record.addEventListener("click", () => {
    const recording = card.dataset.state !== "recording";
    show(recording);
    announce(`Recording demo ${recording ? "started" : "stopped"}.`);
  });

  dismiss.addEventListener("click", () => {
    if (card.dataset.state === "dismissed") return;
    const hadFocus = card.contains(document.activeElement);
    card.dataset.state = "dismissed";
    card.inert = true;
    announce("Prompt dismissed. Returning shortly.");
    comeBack = setTimeout(() => {
      show(false);
      // Give keyboard users their place back, unless they have moved on.
      if (hadFocus && document.activeElement === document.body) {
        dismiss.focus({ preventScroll: true });
      }
    }, 2000);
  });

  // Created only once in view, so the card is never left invisible while waiting.
  whenVisible(
    [card],
    () => {
      if (reducedMotion.matches) return;
      card.animate(
        { opacity: [0, 1], transform: ["translateY(-16px)", "none"] },
        { duration: 320, easing: "cubic-bezier(0.2, 0.8, 0.2, 1)" },
      );
    },
    { threshold: 0.05 },
  );
}
