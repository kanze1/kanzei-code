import { copyButton, writeClipboardText } from "./00-clipboard.js";
import { defer } from "./00-dom.js";
import { toast } from "./00-surface.js";
import { t } from "./02-i18n.js";
import { addMarkdownHook } from "./04-markdown.js";

// Copy and preview share a toolbar outside the scrollable source, so neither can cover code.
export function codeBlockActions(pre) {
  if (pre.parentElement.classList.contains("md-code-block")) return pre.parentElement.querySelector(".md-code-actions");
  const block = document.createElement("div");
  block.className = "md-code-block";
  const toolbar = document.createElement("div");
  toolbar.className = "md-code-toolbar";
  const language = document.createElement("span");
  language.className = "md-code-language";
  const name = pre.querySelector("code").className.match(/language-([\w+-]+)/)?.[1];
  language.textContent = name || t("代码");
  if (!name) language.dataset.i18nKey = "代码";
  const actions = document.createElement("div");
  actions.className = "md-code-actions";
  const copy = copyButton("复制代码");
  copy.classList.add("code-copy");
  copy.querySelector(".copy-label").classList.remove("sr-only");
  actions.append(copy);
  toolbar.append(language, actions);
  pre.before(block);
  block.append(toolbar, pre);
  return actions;
}

defer(() => {
  addMarkdownHook((root) => {
    for (const pre of root.querySelectorAll("pre.code")) codeBlockActions(pre);
  });
  document.addEventListener("click", (event) => {
    const button = event.target.closest?.(".code-copy");
    if (!button) return;
    const source = button.closest(".md-code-block").querySelector("pre.code > code").textContent;
    void writeClipboardText(source, button).catch((error) => toast(`${t("复制失败")}:${error}`, { kind: "err" }));
  });
});
