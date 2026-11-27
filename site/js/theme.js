import { reducedMotion } from "./motion.js";
import { choiceGroup, markSelected } from "./ui.js";

export default function themeSwitcher() {
  const root = document.documentElement;
  const options = [...document.querySelectorAll("[data-theme-choice]")];
  const systemDark = matchMedia("(prefers-color-scheme: dark)");

  // The latest choice, which a view transition may not have applied yet.
  let chosen = options.find(
    (o) => o.dataset.themeChoice === (root.dataset.theme ?? "system"),
  );
  let revealing = false;

  function looksDark({ dataset: { themeChoice } }) {
    return themeChoice === "system"
      ? systemDark.matches
      : themeChoice === "dark";
  }

  function apply() {
    const choice = chosen.dataset.themeChoice;
    if (choice === "system") delete root.dataset.theme;
    else root.dataset.theme = choice;
    try {
      localStorage.setItem("minutae-theme", choice);
    } catch {
      // Storage is blocked: the choice lasts for this visit.
    }
    markSelected(options, chosen, "aria-checked");
  }

  function choose(option) {
    const instant =
      !document.startViewTransition ||
      reducedMotion.matches ||
      revealing || // Chromium can crash when one reveal interrupts another
      looksDark(option) === looksDark(chosen);
    chosen = option;
    if (instant) return apply();

    const { left, top, width, height } = option.getBoundingClientRect();
    const x = left + width / 2;
    const y = top + height / 2;
    const radius = Math.hypot(
      Math.max(x, innerWidth - x),
      Math.max(y, innerHeight - y),
    );

    revealing = true;
    const transition = document.startViewTransition(apply);
    transition.ready
      .then(() =>
        root.animate(
          {
            clipPath: [0, radius].map((r) => `circle(${r}px at ${x}px ${y}px)`),
          },
          {
            duration: 320,
            easing: "cubic-bezier(0.2, 0.8, 0.2, 1)",
            pseudoElement: "::view-transition-new(root)",
          },
        ),
      )
      .catch(() => {}); // skipped, but applied
    transition.finished.finally(() => (revealing = false));
  }

  markSelected(options, chosen, "aria-checked");
  choiceGroup(options, choose, { vertical: true });
}
