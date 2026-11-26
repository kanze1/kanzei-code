import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke, setIpcTransport, type IpcTransport } from "./ipc/client";
import { installEventBridge } from "./ipc/events";
import { App } from "./App";
import { appStore } from "./store/appStore";
import { chatStore, loadOlderMessages } from "./store/chatStore";
import { drawerStore } from "./store/drawerStore";
import { sessionStore } from "./store/sessionStore";
import { uiStore } from "./store/uiStore";
import { MarkdownView } from "./components/MarkdownView";

function createMockTransport(overrides?: Partial<Record<string, (args?: Record<string, unknown>) => unknown>>) {
  const listeners = new Map<string, Set<(payload: unknown) => void>>();
  const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];

  const transport: IpcTransport = {
    async invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
      calls.push({ command, args });
      if (overrides && command in overrides && overrides[command]) {
        return overrides[command]!(args) as T;
      }
      switch (command) {
        case "app_bootstrap":
          return {
            appInfo: { version: "0.1.0", build: "test" },
            uiPrefs: { theme: "dark", ui_layout: {} },
            projects: {
              projects: ["C:/demo"],
              current: "C:/demo",
              names: { "C:/demo": "Demo Project" },
            },
            generalChatRoot: null,
            composerDrafts: { items: [], error: null },
            target: { kind: "project", projectDir: "C:/demo" },
          } as T;
        case "process_list":
          return [
            {
              id: "d|C:/demo",
              origin_project: "C:/demo",
              project_dir: "C:/demo",
              worktree_path: null,
              branch: "main",
              session_id: "ses-demo",
              model: null,
              profile: "dev-auto",
              research_topic: null,
              reasoning: null,
              manual_models: [],
              subagent_mode: "auto",
              tracker_writes: false,
              stage: "空闲",
              running: false,
              label: "主对话",
              title: null,
              title_custom: false,
              kind: "main",
              ordinal: null,
              updated_at: 1000,
            },
          ] as T;
        case "conversation_page_messages": {
          const beforeIndex = args?.beforeIndex as number | undefined;
          if (beforeIndex === 2) {
            return {
              items: [
                { role: "user", parts: [{ type: "text", text: "更早消息 0" }] },
                { role: "assistant", parts: [{ type: "text", text: "更早回复 1" }] },
              ],
              total: 4,
              startIndex: 0,
              endIndex: 2,
              hasMore: false,
            } as T;
          }
          return {
            items: [
              { role: "user", parts: [{ type: "text", text: "最新提问" }] },
              { role: "assistant", parts: [{ type: "text", text: "查看 `src/main.rs:42` 获取详情" }] },
            ],
            total: 4,
            startIndex: 2,
            endIndex: 4,
            hasMore: true,
          } as T;
        }
        case "file_preview":
          return {
            path: args?.path ?? "src/main.rs",
            content: "fn main() {\n    println!(\"hello\");\n}\n",
            hash: "abc",
          } as T;
        case "file_git_diff":
          return { path: args?.path ?? "src/main.rs", diff: "" } as T;
        case "git_status":
          return { repo: "own", branch: "main", changes: 0, last: null, additions: 0, deletions: 0, files: [] } as T;
        case "workspace_overview":
          return { current: "C:/demo", projects: [], observed_at: 0 } as T;
        case "models_list":
          return { models: [{ id: "openai:gpt-5", label: "openai:gpt-5" }], errors: [] } as T;
        case "open_tools_list":
        case "list_pending_inputs":
        case "pending_asks_get":
          return [] as T;
        case "model_effective":
          return { model: { value: "openai:gpt-5", source: "global" } } as T;
        case "auto_state_get":
          return { enabled: false, paused: false, goal: null } as T;
        case "projects_select":
          return {
            projects: ["C:/demo"],
            current: (args?.path as string) ?? "C:/demo",
            names: { "C:/demo": "Demo Project" },
          } as T;
        case "app_shutdown":
          return { stopped: true, state: "quiescent" } as T;
        default:
          return null as T;
      }
    },
    async listen<T>(event: string, handler: (payload: T) => void): Promise<() => void> {
      if (!listeners.has(event)) listeners.set(event, new Set());
      listeners.get(event)!.add(handler as (payload: unknown) => void);
      return () => {
        listeners.get(event)?.delete(handler as (payload: unknown) => void);
      };
    },
  };

  const emit = (event: string, payload: unknown) => {
    for (const fn of listeners.get(event) ?? []) {
      fn(payload);
    }
  };

  return { transport, calls, emit };
}

