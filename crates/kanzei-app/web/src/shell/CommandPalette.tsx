import React, { useEffect, useMemo, useState } from "react";
import { addExistingProject, appStore, openGeneralChat, useAppStore } from "../store/appStore";
import { chatStore, clearCurrentConversation, compactCurrentConversation } from "../store/chatStore";
import { toggleDrawer } from "../store/drawerStore";
import { createConversation } from "../store/sessionStore";
import {
  navigateBack,
  navigateView,
  toggleSidebar,
  toggleTheme,
  uiStore,
  useUiStore,
} from "../store/uiStore";

interface CommandAction {
  id: string;
  label: string;
  shortcut?: string;
  run: () => void;
}

export function CommandPalette(): React.ReactElement | null {
  const open = useUiStore((s) => s.paletteOpen);
  const projectDir = useAppStore((s) => s.activeProjectDir);
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);

  const actions = useMemo<CommandAction[]>(
    () => [
      {
        id: "new-chat",
        label: "新建对话",
        shortcut: "Ctrl+Shift+N",
        run: () => {
          if (projectDir) void createConversation(projectDir);
          else void openGeneralChat();
        },
      },
      {
        id: "general-chat",
        label: "切换到无项目对话",
        run: () => void openGeneralChat(),
      },
      {
        id: "clear-chat",
        label: "清空当前对话（开启新段）",
        shortcut: "Ctrl+Shift+C",
        run: () => void clearCurrentConversation(),
      },
      {
        id: "compact-chat",
        label: "压缩当前对话上下文",
        run: () => void compactCurrentConversation(),
      },
      {
        id: "toggle-files-drawer",
        label: "开关右侧文件上下文抽屉",
        run: () => toggleDrawer("files"),
      },
      {
        id: "toggle-activity-drawer",
        label: "开关右侧活动面板",
        run: () => toggleDrawer("activity"),
      },
      {
        id: "add-project",
        label: "添加已有项目目录…",
        run: () => void addExistingProject(),
      },
      {
        id: "new-project",
        label: "新建项目…",
        run: () => uiStore.setState({ newProjectOpen: true }),
      },
      {
        id: "project-models",
        label: "配置本项目模型覆盖…",
        run: () => uiStore.setState({ projectModelsOpen: true }),
      },
      { id: "go-chat", label: "转到：对话工作面", run: () => navigateView("chat") },
      { id: "go-workspace", label: "转到：工作台项目总览", run: () => navigateView("workspace") },
      { id: "go-memory", label: "转到：记忆与知识图谱", run: () => navigateView("memory") },
      { id: "go-skills", label: "转到：Skills 技能库", run: () => navigateView("skills") },
      { id: "go-settings", label: "转到：全局设置", run: () => navigateView("settings") },
      { id: "toggle-theme", label: "切换明暗主题", run: toggleTheme },
      { id: "toggle-sidebar", label: "收起 / 展开侧栏", run: toggleSidebar },
    ],
    [projectDir]
  );

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return actions;
    return actions.filter((a) => a.label.toLowerCase().includes(q) || a.id.includes(q));
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
        <input
          autoFocus
          className="palette-input"
          placeholder="输入命令或视图名称…"
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
        <div className="palette-list">
          {filtered.map((action, idx) => (
            <button
              key={action.id}
              type="button"
              className={`palette-item${idx === index ? " active" : ""}`}
              onMouseEnter={() => setIndex(idx)}
              onClick={() => execute(action)}
            >
              <span>{action.label}</span>
              {action.shortcut && <span className="palette-shortcut">{action.shortcut}</span>}
            </button>
          ))}
          {filtered.length === 0 && (
            <div style={{ padding: 14, fontSize: 12, color: "var(--dim)" }}>无匹配命令</div>
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
