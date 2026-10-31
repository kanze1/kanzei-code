// Navigation metadata is independent of the navigation controls which render it.
export const views = Object.freeze({
  chat: { label: "对话", root: true }, workspace: { label: "所有项目", scroller: "workspace-scroll" },
  project: { label: "项目管理", scroller: "view-project", barless: true }, lines: { label: "并行线路", scroller: "lines-scroll" },
  documents: { label: "需求", scroller: "documents-scroll" }, memory: { label: "记忆", scroller: "memory-scroll" },
  files: { label: "文件" }, arch: { label: "架构", scroller: "arch-scroll" }, metrics: { label: "运行画像", scroller: "metrics-scroll" },
  settings: { label: "设置", scroller: "settings-scroll" }, skills: { label: "Skills", scroller: "skills-scroll" },
});
