import { announce, copyText } from "../ui.js";

/**
 * Today that is the source build. Once #install has data-released (see site/README.md),
 * it is the release command.
 */
function installCommand(block) {
  const released = "released" in block.dataset;
  const windows = /Windows/.test(navigator.userAgent);
  const tab = !released ? "source" : windows ? "windows" : "unix";
  const { command, commandWindows } = document.getElementById(
    `tab-${tab}`,
  ).dataset;
  return {
    command: (windows && commandWindows) || command,
    name: released ? "install command" : "source build command",
  };
}

export default function closer() {
  const toggle = document.querySelector(".closer-toggle");
  const meta = document.getElementById("closer-meta");
  const block = document.getElementById("install");
  const due = meta.textContent.trim();
  const checked = () => toggle.getAttribute("aria-checked") === "true";

  toggle.addEventListener("click", async () => {
    toggle.setAttribute("aria-checked", !checked());
    if (!checked()) {
      meta.textContent = due;
      announce("Install Minutae unchecked.");
      return;
    }

    meta.textContent = "OWNER: YOU · DONE";
    const { command, name } = installCommand(block);
    const copied = await copyText(command);
    if (!checked()) return; // unticked while the clipboard was busy

    meta.textContent = copied
      ? `OWNER: YOU · DONE · ${name.toUpperCase()} COPIED`
      : "OWNER: YOU · DONE · COPY FAILED, SEE DOCS";
    announce(
      `Install Minutae marked done.${copied ? ` The ${name} is copied to the clipboard.` : ""}`,
    );
  });
}