afterEach(() => {
  cleanup();
  setIpcTransport(null);
  appStore.setState({
    bootstrapped: false,
    activeProjectDir: null,
    generalMode: false,
    projects: { projects: [], current: null, names: {} },
  });
  sessionStore.setState({ processes: [], activeProcessId: null, projectDir: null, asks: [] });
  chatStore.setState({
    sessionId: null,
    messages: [],
    streamingText: "",
    streamingReasoning: "",
    liveTools: [],
    liveSubagents: [],
  });
  drawerStore.setState({ open: false, activeFile: null, editDirty: false });
  uiStore.setState({ view: "chat", confirmDialog: null, closingWindow: false });
});

describe("Kanzei 新前端架构核心契约", () => {
  it("IPC 客户端严格校验参数契约，拒绝未声明键与缺失必填参数", async () => {
    const { transport } = createMockTransport();
    setIpcTransport(transport);

    await expect(
      invoke("process_rename", { projectDir: "C:/demo" } as never)
    ).rejects.toThrow(/缺少必填参数/);

    await expect(
      invoke("app_bootstrap", { unexpectedKey: 1 } as never)
    ).rejects.toThrow(/未在契约中声明/);
  });

  it("应用引导渲染窗口控制三键、侧栏、尾窗分页对话并支持向上翻页", async () => {
    const { transport, calls } = createMockTransport();
    setIpcTransport(transport);

    render(<App />);

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "最小化" })).toBeTruthy();
      expect(screen.getByRole("button", { name: "最大化" })).toBeTruthy();
      expect(screen.getByRole("button", { name: "关闭" })).toBeTruthy();
    });

    await waitFor(() => {
      expect(chatStore.getState().messages.length).toBe(2);
      expect(chatStore.getState().hasMore).toBe(true);
    });

    const added = await loadOlderMessages();
    expect(added).toBe(2);
    expect(chatStore.getState().messages.length).toBe(4);
    expect(chatStore.getState().hasMore).toBe(false);
    expect(calls.some((c) => c.command === "app_bootstrap")).toBe(true);
  });

  it("Markdown 文件路径链接点击后直接在右侧上下文抽屉打开并高亮行号", async () => {
    const { transport } = createMockTransport();
    setIpcTransport(transport);
    drawerStore.setState({ projectDir: "C:/demo" });

    render(<MarkdownView text="请检查 `src/main.rs:42` 中的实现。" />);
    const link = screen.getByRole("button", { name: "src/main.rs:42" });
    fireEvent.click(link);

    await waitFor(() => {
      expect(drawerStore.getState().open).toBe(true);
      expect(drawerStore.getState().activeFile).toBe("src/main.rs");
      expect(drawerStore.getState().highlightLine).toBe(42);
    });
  });

  it("流式事件按 sessionId 严格隔离，非当前会话不会污染主界面", async () => {
    const { transport, emit } = createMockTransport();
    setIpcTransport(transport);
    const dispose = await installEventBridge();

    chatStore.setState({ sessionId: "ses-active", streamingText: "" });
    emit("kz:text", { sessionId: "ses-other", text: "不该出现" });
    emit("kz:text", { sessionId: "ses-active", text: "正确增量" });

    expect(chatStore.getState().streamingText).toBe("正确增量");
    dispose();
  });

  it("运行中点击关闭窗口触发保护弹窗，确认后执行单进程 app_shutdown", async () => {
    const closeSpy = vi.fn().mockResolvedValue(undefined);
    window.__TAURI__ = {
      window: {
        getCurrentWindow: () => ({
          minimize: vi.fn().mockResolvedValue(undefined),
          toggleMaximize: vi.fn().mockResolvedValue(undefined),
          isMaximized: vi.fn().mockResolvedValue(false),
          startDragging: vi.fn().mockResolvedValue(undefined),
          close: closeSpy,
          onResized: vi.fn().mockResolvedValue(() => {}),
        }),
      },
    };
    const { transport, calls } = createMockTransport({
      process_list: () => [
        {
          id: "d|C:/demo",
          origin_project: "C:/demo",
          project_dir: "C:/demo",
          worktree_path: null,
          branch: "main",
          session_id: "ses-demo",
          model: null,
          profile: "dev-auto",
          research_topic: null,
          reasoning: null,
          manual_models: [],
          subagent_mode: "auto",
          tracker_writes: false,
          stage: "运行中",
          running: true,
          label: "主对话",
          title: null,
          title_custom: false,
          kind: "main",
          ordinal: null,
          updated_at: 1000,
        },
      ],
    });
    setIpcTransport(transport);

    render(<App />);
    await waitFor(() => expect(sessionStore.getState().processes[0]?.running).toBe(true));

    fireEvent.click(screen.getByRole("button", { name: "关闭" }));
    await waitFor(() => expect(screen.getByText("确认退出 kanzei？")).toBeTruthy());

    fireEvent.click(screen.getByRole("button", { name: "停止并退出" }));
    await waitFor(() => {
      expect(calls.some((c) => c.command === "app_shutdown")).toBe(true);
      expect(closeSpy).toHaveBeenCalled();
    });
    delete window.__TAURI__;
  });
});
