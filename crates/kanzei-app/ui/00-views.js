// Navigation metadata is independent of the navigation controls which render it.
export const views = Object.freeze({
  chat: { label: "对话", root: true }, workspace: { label: "所有项目", scroller: "workspace-scroll" },
  memory: { label: "记忆", scroller: "memory-scroll" },
  files: { label: "文件" },
  settings: { label: "设置", scroller: "settings-scroll" }, skills: { label: "Skills", scroller: "skills-scroll" },
});
