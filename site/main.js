/* ==========================================================================
   Minutae website — Interactions & Motion
   One shared rAF loop, IntersectionObserver-gated, prefers-reduced-motion safe.
   ========================================================================== */

const reducedMotionQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
const prefersReducedMotion = () => reducedMotionQuery.matches;

const liveStatus = document.getElementById("live-status");
function announce(message) {
  if (!liveStatus) return;
  liveStatus.textContent = "";
  requestAnimationFrame(() => {
    liveStatus.textContent = message;
  });
}

/* ==========================================================================
   Shared requestAnimationFrame Scheduler
   ========================================================================== */

const scheduler = (() => {
  const tasks = new Map();
  let rafId = 0;
  let lastNow = 0;

  function tick(now) {
    rafId = 0;
    const dt = lastNow ? Math.min(now - lastNow, 100) : 16;
    lastNow = now;

    for (const [key, fn] of Array.from(tasks.entries())) {
      const keep = fn(now, dt);
      if (keep === false) {
        tasks.delete(key);
      }
    }

    if (tasks.size > 0 && !document.hidden) {
      rafId = requestAnimationFrame(tick);
    } else {
      lastNow = 0;
    }
  }

  function add(key, fn) {
    tasks.set(key, fn);
    if (!rafId && !document.hidden) {
      lastNow = 0;
      rafId = requestAnimationFrame(tick);
    }
  }

  function remove(key) {
    tasks.delete(key);
    if (tasks.size === 0 && rafId) {
      cancelAnimationFrame(rafId);
      rafId = 0;
      lastNow = 0;
    }
  }

  document.addEventListener("visibilitychange", () => {
    if (document.hidden) {
      if (rafId) {
        cancelAnimationFrame(rafId);
        rafId = 0;
        lastNow = 0;
      }
    } else if (tasks.size > 0 && !rafId) {
      lastNow = 0;
      rafId = requestAnimationFrame(tick);
    }
  });

  return { add, remove };
})();

/* ==========================================================================
   Helpers: Clipboard, Formatting, Easing
   ========================================================================== */

const INSTALL_COMMANDS = {
  unix: {
    cmd: "curl -fsSL https://raw.githubusercontent.com/CinematicGenius007/minutae/main/scripts/install.sh | sh",
    showPrompt: true,
  },
  windows: {
    cmd: "scoop install https://github.com/CinematicGenius007/minutae/releases/latest/download/minutae.json",
    showPrompt: true,
  },
  source: {
    cmd: "git clone https://github.com/CinematicGenius007/minutae && cd minutae && sh scripts/install.sh --from-source",
    showPrompt: true,
  },
  agent: {
    cmd: "Install Minutae for me. Follow docs/agents/INSTALL.md in github.com/CinematicGenius007/minutae",
    showPrompt: false,
  },
};

async function copyText(text) {
  if (navigator.clipboard && window.isSecureContext) {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch (_) {
      // Fall through to execCommand fallback
    }
  }
  try {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.setAttribute("readonly", "");
    ta.style.position = "fixed";
    ta.style.top = "-9999px";
    ta.style.opacity = "0";
    document.body.appendChild(ta);
    ta.select();
    const ok = document.execCommand("copy");
    document.body.removeChild(ta);
    return ok;
  } catch (_) {
    return false;
  }
}

function formatMMSS(totalSeconds) {
  const s = Math.max(0, Math.floor(totalSeconds));
  const mm = String(Math.floor(s / 60)).padStart(2, "0");
  const ss = String(s % 60).padStart(2, "0");
  return `${mm}:${ss}`;
}

function easeOutCubic(t) {
  return 1 - Math.pow(1 - t, 3);
}

/* ==========================================================================
   8. Theme Switcher (light / dark / system + View Transitions circular reveal)
   ========================================================================== */

