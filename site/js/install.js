import { announce, choiceGroup, copyText, markSelected } from "./ui.js";

export default function installTabs() {
  const block = document.getElementById("install");
  const tabs = [...block.querySelectorAll('[role="tab"]')];
  const notes = [...block.querySelectorAll(".install-note")];
  const panel = document.getElementById("install-panel");
  const prompt = block.querySelector(".install-prompt");
  const command = block.querySelector(".install-command");
  const copy = block.querySelector(".copy-btn");
  let resetLabel = 0;

  choiceGroup(tabs, (tab) => {
    markSelected(tabs, tab, "aria-selected");
    panel.setAttribute("aria-labelledby", tab.id);
    prompt.hidden = "noPrompt" in tab.dataset; // a sentence, not a shell command
    command.textContent = tab.dataset.command;
    command.scrollLeft = 0;
    for (const note of notes) note.hidden = note.id !== tab.dataset.note;
  });

  copy.addEventListener("click", async () => {
    const copied = await copyText(command.textContent);
    if (!copied) getSelection().selectAllChildren(command);
    copy.textContent = copied ? "copied" : "select";
    announce(
      copied
        ? "Install command copied to clipboard."
        : "Could not copy automatically. Command selected.",
    );
    clearTimeout(resetLabel);
    resetLabel = setTimeout(() => (copy.textContent = "copy"), 1600);
  });
}
