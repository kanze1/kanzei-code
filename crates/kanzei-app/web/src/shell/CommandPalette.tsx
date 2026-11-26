import React, { useEffect, useMemo, useState } from "react";
import { addExistingProject, appStore, openGeneralChat, useAppStore } from "../store/appStore";
import { chatStore, clearCurrentConversation, compactCurrentConversation } from "../store/chatStore";
import { toggleDrawer } from "../store/drawerStore";
import { createConversation } from "../store/sessionStore";
import {
  navigateBack,
  navigateView,
  openBtwOverlay,
  openProjectModelsDialog,
  openTraceDialog,
  toggleSidebar,
  toggleTheme,
  triggerContextInspector,
  uiStore,
  useUiStore,
} from "../store/uiStore";
import { IconSearch } from "../components/Icons";

interface CommandAction {
  id: string;
  group: string;
  label: string;
  shortcut?: string;
  run: () => void;
}

export function CommandPalette(): React.ReactElement | null {
  const open = useUiStore((s) => s.paletteOpen);
  const projectDir = useAppStore((s) => s.activeProjectDir);
  const generalMode = useAppStore((s) => s.generalMode);
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);

  const actions = useMemo<CommandAction[]>(
    () => [
      {
        id: "new-chat",
        group: "对话",
        label: "新建对话",
        shortcut: "Ctrl+Shift+N",
        run: () => {
          if (projectDir) void createConversation(projectDir);
          else void openGeneralChat();
        },
      },
      {
        id: "general-chat",
        group: "对话",
        label: "切换到无项目对话",
        run: () => void openGeneralChat(),
      },
      {
        id: "clear-chat",
        group: "对话",
        label: "清空当前对话（开启新段）",
        shortcut: "Ctrl+Shift+C",
        run: () => void clearCurrentConversation(),
      },
      {
        id: "compact-chat",
        group: "对话",
        label: "压缩当前对话上下文",
        run: () => void compactCurrentConversation(),
      },
      ...(!generalMode && projectDir
        ? [
            {
              id: "toggle-files-drawer",
              group: "抽屉",
              label: "开关右侧文件上下文抽屉",
              run: () => toggleDrawer("files"),
            },
          ]
        : []),
      {
        id: "toggle-activity-drawer",
        group: "抽屉",
        label: "开关右侧活动面板",
        run: () => toggleDrawer("activity"),
      },
      {
        id: "open-inbox",
        group: "全局",
        label: "打开待办收件箱（跨项目权限审批与后台完工提醒）",
        run: () => uiStore.setState({ inboxOpen: true }),
      },
      {
        id: "open-btw",
        group: "对话",
        label: "/btw 打开旁路只读提问（不写入主对话历史）",
        run: () => openBtwOverlay(""),
      },
      {
        id: "open-context-breakdown",
        group: "对话",
        label: "/context 查看上下文构成占比拆解",
        run: () => triggerContextInspector(),
      },
      {
        id: "open-trace",
        group: "对话",
        label: "/trace 查看会话执行轨迹与工具耗时",
        run: () => openTraceDialog(),
      },
      {
        id: "add-project",
        group: "项目",
        label: "添加已有项目目录…",
        run: () => void addExistingProject(),
      },
      {
        id: "new-project",
        group: "项目",
        label: "新建项目…",
        run: () => uiStore.setState({ newProjectOpen: true }),
      },
      ...(!generalMode && projectDir
        ? [
            {
              id: "project-models",
              group: "项目",
              label: "配置本项目模型覆盖…",
              run: () => openProjectModelsDialog(projectDir),
            },
          ]
        : []),
      { id: "go-chat", group: "导航", label: "转到：对话工作面", run: () => navigateView("chat") },
      { id: "go-workspace", group: "导航", label: "转到：工作台与 My Quests 任务看板", run: () => navigateView("workspace") },
      { id: "go-wiki", group: "导航", label: "转到：Repo Wiki 仓库架构百科", run: () => navigateView("wiki") },
      { id: "go-memory", group: "导航", label: "转到：记忆与知识图谱", run: () => navigateView("memory") },
      { id: "go-skills", group: "导航", label: "转到：Skills 技能库", run: () => navigateView("skills") },
      { id: "go-settings", group: "导航", label: "转到：全局设置", run: () => navigateView("settings") },
      { id: "toggle-theme", group: "外观", label: "切换明暗主题", run: toggleTheme },
      { id: "toggle-sidebar", group: "外观", label: "收起 / 展开侧栏", run: toggleSidebar },
    ],
    [projectDir, generalMode]
  );

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return actions;
    return actions.filter(
      (a) =>
        a.label.toLowerCase().includes(q) ||
        a.group.toLowerCase().includes(q) ||
        a.id.includes(q)
    );
  }, [actions, query]);

  useEffect(() => {
    if (open) {
      setQuery("");
      setIndex(0);
    }
  }, [open]);

  if (!open) return null;

  const close = () => uiStore.setState({ paletteOpen: false });
  const execute = (action: CommandAction | undefined) => {
    if (!action) return;
    close();
    action.run();
  };

  return (
    <div className="modal-scrim" role="dialog" aria-modal="true" aria-label="命令面板" onClick={close}>
      <div className="palette-panel" onClick={(e) => e.stopPropagation()}>
        <div className="palette-search-row">
          <IconSearch size={15} />
          <input
            autoFocus
            className="palette-input"
            placeholder="输入命令、视图或操作名称…"
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setIndex(0);
            }}
            onKeyDown={(e) => {
              if (e.key === "Escape") close();
              else if (e.key === "ArrowDown") {
                e.preventDefault();
                setIndex((i) => Math.min(filtered.length - 1, i + 1));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setIndex((i) => Math.max(0, i - 1));
              } else if (e.key === "Enter") {
                e.preventDefault();
                execute(filtered[index]);
              }
            }}
          />
          <kbd className="kbd">ESC</kbd>
        </div>
        <div className="palette-list">
          {filtered.map((action, idx) => (
            <button
              key={action.id}
              type="button"
              className={`palette-item${idx === index ? " active" : ""}`}
              onMouseEnter={() => setIndex(idx)}
              onClick={() => execute(action)}
            >
              <span className="palette-item-main">
                <span className="palette-item-group">{action.group}</span>
                <span>{action.label}</span>
              </span>
              {action.shortcut && <kbd className="kbd">{action.shortcut}</kbd>}
            </button>
          ))}
          {filtered.length === 0 && (
            <div className="empty-state" style={{ padding: "24px 12px" }}>
              <p>无匹配命令</p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

export function GlobalShortcuts(): null {
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;
      if (mod && e.key.toLowerCase() === "p") {
        e.preventDefault();
        uiStore.setState((prev) => ({ ...prev, paletteOpen: !prev.paletteOpen }));
        return;
      }
      if (mod && e.key.toLowerCase() === "k") {
        e.preventDefault();
        navigateView("chat");
        document.getElementById("kz-composer-input")?.focus();
        return;
      }
      if (mod && e.shiftKey && e.key.toLowerCase() === "n") {
        e.preventDefault();
        const projectDir = appStore.getState().activeProjectDir;
        if (projectDir) void createConversation(projectDir);
        else void openGeneralChat();
        return;
      }
      if (mod && e.shiftKey && e.key.toLowerCase() === "c") {
        e.preventDefault();
        void clearCurrentConversation();
        return;
      }
      if (mod && e.key.toLowerCase() === "f" && uiStore.getState().view === "chat") {
        e.preventDefault();
        chatStore.setState((prev) => ({ ...prev, searchOpen: !prev.searchOpen }));
        return;
      }
      if (e.altKey && e.key === "ArrowLeft") {
        e.preventDefault();
        navigateBack();
        return;
      }
      if (e.key === "Escape") {
        if (uiStore.getState().paletteOpen) {
          uiStore.setState({ paletteOpen: false });
        } else if (uiStore.getState().viewer) {
          uiStore.setState({ viewer: null });
        } else if (chatStore.getState().searchOpen) {
          chatStore.setState({ searchOpen: false, searchQuery: "" });
        }
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  return null;
}