(() => {
  const STORAGE_KEY = "minutae-theme";
  const radios = Array.from(document.querySelectorAll("[data-theme-choice]"));
  if (!radios.length) return;

  const darkMedia = window.matchMedia("(prefers-color-scheme: dark)");

  function getSavedChoice() {
    try {
      const saved = localStorage.getItem(STORAGE_KEY);
      if (saved === "light" || saved === "dark" || saved === "system") {
        return saved;
      }
    } catch (_) {}
    return "system";
  }

  function resolveEffective(choice) {
    if (choice === "light" || choice === "dark") return choice;
    return darkMedia.matches ? "dark" : "light";
  }

  function syncRadios(choice) {
    radios.forEach((btn) => {
      const isSelected = btn.dataset.themeChoice === choice;
      btn.setAttribute("aria-checked", String(isSelected));
      btn.tabIndex = isSelected ? 0 : -1;
    });
  }

  function applyTheme(choice) {
    if (choice === "light" || choice === "dark") {
      document.documentElement.setAttribute("data-theme", choice);
    } else {
      document.documentElement.removeAttribute("data-theme");
    }
    try {
      localStorage.setItem(STORAGE_KEY, choice);
    } catch (_) {}
    syncRadios(choice);
  }

  let currentChoice = getSavedChoice();
  syncRadios(currentChoice);

  function selectTheme(choice, sourceEl) {
    if (choice === currentChoice) return;
    const prevEffective = resolveEffective(currentChoice);
    const nextEffective = resolveEffective(choice);
    currentChoice = choice;

    const canTransition =
      typeof document.startViewTransition === "function" &&
      !prefersReducedMotion() &&
      prevEffective !== nextEffective;

    if (!canTransition) {
      applyTheme(choice);
      return;
    }

    const rect = sourceEl.getBoundingClientRect();
    const x = rect.left + rect.width / 2;
    const y = rect.top + rect.height / 2;
    const maxRadius = Math.hypot(
      Math.max(x, window.innerWidth - x),
      Math.max(y, window.innerHeight - y),
    );

    const transition = document.startViewTransition(() => {
      applyTheme(choice);
    });

    transition.ready
      .then(() => {
        document.documentElement.animate(
          {
            clipPath: [
              `circle(0px at ${x}px ${y}px)`,
              `circle(${maxRadius}px at ${x}px ${y}px)`,
            ],
          },
          {
            duration: 320,
            easing: "cubic-bezier(0.2, 0.8, 0.2, 1)",
            pseudoElement: "::view-transition-new(root)",
          },
        );
      })
      .catch(() => {});
  }

  radios.forEach((btn, idx) => {
    btn.addEventListener("click", () => {
      selectTheme(btn.dataset.themeChoice, btn);
    });

    btn.addEventListener("keydown", (e) => {
      let nextIdx = -1;
      if (e.key === "ArrowRight" || e.key === "ArrowDown") {
        nextIdx = (idx + 1) % radios.length;
      } else if (e.key === "ArrowLeft" || e.key === "ArrowUp") {
        nextIdx = (idx - 1 + radios.length) % radios.length;
      } else if (e.key === "Home") {
        nextIdx = 0;
      } else if (e.key === "End") {
        nextIdx = radios.length - 1;
      }
      if (nextIdx !== -1) {
        e.preventDefault();
        const target = radios[nextIdx];
        target.focus();
        selectTheme(target.dataset.themeChoice, target);
      }
    });
  });
})();

/* ==========================================================================
   Install Tabs & Clipboard Copy
   ========================================================================== */

(() => {
  const tabs = Array.from(document.querySelectorAll("[data-install-tab]"));
  const panel = document.getElementById("install-panel");
  const promptEl = document.getElementById("install-prompt");
  const cmdEl = document.getElementById("install-command");
  const copyBtn = document.getElementById("install-copy");
  if (!tabs.length || !panel || !cmdEl || !copyBtn) return;

  let activeKey = "unix";
  let resetTimer = 0;

  function activateTab(key, focusTab = false) {
    const entry = INSTALL_COMMANDS[key];
    if (!entry) return;
    activeKey = key;

    tabs.forEach((tab) => {
      const selected = tab.dataset.installTab === key;
      tab.setAttribute("aria-selected", String(selected));
      tab.tabIndex = selected ? 0 : -1;
      if (selected) {
        panel.setAttribute("aria-labelledby", tab.id);
        if (focusTab) tab.focus();
      }
    });

    if (promptEl) promptEl.hidden = !entry.showPrompt;
    cmdEl.textContent = entry.cmd;
    cmdEl.scrollLeft = 0;
  }

  tabs.forEach((tab, idx) => {
    tab.addEventListener("click", () => {
      activateTab(tab.dataset.installTab);
    });

    tab.addEventListener("keydown", (e) => {
      let nextIdx = -1;
      if (e.key === "ArrowRight") nextIdx = (idx + 1) % tabs.length;
      else if (e.key === "ArrowLeft")
        nextIdx = (idx - 1 + tabs.length) % tabs.length;
      else if (e.key === "Home") nextIdx = 0;
      else if (e.key === "End") nextIdx = tabs.length - 1;

      if (nextIdx !== -1) {
        e.preventDefault();
        activateTab(tabs[nextIdx].dataset.installTab, true);
      }
    });
  });

  copyBtn.addEventListener("click", async () => {
    const cmd = INSTALL_COMMANDS[activeKey].cmd;
    const ok = await copyText(cmd);
    window.clearTimeout(resetTimer);

    if (ok) {
      copyBtn.textContent = "copied";
      announce("Install command copied to clipboard.");
    } else {
      copyBtn.textContent = "select";
      try {
        const range = document.createRange();
        range.selectNodeContents(cmdEl);
        const sel = window.getSelection();
        sel.removeAllRanges();
        sel.addRange(range);
      } catch (_) {}
      announce("Could not copy automatically. Command selected.");
    }

    resetTimer = window.setTimeout(() => {
      copyBtn.textContent = "copy";
    }, 1600);
  });
})();

