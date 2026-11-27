import {
  everyFrame,
  formatTime,
  reducedMotion,
  whenVisible,
} from "../motion.js";
import { announce, setText } from "../ui.js";

export default function prompt() {
  const card = document.getElementById("prompt-card");
  const title = card.querySelector(".prompt-title");
  const body = card.querySelector(".prompt-body");
  const timer = card.querySelector(".prompt-timer");
  const [record, dismiss] = card.querySelectorAll(".prompt-btn");
  const idle = { title: title.textContent, body: body.textContent };
  let stopTimer;
  let comeBack = 0;

  function show(recording) {
    clearTimeout(comeBack);
    stopTimer?.();
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
      let elapsed = 0;
      stopTimer = everyFrame((dt) => {
        elapsed += dt;
        setText(timer, formatTime(elapsed / 1000));
      });
    }
  }

  record.addEventListener("click", () => {
    const recording = card.dataset.state !== "recording";
    show(recording);
    announce(`Recording demo ${recording ? "started" : "stopped"}.`);
  });

  dismiss.addEventListener("click", () => {
    if (card.dataset.state === "dismissed") return;
    card.dataset.state = "dismissed";
    announce("Prompt dismissed. Returning shortly.");
    comeBack = setTimeout(() => show(false), 2000);
  });

  if (reducedMotion.matches) return;
  const drop = card.animate(
    { opacity: [0, 1], transform: ["translateY(-16px)", "none"] },
    {
      duration: 320,
      easing: "cubic-bezier(0.2, 0.8, 0.2, 1)",
      fill: "backwards",
    },
  );
  drop.pause(); // hidden until seen
  whenVisible([card], () => drop.play(), { threshold: 0.3 });
}
