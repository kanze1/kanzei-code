// Shared external-tool menu entries for projects, conversations and files.
import { t } from "./02-i18n.js";
import { navigate_view } from "./03-shell.js";
import { fillTemplate } from "./04-structured-parse.js";

export function openToolItems(tools, run) {
  const usable = tools.filter(tool => tool.available !== false);
  if (!usable.length) return [{
    label: `${t("在设置里添加打开方式")}…`,
    onSelect: () => {
      navigate_view("settings");
      document.dispatchEvent(new CustomEvent("kz:open-settings-section", { detail: { id: "sg-open-tools" } }));
    },
  }];
  return usable.map(tool => ({
    label: fillTemplate(t("用 {tool} 打开"), { tool: tool.builtin ? t(tool.label) : tool.label || tool.id }),
    onSelect: () => run(tool),
  }));
}