/* ==========================================================================
   1. Hero Tape — Scrolling waveforms, matching typewriter, ticking timecode
   ========================================================================== */

(() => {
  const tape = document.getElementById("hero-tape");
  const tapeBtn = document.getElementById("hero-tape-btn");
  const waveThemPath = document.querySelector("#wave-them path");
  const waveYouPath = document.querySelector("#wave-you path");
  const textThemEl = document.getElementById("tape-text-them");
  const textYouEl = document.getElementById("tape-text-you");
  const timeEl = document.getElementById("tape-time");
  const dotEl = document.getElementById("tape-dot");

  if (
    !tape ||
    !tapeBtn ||
    !waveThemPath ||
    !waveYouPath ||
    !textThemEl ||
    !textYouEl ||
    !timeEl
  ) {
    return;
  }

  const EXCHANGES = [
    {
      them: "So we move to May. Can you share the\nupdated mockups by Friday?",
      you: "Friday works. I'll move it on the\nroadmap today.",
    },
    {
      them: "Should we check the performance\nbudget before we lock the date?",
      you: "Good call. Bob can review it\nbefore Thursday's sync.",
    },
    {
      them: "And the new onboarding ships in the\nsame release?",
      you: "Yes, everything goes out together\nin the May drop.",
    },
    {
      them: "Anything else before we wrap up the\nQ2 sync?",
      you: "That's all. The summary will be in\nthe folder in a second.",
    },
  ];

  // Parse initial static 62-bar heights from the designed SVG paths
  function parseHeights(d) {
    const heights = [];
    const re = /v(\d+)/g;
    let m;
    while ((m = re.exec(d))) {
      heights.push(Number(m[1]));
    }
    return heights;
  }

  const BAR_COUNT = 62;
  const BAR_STEP = 9;
  const BASE_TIME = 12 * 60 + 4; // 12:04

  const themBars = parseHeights(waveThemPath.getAttribute("d") || "");
  const youBars = parseHeights(waveYouPath.getAttribute("d") || "");
  while (themBars.length < BAR_COUNT) themBars.push(2);
  while (youBars.length < BAR_COUNT) youBars.push(2);

  function buildPath(bars) {
    let d = "";
    for (let i = 0; i < BAR_COUNT; i++) {
      const h = Math.max(2, Math.min(54, Math.round(bars[i])));
      const x = 2 + i * BAR_STEP;
      const y = Math.round(32 - h / 2);
      d += `M${x} ${y}v${h}`;
    }
    return d;
  }

  // Deterministic organic voice amplitude generator
  let waveSeed = 0;
  function sampleBar(isSpeaking, phase) {
    waveSeed += 1;
    if (!isSpeaking) return 2;
    const syllable =
      Math.abs(Math.sin(phase * 0.42)) * 0.55 +
      Math.abs(Math.sin(phase * 1.13 + 1.4)) * 0.35 +
      Math.abs(Math.cos(phase * 2.17)) * 0.1;
    // Occasional brief breath pause between words
    if (Math.sin(phase * 0.27) < -0.78) return 4;
    return Math.round(10 + syllable * 42);
  }

  let inView = true;
  let paused = false;
  let elapsedMs = 0;
  let barAccumMs = 0;
  let exchangeIdx = 0;
  // Phase within exchange:
  // 0: initial hold of designed frame on first load (only once)
  // 1: typing THEM
  // 2: short beat
  // 3: typing YOU
  // 4: hold finished exchange before next
  let phase = 0;
  let phaseMs = 0;

  const INITIAL_HOLD_MS = 1800;
  const CHAR_MS = 38;
  const BEAT_MS = 450;
  const END_HOLD_MS = 2400;
  const BAR_SHIFT_MS = 110;

  function renderExchangeText() {
    const ex = EXCHANGES[exchangeIdx];
    if (phase === 0 || phase === 4) {
      textThemEl.textContent = ex.them;
      textYouEl.textContent = ex.you;
    } else if (phase === 1) {
      const n = Math.min(ex.them.length, Math.floor(phaseMs / CHAR_MS));
      textThemEl.textContent = ex.them.slice(0, n);
      textYouEl.textContent = "";
    } else if (phase === 2) {
      textThemEl.textContent = ex.them;
      textYouEl.textContent = "";
    } else if (phase === 3) {
      const n = Math.min(ex.you.length, Math.floor(phaseMs / CHAR_MS));
      textThemEl.textContent = ex.them;
      textYouEl.textContent = ex.you.slice(0, n);
    }
  }

  function stepTape(_now, dt) {
    if (paused || !inView || prefersReducedMotion()) {
      return false;
    }

    elapsedMs += dt;
    phaseMs += dt;
    barAccumMs += dt;

    // Tick timecode from 12:04
    const displaySec = BASE_TIME + Math.floor(elapsedMs / 1000);
    const formatted = formatMMSS(displaySec);
    if (timeEl.textContent !== formatted) {
      timeEl.textContent = formatted;
    }

    // Advance phase machine
    const ex = EXCHANGES[exchangeIdx];
    if (phase === 0) {
      if (phaseMs >= INITIAL_HOLD_MS) {
        exchangeIdx = (exchangeIdx + 1) % EXCHANGES.length;
        phase = 1;
        phaseMs = 0;
      }
    } else if (phase === 1) {
      if (phaseMs >= ex.them.length * CHAR_MS + 120) {
        phase = 2;
        phaseMs = 0;
      }
    } else if (phase === 2) {
      if (phaseMs >= BEAT_MS) {
        phase = 3;
        phaseMs = 0;
      }
    } else if (phase === 3) {
      if (phaseMs >= ex.you.length * CHAR_MS + 120) {
        phase = 4;
        phaseMs = 0;
      }
    } else if (phase === 4) {
      if (phaseMs >= END_HOLD_MS) {
        exchangeIdx = (exchangeIdx + 1) % EXCHANGES.length;
        phase = 1;
        phaseMs = 0;
      }
    }

    renderExchangeText();

    // Scroll waveforms left-to-right into the red playhead on the right
    if (barAccumMs >= BAR_SHIFT_MS) {
      const shifts = Math.min(3, Math.floor(barAccumMs / BAR_SHIFT_MS));
      barAccumMs -= shifts * BAR_SHIFT_MS;

      const themTalking = phase === 1 || (phase === 0 && elapsedMs < 900);
      const youTalking = phase === 3 || (phase === 0 && elapsedMs >= 900);

      for (let i = 0; i < shifts; i++) {
        themBars.pop();
        youBars.pop();
        themBars.unshift(sampleBar(themTalking, waveSeed));
        youBars.unshift(sampleBar(youTalking, waveSeed + 5));
      }

      waveThemPath.setAttribute("d", buildPath(themBars));
      waveYouPath.setAttribute("d", buildPath(youBars));
    }

    return true;
  }

  function updateTapeRunState() {
    const shouldRun = inView && !paused && !prefersReducedMotion();
    if (dotEl) dotEl.classList.toggle("is-pulsing", shouldRun);
    if (shouldRun) {
      scheduler.add("hero-tape", stepTape);
    } else {
      scheduler.remove("hero-tape");
    }
  }

  function togglePause() {
    paused = !paused;
    tapeBtn.setAttribute("aria-pressed", String(paused));
    tapeBtn.setAttribute(
      "aria-label",
      paused
        ? "Resume live recording preview"
        : "Pause live recording preview",
    );
    announce(paused ? "Recording preview paused." : "Recording preview resumed.");
    updateTapeRunState();
  }

  tapeBtn.addEventListener("click", togglePause);

  const tapeObserver = new IntersectionObserver(
    (entries) => {
      inView = entries[0].isIntersecting;
      updateTapeRunState();
    },
    { threshold: 0.05 },
  );
  tapeObserver.observe(tape);

  reducedMotionQuery.addEventListener("change", () => {
    if (prefersReducedMotion()) {
      exchangeIdx = 0;
      phase = 0;
      textThemEl.textContent = EXCHANGES[0].them;
      textYouEl.textContent = EXCHANGES[0].you;
      timeEl.textContent = "12:04";
    }
    updateTapeRunState();
  });

  updateTapeRunState();
})();

