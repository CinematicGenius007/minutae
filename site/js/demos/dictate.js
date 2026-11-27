import {
  everyFrame,
  reducedMotion,
  tween,
  watchVisibility,
} from "../motion.js";

const CHAR_MS = 14;
const BAR_MS = 55;

function barsPath(step) {
  return Array.from({ length: 9 }, (_, i) => {
    const wave =
      0.5 + 0.5 * Math.sin(step * 0.24 + i * 0.95) * Math.cos(step * 0.11 + i);
    const envelope = 1 - Math.abs(i - 4) * 0.14;
    const height = Math.max(2, Math.round((2 + wave * 12) * envelope));
    return `M${2 + i * 6} ${Math.round(8 - height / 2)}v${height}`;
  }).join("");
}

export default function dictate() {
  const section = document.getElementById("chapter-dictate");
  const keycap = section.querySelector(".keycap-btn");
  const typed = document.getElementById("dictate-text");
  const rest = typed.nextElementSibling;
  const hud = section.querySelector(".dictate-hud");
  const label = hud.querySelector(".hud-label");
  const bars = hud.querySelector("path");
  const restingBars = bars.getAttribute("d");
  const sentence = typed.textContent.replace(/\s+/g, " ").trim();

  let holder = null; // "pointer", "key", "chord" or "demo"
  let visible = false;
  let demoPending = true;
  let demoTimer = 0;
  let step = 0;
  let stopBars;
  let stopTyping;

  function show(length) {
    typed.textContent = sentence.slice(0, length);
    rest.textContent = sentence.slice(length);
  }

  function setListening(listening) {
    keycap.classList.toggle("is-pressed", listening);
    hud.dataset.state = listening ? "listening" : "ready";
    label.textContent = listening ? "Listening" : "Ready";
  }

  function listen(by) {
    if (holder && holder !== "demo") return;
    if (by !== "demo") {
      demoPending = false; // the visitor takes over
      clearTimeout(demoTimer);
    }
    stopBars?.();
    holder = by;
    stopTyping?.();
    show(0);
    setListening(true);
    if (reducedMotion.matches) return;

    let sinceBar = 0;
    stopBars = everyFrame((dt) => {
      sinceBar += dt;
      if (sinceBar < BAR_MS) return;
      sinceBar = 0;
      bars.setAttribute("d", barsPath(++step));
    });
  }

  // Only whatever pressed the key releases it.
  function release(by) {
    if (!holder || holder !== by) return;
    holder = null;
    stopBars?.();
    bars.setAttribute("d", restingBars);
    setListening(false);
    if (reducedMotion.matches) show(sentence.length);
    else {
      stopTyping = tween(sentence.length * CHAR_MS, (progress) =>
        show(Math.floor(progress * sentence.length)),
      );
    }
  }

  function playDemo() {
    demoPending = false;
    demoTimer = setTimeout(() => {
      if (!visible || holder || reducedMotion.matches) return;
      listen("demo");
      demoTimer = setTimeout(() => release("demo"), 850);
    }, 320);
  }

  keycap.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;
    listen("pointer");
    // Keeps the release if the pointer drifts off.
    keycap.setPointerCapture(event.pointerId);
  });
  for (const type of ["pointerup", "pointercancel"]) {
    keycap.addEventListener(type, () => release("pointer"));
  }

  const isPress = (event) => event.key === " " || event.key === "Enter";
  keycap.addEventListener("keydown", (event) => {
    if (!isPress(event)) return;
    event.preventDefault();
    if (!event.repeat) listen("key");
  });
  keycap.addEventListener("keyup", (event) => {
    if (!isPress(event)) return;
    event.preventDefault();
    release("key");
  });
  keycap.addEventListener("blur", () => release("key"));

  // The only page-wide key taken, and only while the section is on screen.
  addEventListener("keydown", (event) => {
    const chord =
      event.code === "Space" &&
      event.altKey &&
      !event.ctrlKey &&
      !event.metaKey;
    if (!visible || !chord) return;
    event.preventDefault();
    if (!event.repeat) listen("chord");
  });
  addEventListener("keyup", (event) => {
    if (event.code === "Space" || event.key === "Alt") release("chord");
  });
  addEventListener("blur", () => release(holder));

  watchVisibility(
    section,
    (isVisible) => {
      visible = isVisible;
      if (!visible) release("chord");
      else if (demoPending && !reducedMotion.matches) playDemo();
    },
    0.35,
  );
}
