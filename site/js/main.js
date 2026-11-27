// Demos load as their section nears. If one fails, its static HTML is still complete.

import installTabs from "./install.js";
import { whenVisible } from "./motion.js";
import themeSwitcher from "./theme.js";
import transport from "./transport.js";

themeSwitcher();
installTabs();
transport();

const demos = [
  ["#hero-tape", () => import("./demos/tape.js")],
  [".chapter-time-display, #speaker-list", () => import("./demos/counters.js")],
  ["#prompt-card", () => import("./demos/prompt.js")],
  ["#file-viewer", () => import("./demos/file-viewer.js")],
  ["#chapter-dictate", () => import("./demos/dictate.js")],
  ["#chapter-closer", () => import("./demos/closer.js")],
];

for (const [selector, load] of demos) {
  whenVisible(
    [...document.querySelectorAll(selector)],
    () => load().then((demo) => demo.default()),
    { rootMargin: "50% 0px" },
  );
}