/* ==========================================================================
   2. Chapter Timestamps — Digit-by-digit roll once on enter
   ========================================================================== */

(() => {
  const displays = Array.from(
    document.querySelectorAll(".chapter-time-display[data-roll-to]"),
  );
  if (!displays.length) return;

  function rollDisplay(el) {
    const fromStr = el.dataset.rollFrom || "00:00";
    const toStr = el.dataset.rollTo || el.textContent.trim();
    if (prefersReducedMotion() || fromStr.length !== toStr.length) {
      el.textContent = toStr;
      return;
    }

    const startTime = performance.now();
    const duration = 560;
    const key = `roll-${toStr}`;

    scheduler.add(key, (now) => {
      const p = Math.min(1, (now - startTime) / duration);
      let out = "";
      for (let i = 0; i < toStr.length; i++) {
        const fc = fromStr[i];
        const tc = toStr[i];
        if (fc === ":" || tc === ":" || fc === tc) {
          out += tc;
          continue;
        }
        const fNum = Number(fc);
        const tNum = Number(tc);
        // Stagger digits slightly left-to-right
        const localP = Math.max(0, Math.min(1, (p - i * 0.08) / (1 - i * 0.08)));
        const eased = easeOutCubic(localP);
        const val = Math.round(fNum + (tNum - fNum) * eased);
        out += String(val);
      }
      el.textContent = out;
      if (p >= 1) {
        el.textContent = toStr;
        return false;
      }
      return true;
    });
  }

  const observer = new IntersectionObserver(
    (entries) => {
      entries.forEach((entry) => {
        if (!entry.isIntersecting) return;
        observer.unobserve(entry.target);
        rollDisplay(entry.target);
      });
    },
    { threshold: 0.35 },
  );

  displays.forEach((el) => observer.observe(el));
})();

