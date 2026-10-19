import { defer } from "./00-dom.js";
import { toast } from "./00-surface.js";
import { t } from "./02-i18n.js";

export const COPY_ICON = '<svg viewBox="0 0 16 16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round"><rect x="5.5" y="5.5" width="8" height="8" rx="1.6"/><path d="M10.5 5.5V3.6A1.1 1.1 0 0 0 9.4 2.5H3.6a1.1 1.1 0 0 0-1.1 1.1v5.8a1.1 1.1 0 0 0 1.1 1.1h1.9"/></svg>';

function setCopyLabel(button, key) {
  button.title = t(key);
  button.dataset.i18nTitle = key;
  button.setAttribute("aria-label", t(key));
  button.dataset.i18nAriaLabel = key;
  const label = button.querySelector(".copy-label");
  label.textContent = t(button.dataset.copied ? "已复制" : "复制");
  label.dataset.i18nKey = button.dataset.copied ? "已复制" : "复制";
}

function resetCopyButton(button) {
  delete button.dataset.copied;
  setCopyLabel(button, button.dataset.copyLabel);
}

export function copyButton(labelKey = "复制消息") {
  const button = document.createElement("button");
  button.className = "copy-btn";
  button.type = "button";
  button.dataset.copyLabel = labelKey;
  button.innerHTML = COPY_ICON;
  const label = document.createElement("span");
  label.className = "copy-label sr-only";
  button.append(label);
  resetCopyButton(button);
  return button;
}

// Callers own the error destination; success is shown only after the clipboard confirms the write.
export async function writeClipboardText(text, button = null) {
  if (button?.getAttribute("aria-busy") === "true") return;
  if (!text) { toast(t("没有可复制的内容")); return; }
  if (button) {
    resetCopyButton(button);
    button.setAttribute("aria-busy", "true");
    button.setAttribute("aria-disabled", "true");
  }
  try {
    await navigator.clipboard.writeText(text);
  } finally {
    if (button) {
      button.removeAttribute("aria-busy");
      button.removeAttribute("aria-disabled");
    }
  }
  if (button) {
    button.dataset.copied = "true";
    setCopyLabel(button, "已复制");
  }
  toast(t("已复制"), { kind: "ok" });
}

// Delegation also covers history restored from cached HTML, which has no per-node listeners.
defer(() => {
  for (const type of ["pointerout", "focusout"]) {
    document.addEventListener(type, (event) => {
      const button = event.target.closest?.(".copy-btn");
      if (button && !button.contains(event.relatedTarget)) resetCopyButton(button);
    });
  }
});
