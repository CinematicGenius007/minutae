import { reducedMotion, whenVisible } from "../motion.js";
import { announce } from "../ui.js";

function revealTree(tree) {
  // The new Markdown file appears last.
  const lines = [...tree.children].sort(
    (a, b) => a.classList.contains("is-new") - b.classList.contains("is-new"),
  );
  lines.forEach((line, i) =>
    line.animate(
      { opacity: [0, 1], transform: ["translateY(4px)", "none"] },
      {
        delay: i * 65 + (i === lines.length - 1 ? 110 : 0),
        duration: 200,
        easing: "cubic-bezier(0.2, 0.8, 0.2, 1)",
        fill: "backwards",
      },
    ),
  );
}

export default function fileViewer() {
  const viewer = document.getElementById("file-viewer");
  const tree = viewer.querySelector(".file-tree");
  whenVisible(
    [tree],
    () => {
      if (!reducedMotion.matches) revealTree(tree);
    },
    { threshold: 0.05 },
  );

  for (const checkbox of viewer.querySelectorAll(".read-check-input")) {
    const mark = document.getElementById(checkbox.dataset.mark);
    const sync = () => (mark.textContent = checkbox.checked ? "x" : " ");

    checkbox.addEventListener("change", () => {
      sync();
      const task = mark.parentElement.textContent.slice("- [ ] ".length);
      const state = checkbox.checked ? "done" : "not done";
      announce(`${task} marked ${state} in Markdown.`);
    });
    sync(); // may already be ticked
  }
}