/* ==========================================================================
   3. Prompt (00:04) — Drops in, Record / Stop simulation, Not now slide-away
   ========================================================================== */

(() => {
  const card = document.getElementById("prompt-card");
  const dot = document.getElementById("prompt-dot");
  const titleEl = document.getElementById("prompt-title");
  const bodyEl = document.getElementById("prompt-body");
  const timerEl = document.getElementById("prompt-timer");
  const recordBtn = document.getElementById("prompt-record");
  const dismissBtn = document.getElementById("prompt-dismiss");
  if (!card || !titleEl || !bodyEl || !recordBtn || !dismissBtn) return;

  let state = "idle"; // "idle" | "recording" | "dismissed"
  let recStartMs = 0;
  let returnTimer = 0;

  // Drop-in notification animation once on enter
  if (!prefersReducedMotion()) {
    card.classList.add("is-entering");
    const enterObs = new IntersectionObserver(
      (entries) => {
        if (!entries[0].isIntersecting) return;
        enterObs.disconnect();
        requestAnimationFrame(() => {
          card.classList.remove("is-entering");
        });
      },
      { threshold: 0.3 },
    );
    enterObs.observe(card);
  }

  function setIdle() {
    state = "idle";
    card.dataset.state = "idle";
    card.classList.remove("is-dismissed");
    scheduler.remove("prompt-timer");
    if (dot) dot.classList.remove("is-pulsing");
    titleEl.textContent = "An app is using your mic";
    bodyEl.textContent = "Record this call? Audio stays on this computer.";
    if (timerEl) {
      timerEl.hidden = true;
      timerEl.textContent = "00:00";
    }
    recordBtn.textContent = "Record";
    dismissBtn.hidden = false;
  }

  function setRecording() {
    state = "recording";
    card.dataset.state = "recording";
    recStartMs = performance.now();
    if (dot) dot.classList.toggle("is-pulsing", !prefersReducedMotion());
    titleEl.textContent = "Recording";
    bodyEl.textContent = "Demo only — nothing is actually being recorded.";
    if (timerEl) {
      timerEl.hidden = false;
      timerEl.textContent = "00:00";
    }
    recordBtn.textContent = "Stop";
    dismissBtn.hidden = true;
    announce("Recording demo started.");

    scheduler.add("prompt-timer", (now) => {
      if (state !== "recording") return false;
      const secs = Math.floor((now - recStartMs) / 1000);
      if (timerEl) {
        const text = formatMMSS(secs);
        if (timerEl.textContent !== text) timerEl.textContent = text;
      }
      return true;
    });
  }

  recordBtn.addEventListener("click", () => {
    window.clearTimeout(returnTimer);
    if (state === "recording") {
      setIdle();
      announce("Recording demo stopped.");
    } else {
      setRecording();
    }
  });

  dismissBtn.addEventListener("click", () => {
    if (state !== "idle") return;
    state = "dismissed";
    card.dataset.state = "dismissed";
    card.classList.add("is-dismissed");
    announce("Prompt dismissed. Returning shortly.");

    window.clearTimeout(returnTimer);
    returnTimer = window.setTimeout(() => {
      setIdle();
    }, 2000);
  });
})();

/* ==========================================================================
   4. Speaker List (count-up once) + File Viewer (tree order + checkbox sync)
   ========================================================================== */

