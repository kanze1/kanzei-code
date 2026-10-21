import { copyButton, writeClipboardText } from "./00-clipboard.js";
import { defer } from "./00-dom.js";
import { toast } from "./00-surface.js";
import { t } from "./02-i18n.js";
import { addMarkdownHook } from "./04-markdown.js";

// Read the rendered Markdown structure, keeping text boundaries and excluding UI decorations.
function quoteText(root) {
  const read = (node) => {
    if (node.nodeType === Node.TEXT_NODE) return node.textContent;
    if (node.nodeType !== Node.ELEMENT_NODE || node.matches('[aria-hidden="true"], button, .md-quote-actions, .md-code-toolbar')) return "";
    if (node.tagName === "BR") return "\n";
    if (node.matches(".md-math")) return node.dataset.mathSource + (node.classList.contains("md-math-display") ? "\n\n" : "");
    if (node.tagName === "PRE") return `${node.textContent}\n\n`;
    if (node.matches("ul, ol")) {
      return [...node.children].map((item, index) => {
        const body = [...item.childNodes].filter(child => !child.matches?.("ul, ol")).map(read).join("").replace(/\n\n$/, "");
        const marker = item.classList.contains("md-task-item") ? "" : node.tagName === "OL" ? `${node.start + index}. ` : "- ";
        const nested = [...item.children].filter(child => child.matches("ul, ol")).map(child => read(child).replace(/\n\n$/, "").split("\n").map(line => `  ${line}`).join("\n"));
        return `${marker}${body}${nested.length ? `\n${nested.join("\n")}` : ""}`;
      }).join("\n") + "\n\n";
    }
    if (node.tagName === "TABLE") return [...node.rows].map(row => [...row.cells].map(read).join("\t")).join("\n") + "\n\n";
    const text = [...node.childNodes].map(read).join("");
    return node.matches("p, h1, h2, h3, h4, h5, h6") ? `${text}\n\n` : text;
  };
  return read(root).replace(/\n\n$/, "");
}

defer(() => {
  addMarkdownHook((root) => {
    for (const quote of root.querySelectorAll(".md-quote")) {
      if (quote.querySelector(":scope > .md-quote-actions")) continue;
      const actions = document.createElement("div");
      actions.className = "md-quote-actions";
      const copy = copyButton("复制引用");
      copy.classList.add("quote-copy");
      copy.querySelector(".copy-label").classList.remove("sr-only");
      actions.append(copy);
      quote.prepend(actions);
    }
  });
  // History restored from cached HTML shares the same copy scope and feedback lifecycle.
  document.addEventListener("click", (event) => {
    const button = event.target.closest?.(".quote-copy");
    if (!button) return;
    const content = button.closest(".md-quote").querySelector(":scope > .md-quote-content");
    void writeClipboardText(quoteText(content), button).catch(error => toast(`${t("复制失败")}:${error}`, { kind: "err" }));
  });
});
