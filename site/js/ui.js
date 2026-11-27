const status = document.getElementById("live-status");

export function announce(message) {
  // Cleared first, so the same message is read again.
  status.textContent = "";
  requestAnimationFrame(() => (status.textContent = message));
}

export function setText(element, text) {
  if (element.textContent !== text) element.textContent = text;
}

export async function copyText(text) {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}

export function markSelected(items, chosen, attribute) {
  for (const item of items) {
    item.setAttribute(attribute, item === chosen);
    item.tabIndex = item === chosen ? 0 : -1;
  }
}

export function choiceGroup(items, onChoose, { vertical = false } = {}) {
  items.forEach((item, index) => {
    item.addEventListener("click", () => onChoose(item));
    item.addEventListener("keydown", (event) => {
      const step = {
        ArrowLeft: -1,
        ArrowRight: 1,
        ...(vertical && { ArrowUp: -1, ArrowDown: 1 }),
        Home: -index,
        End: items.length - 1 - index,
      }[event.key];
      if (step === undefined) return;
      event.preventDefault();
      const target = items.at((index + step) % items.length);
      target.focus();
      onChoose(target);
    });
  });
}