(() => {
  const speakerList = document.getElementById("speaker-list");
  if (speakerList) {
    const durEls = Array.from(
      speakerList.querySelectorAll(".speaker-dur-display[data-count-to]"),
    );

    const obs = new IntersectionObserver(
      (entries) => {
        if (!entries[0].isIntersecting) return;
        obs.disconnect();
        if (prefersReducedMotion()) return;

        const start = performance.now();
        const duration = 680;
        scheduler.add("speaker-count", (now) => {
          const p = Math.min(1, (now - start) / duration);
          const eased = easeOutCubic(p);
          durEls.forEach((el) => {
            const target = Number(el.dataset.countTo || 0);
            el.textContent = formatMMSS(Math.round(target * eased));
          });
          if (p >= 1) {
            durEls.forEach((el) => {
              el.textContent = formatMMSS(Number(el.dataset.countTo || 0));
            });
            return false;
          }
          return true;
        });
      },
      { threshold: 0.3 },
    );
    obs.observe(speakerList);
  }

  // File tree line-by-line reveal, with .md line last
  const tree = document.getElementById("file-tree");
  if (tree && !prefersReducedMotion()) {
    const lines = Array.from(tree.querySelectorAll(".tree-line"));
    tree.classList.add("is-pending");

    const treeObs = new IntersectionObserver(
      (entries) => {
        if (!entries[0].isIntersecting) return;
        treeObs.disconnect();

        const sorted = lines
          .slice()
          .sort(
            (a, b) => Number(a.dataset.treeOrder) - Number(b.dataset.treeOrder),
          );

        sorted.forEach((line, idx) => {
          const isLast = idx === sorted.length - 1;
          const delay = idx * 65 + (isLast ? 110 : 0);
          window.setTimeout(() => {
            line.classList.add("is-visible");
            if (isLast) {
              window.setTimeout(() => {
                tree.classList.remove("is-pending");
              }, 260);
            }
          }, delay);
        });
      },
      { threshold: 0.25 },
    );
    treeObs.observe(tree);
  }

  // READ checkboxes synced with RAW pane
  const checkboxes = Array.from(
    document.querySelectorAll(".read-check-input[data-sync-raw]"),
  );
  checkboxes.forEach((input) => {
    input.addEventListener("change", () => {
      const rawLine = document.getElementById(input.dataset.syncRaw);
      const rawText = input.dataset.rawText || "";
      if (!rawLine) return;
      const mark = input.checked ? "x" : " ";
      rawLine.textContent = `- [${mark}] ${rawText}`;
      announce(
        `${rawText} marked ${input.checked ? "done" : "not done"} in Markdown.`,
      );
    });
  });
})();

/* ==========================================================================
   5. Dictation — Hold keycap or Alt+Space while in view, auto-demo once
   ========================================================================== */

(() => {
  const section = document.getElementById("chapter-dictate");
  const keycap = document.getElementById("dictate-keycap");
  const textEl = document.getElementById("dictate-text");
  const hudDot = document.getElementById("hud-dot");
  const hudBarsPath = document.querySelector("#hud-bars path");

  if (!section || !keycap || !textEl || !hudBarsPath) return;

  const SENTENCE =
    "Let's keep the May date and I'll send the revised plan tonight.";
  const STATIC_HUD_PATH = hudBarsPath.getAttribute("d") || "";

  let inView = false;
  let listening = false;
  let activeSource = null; // "pointer" | "button-key" | "alt-space" | "demo"
  let demoTimer = 0;
  let demoDone = false;
  let hudPhase = 0;
  let hudAccum = 0;

  function buildHudPath(phase) {
    let d = "";
    for (let i = 0; i < 9; i++) {
      const x = 2 + i * 6;
      const env = 1 - Math.abs(i - 4) * 0.14;
      const wave =
        0.5 + 0.5 * Math.sin(phase * 0.24 + i * 0.95) * Math.cos(phase * 0.11 + i);
      const h = Math.max(2, Math.min(14, Math.round((2 + wave * 12) * env)));
      const y = Math.round(8 - h / 2);
      d += `M${x} ${y}v${h}`;
    }
    return d;
  }

  function startListening(source) {
    if (listening) return;
    if (source !== "demo") {
      window.clearTimeout(demoTimer);
      demoDone = true;
    }

    listening = true;
    activeSource = source;
    scheduler.remove("dictate-type");
    keycap.classList.add("is-pressed");
    if (hudDot) hudDot.classList.toggle("is-pulsing", !prefersReducedMotion());

    if (prefersReducedMotion()) {
      textEl.textContent = "";
      return;
    }

    textEl.textContent = "";
    hudAccum = 0;
    scheduler.add("dictate-hud", (_now, dt) => {
      if (!listening) return false;
      hudAccum += dt;
      if (hudAccum >= 55) {
        hudAccum = 0;
        hudPhase += 1;
        hudBarsPath.setAttribute("d", buildHudPath(hudPhase));
      }
      return true;
    });
  }

  function releaseListening(source) {
    if (!listening) return;
    if (source && activeSource && source !== activeSource && source !== "any") {
      return;
    }

    listening = false;
    activeSource = null;
    keycap.classList.remove("is-pressed");
    if (hudDot) hudDot.classList.remove("is-pulsing");
    scheduler.remove("dictate-hud");
    hudBarsPath.setAttribute("d", STATIC_HUD_PATH);

    if (prefersReducedMotion()) {
      textEl.textContent = SENTENCE;
      return;
    }

    const start = performance.now();
    const charMs = 14;
    scheduler.add("dictate-type", (now) => {
      const count = Math.min(
        SENTENCE.length,
        Math.floor((now - start) / charMs),
      );
      textEl.textContent = SENTENCE.slice(0, count);
      if (count >= SENTENCE.length) {
        textEl.textContent = SENTENCE;
        return false;
      }
      return true;
    });
  }

  // Pointer press & hold on keycap
  keycap.addEventListener("pointerdown", (e) => {
    if (e.button !== 0 && e.pointerType === "mouse") return;
    try {
      keycap.setPointerCapture(e.pointerId);
    } catch (_) {}
    startListening("pointer");
  });

  keycap.addEventListener("pointerup", () => releaseListening("pointer"));
  keycap.addEventListener("pointercancel", () => releaseListening("pointer"));

  // Keyboard press & hold when keycap is focused
  keycap.addEventListener("keydown", (e) => {
    if (e.key === " " || e.key === "Enter") {
      e.preventDefault();
      if (!e.repeat) startListening("button-key");
    }
  });

  keycap.addEventListener("keyup", (e) => {
    if (e.key === " " || e.key === "Enter") {
      e.preventDefault();
      releaseListening("button-key");
    }
  });

  keycap.addEventListener("blur", () => {
    if (activeSource === "button-key") releaseListening("button-key");
  });

  // Global Alt+Space while section is in view (never preventDefault outside this)
  window.addEventListener("keydown", (e) => {
    if (!inView) return;
    const isSpace = e.code === "Space" || e.key === " ";
    if (e.altKey && isSpace && !e.ctrlKey && !e.metaKey) {
      e.preventDefault();
      if (!e.repeat) startListening("alt-space");
    }
  });

  window.addEventListener("keyup", (e) => {
    if (activeSource !== "alt-space") return;
    if (e.code === "Space" || e.key === " " || e.key === "Alt") {
      releaseListening("alt-space");
    }
  });

  window.addEventListener("blur", () => {
    if (listening) releaseListening("any");
  });

  // Track visibility & run auto-demo once on first view
  const obs = new IntersectionObserver(
    (entries) => {
      inView = entries[0].isIntersecting;
      if (!inView && activeSource === "alt-space") {
        releaseListening("alt-space");
      }
      if (inView && !demoDone && !prefersReducedMotion()) {
        demoDone = true;
        demoTimer = window.setTimeout(() => {
          if (!inView || listening || prefersReducedMotion()) return;
          startListening("demo");
          demoTimer = window.setTimeout(() => {
            releaseListening("demo");
          }, 850);
        }, 320);
      }
    },
    { threshold: 0.35 },
  );
  obs.observe(section);
})();

/* ==========================================================================
   6. Closer — Checkbox ticks, strikes title, sets DONE, copies OS command
   ========================================================================== */

(() => {
  const toggle = document.getElementById("closer-toggle");
  const meta = document.getElementById("closer-meta");
  if (!toggle || !meta) return;

  const DEFAULT_META = "OWNER: YOU · DUE: BEFORE YOUR NEXT CALL";

  function detectOsCommand() {
    const ua = navigator.userAgent || "";
    const platform = navigator.platform || "";
    const isWindows = /Win/i.test(platform) || /Windows/i.test(ua);
    return isWindows ? INSTALL_COMMANDS.windows.cmd : INSTALL_COMMANDS.unix.cmd;
  }

  toggle.addEventListener("click", async () => {
    const nextChecked = toggle.getAttribute("aria-checked") !== "true";
    toggle.setAttribute("aria-checked", String(nextChecked));

    if (!nextChecked) {
      meta.textContent = DEFAULT_META;
      announce("Install Minutae unchecked.");
      return;
    }

    meta.textContent = "OWNER: YOU · DONE";
    const cmd = detectOsCommand();
    const copied = await copyText(cmd);
    if (toggle.getAttribute("aria-checked") !== "true") return;
    if (copied) {
      meta.textContent = "OWNER: YOU · DONE · INSTALL COMMAND COPIED";
      announce("Install Minutae marked done. Install command copied to clipboard.");
    } else {
      meta.textContent = "OWNER: YOU · DONE · COPY COMMAND ABOVE";
      announce("Install Minutae marked done.");
    }
  });
})();

/* ==========================================================================
   7. Transport — Docks after hero, tracks scroll progress, plays the call
   ========================================================================== */

(() => {
  const slot = document.getElementById("transport-slot");
  const bar = document.getElementById("transport");
  const playBtn = document.getElementById("transport-play");
  const timeEl = document.getElementById("transport-time");
  const segments = Array.from(document.querySelectorAll(".transport-seg"));
  const closer = document.getElementById("chapter-closer");

  if (!slot || !bar || !playBtn || !timeEl || !segments.length) return;

  const targets = segments.map((seg) => ({
    seg,
    el: document.getElementById(seg.dataset.segTarget),
    time: seg.dataset.segTime || "00:00",
  }));

  let playing = false;
  let scrollRafPending = false;

  function updateTransport() {
    scrollRafPending = false;

    // Dock to bottom once the hero transport slot scrolls above the top edge
    const slotRect = slot.getBoundingClientRect();
    const shouldDock = slotRect.bottom <= 0;
    bar.classList.toggle("is-docked", shouldDock);

    // Compute per-segment progress based on scroll position
    const scrollY = window.scrollY;
    const anchorY = scrollY + window.innerHeight * 0.35;
    const maxScroll = Math.max(
      1,
      document.documentElement.scrollHeight - window.innerHeight,
    );

    const offsets = targets.map((t, idx) =>
      idx === 0 || !t.el ? 0 : t.el.offsetTop,
    );
    const closerTop = closer
      ? closer.offsetTop
      : document.documentElement.scrollHeight;

    let activeIdx = 0;
    for (let i = 0; i < offsets.length; i++) {
      if (anchorY >= offsets[i]) activeIdx = i;
    }

    // At the very top of page, keep "start" active and filled as drawn
    const atTop = scrollY < 16;

    targets.forEach((t, i) => {
      let progress = 0;
      if (atTop) {
        progress = i === 0 ? 1 : 0;
      } else {
        const startY = offsets[i];
        const endY = i < offsets.length - 1 ? offsets[i + 1] : closerTop;
        const span = Math.max(1, endY - startY);
        if (anchorY >= endY) progress = 1;
        else if (anchorY <= startY) progress = 0;
        else progress = (anchorY - startY) / span;
      }

      t.seg.style.setProperty("--progress", progress.toFixed(4));
      t.seg.classList.toggle("is-active", i === activeIdx);
    });

    const atBottom = scrollY >= maxScroll - 24;
    const displayTime = atBottom ? "33:00" : targets[activeIdx].time;
    if (timeEl.textContent !== displayTime) {
      timeEl.textContent = displayTime;
    }
  }

  function scheduleUpdate() {
    if (scrollRafPending) return;
    scrollRafPending = true;
    requestAnimationFrame(updateTransport);
  }

  window.addEventListener("scroll", scheduleUpdate, { passive: true });
  window.addEventListener("resize", scheduleUpdate, { passive: true });
  updateTransport();

  // Segment click scrolls smoothly (or instantly under reduced motion)
  targets.forEach(({ seg, el }) => {
    seg.addEventListener("click", () => {
      stopPlaying();
      if (!el) return;
      if (seg.dataset.segTarget === "chapter-start") {
        window.scrollTo({
          top: 0,
          behavior: prefersReducedMotion() ? "auto" : "smooth",
        });
      } else {
        el.scrollIntoView({
          behavior: prefersReducedMotion() ? "auto" : "smooth",
          block: "start",
        });
      }
    });
  });

  function stopPlaying() {
    if (!playing) return;
    playing = false;
    playBtn.setAttribute("aria-pressed", "false");
    playBtn.setAttribute("aria-label", "Play through the call");
    scheduler.remove("transport-play");
  }

  function startPlaying() {
    const maxScroll =
      document.documentElement.scrollHeight - window.innerHeight;
    if (maxScroll <= 0) return;

    if (window.scrollY >= maxScroll - 8) {
      window.scrollTo({ top: 0, behavior: "auto" });
    }

    if (prefersReducedMotion()) {
      // Jump to next chapter cleanly without smooth auto-scroll loop
      const curY = window.scrollY + 40;
      const next = targets.find((t) => t.el && t.el.offsetTop > curY);
      if (next && next.el) {
        window.scrollTo({ top: next.el.offsetTop, behavior: "auto" });
      } else {
        window.scrollTo({ top: maxScroll, behavior: "auto" });
      }
      return;
    }

    playing = true;
    playBtn.setAttribute("aria-pressed", "true");
    playBtn.setAttribute("aria-label", "Pause call walkthrough");
    announce("Playing through the call.");

    const speedPxPerSec = 175;
    let exactY = window.scrollY;

    scheduler.add("transport-play", (_now, dt) => {
      if (!playing) return false;
      const limit = document.documentElement.scrollHeight - window.innerHeight;
      exactY = Math.min(limit, exactY + (speedPxPerSec * dt) / 1000);
      window.scrollTo({ top: exactY, behavior: "auto" });
      if (exactY >= limit - 1) {
        stopPlaying();
        announce("Reached the end of the call.");
        return false;
      }
      return true;
    });
  }

  playBtn.addEventListener("click", () => {
    if (playing) stopPlaying();
    else startPlaying();
  });

  // Any user wheel, touch, or navigation key cancels auto-play immediately
  window.addEventListener("wheel", stopPlaying, { passive: true });
  window.addEventListener("touchstart", stopPlaying, { passive: true });
  window.addEventListener("keydown", (e) => {
    if (
      playing &&
      [
        "ArrowUp",
        "ArrowDown",
        "PageUp",
        "PageDown",
        "Home",
        "End",
        " ",
        "Escape",
      ].includes(e.key)
    ) {
      stopPlaying();
    }
  });
})();
