import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { invoke, setIpcTransport, type IpcTransport } from "./ipc/client";
import { installEventBridge } from "./ipc/events";
import { App } from "./App";
import { ProjectModelsDialogHost, RewindDialogHost } from "./components/Dialogs";
import { ErrorBoundary } from "./components/ErrorBoundary";
import SettingsView from "./views/settings/SettingsView";
import { AskOverlay } from "./views/chat/AskOverlay";
import {
  ActionReportCard,
  buildRoundReportSummary,
  evaluateGateVerdict,
} from "./views/chat/ActionReportCard";
import { BtwOverlay } from "./views/chat/BtwOverlay";
import { Composer } from "./views/chat/Composer";
import {
  applyReverseHunk,
  ContextDrawer,
  parseUnifiedDiffHunks,
} from "./views/chat/ContextDrawer";
import { MessageItem } from "./views/chat/MessageItem";
import MemoryView from "./views/memory/MemoryView";
import SkillsView from "./views/skills/SkillsView";
import RepoWikiView from "./views/wiki/RepoWikiView";
import WorkspaceView from "./views/workspace/WorkspaceView";
import { InboxDrawer } from "./shell/InboxDrawer";
import { StatusBar } from "./shell/StatusBar";
import { activateProcess, appStore, createConversation } from "./store/appStore";
import {
  chatStore,
  clearPendingDraftTimersForTest,
  draftKeyFor,
  flushPendingDraftSaves,
  loadOlderMessages,
  persistDraftNow,
  updateDraft,
} from "./store/chatStore";
import {
  drawerStore,
  openFileInDrawer,
  saveDrawerFile,
  setDrawerContext,
  setDrawerEditContent,
} from "./store/drawerStore";
import { loadProcessesForProject, sessionStore } from "./store/sessionStore";
import { WindowChrome } from "./shell/WindowChrome";
import type { ProcessInfo } from "./ipc/types";
import lifecycleActive from "./ipc/lifecycle-active.fixture.json";
import {
  openProjectModelsDialog,
  triggerContextInspector,
  uiStore,
} from "./store/uiStore";
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
          return {
            model: { ref: "primary", resolved: "openai:gpt-5", source: "global", error: null },
            defaultModel: { ref: "primary", resolved: "openai:gpt-5", source: "global", error: null },
            reasoning: { value: "xhigh", source: "global" },
            defaultReasoning: { value: "xhigh", source: "global" },
            codexFastMode: { enabled: false, applies: false, active: false, source: "builtin" },
          } as T;
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
  clearPendingDraftTimersForTest();
  cleanup();
  vi.unstubAllGlobals();
  delete window.__TAURI__;
  setIpcTransport(null);
  appStore.setState({
    bootstrapped: false,
    activeProjectDir: null,
    generalMode: false,
    projects: { projects: [], current: null, names: {} },
    modelsCatalog: [],
    modelsErrors: [],
    modelsLoading: false,
  });
  sessionStore.setState({
    processes: [],
    activeProcessId: null,
    projectDir: null,
    asks: [],
    effectiveModel: null,
    autoRun: { enabled: false, paused: false, goal: null },
  });
  chatStore.setState({
    sessionId: null,
    projectDir: null,
    processId: null,
    messages: [],
    total: 0,
    startIndex: 0,
    hasMore: false,
    streamingText: "",
    streamingReasoning: "",
    liveTools: [],
    liveSubagents: [],
    lastUsage: null,
    lastActionReport: null,
    autoAllow: false,
    drafts: {},
  });
  drawerStore.setState({
    open: false,
    projectDir: null,
    worktreePath: null,
    activeFile: null,
    preview: null,
    diff: null,
    editContent: "",
    editHash: null,
    editDirty: false,
    saving: false,
    conflictMessage: null,
    conflictHash: null,
    conflictExists: true,
    error: null,
  });
  uiStore.setState({
    view: "chat",
    confirmDialog: null,
    closingWindow: false,
    windowError: "",
    contextInspectorNonce: 0,
    globalAsks: [],
    globalAsksError: null,
    recentCompleted: [],
    projectModelsOpen: false,
    projectModelsTargetDir: null,
    rewindDialog: null,
    viewer: null,
    toasts: [],
  });
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

  it("Markdown 绝对路径链接向文件接口传递项目相对路径并定位行号", async () => {
    const { transport, calls } = createMockTransport();
    setIpcTransport(transport);
    drawerStore.setState({ projectDir: "C:\\Work\\kanzei code" });

    render(<MarkdownView text="[运行架构图](<c:/Work/kanzei code/docs/architecture/01_runtime_loop.md#L42>)" />);
    fireEvent.click(screen.getByRole("button", { name: "运行架构图" }));

    const path = "docs/architecture/01_runtime_loop.md";
    await waitFor(() => expect(drawerStore.getState().highlightLine).toBe(42));
    expect(drawerStore.getState().activeFile).toBe(path);
    for (const command of ["file_preview", "file_git_diff"]) {
      expect(calls.find((call) => call.command === command)?.args?.path).toBe(path);
    }
  });

  it("Markdown 百分号编码绝对路径、file URI 与 worktree 路径正确转为相对路径，外部路径仍交后端拒绝且不污染变更列表", async () => {
    const { transport, calls } = createMockTransport({
      file_preview: (args) => {
        const p = String(args?.path ?? "");
        if (/^[a-z]:\//i.test(p)) {
          throw new Error(`路径不合法(不允许盘符或冒号): ${p}`);
        }
        return {
          path: p,
          content: "line1\nline2\n",
          binary: false,
          truncated: false,
          size: 12,
          lines: 2,
          hash: "hash-ok",
        };
      },
    });
    setIpcTransport(transport);
    drawerStore.setState({
      projectDir: "C:\\Users\\kanzei\\Documents\\kanzei code",
      worktreePath: null,
      touchedFiles: [],
    });

    render(
      <MarkdownView
        text={
          "[会话库路径](C:/Users/kanzei/Documents/kanzei%20code/crates/kanzei-app/src/processes/naming.rs:18:4) " +
          "[文件URI](file:///C:/Users/kanzei/Documents/kanzei%20code/docs/%E4%BD%BF%E7%94%A8%E6%89%8B%E5%86%8C.md#L9-L12) " +
          "[越界文件](file:///C:/Windows/win.ini)"
        }
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "会话库路径" }));
    await waitFor(() =>
      expect(drawerStore.getState().activeFile).toBe("crates/kanzei-app/src/processes/naming.rs")
    );
    expect(drawerStore.getState().highlightLine).toBe(18);
    expect(drawerStore.getState().error).toBeNull();
    expect(
      [...calls].reverse().find((call) => call.command === "file_preview")?.args?.path
    ).toBe("crates/kanzei-app/src/processes/naming.rs");

    fireEvent.click(screen.getByRole("button", { name: "文件URI" }));
    await waitFor(() =>
      expect(drawerStore.getState().activeFile).toBe("docs/使用手册.md")
    );
    expect(drawerStore.getState().highlightLine).toBe(9);
    expect(drawerStore.getState().error).toBeNull();

    // 手动输入/直接传入带引号与行号的绝对路径也能正确剥离并定位行号
    await openFileInDrawer(
      '"C:\\Users\\kanzei\\Documents\\kanzei code\\crates\\kanzei-app\\src\\processes\\naming.rs:27"'
    );
    expect(drawerStore.getState().activeFile).toBe("crates/kanzei-app/src/processes/naming.rs");
    expect(drawerStore.getState().highlightLine).toBe(27);
    expect(drawerStore.getState().error).toBeNull();

    // 处于 worktree 内时优先匹配 worktree 根前缀
    drawerStore.setState({
      worktreePath: "C:\\Users\\kanzei\\Documents\\kanzei code\\.kanzei\\worktrees\\feat-a",
    });
    await openFileInDrawer(
      "C:/Users/kanzei/Documents/kanzei%20code/.kanzei/worktrees/feat-a/src/lib.rs"
    );
    expect(drawerStore.getState().activeFile).toBe("src/lib.rs");
    expect(drawerStore.getState().error).toBeNull();

    // 项目外路径不得静默转相对路径，应保留绝对路径交后端拒绝且不污染 touchedFiles
    fireEvent.click(screen.getByRole("button", { name: "越界文件" }));
    await waitFor(() => expect(drawerStore.getState().error).toContain("不允许盘符或冒号"));
    expect(drawerStore.getState().touchedFiles).not.toContain("C:/Windows/win.ini");
    expect(drawerStore.getState().touchedFiles).toContain("crates/kanzei-app/src/processes/naming.rs");
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

  it("生命周期使用 Rust 的 runtime 字段，旧快照和兼容通知不能覆盖运行状态", async () => {
    const { transport, emit } = createMockTransport();
    setIpcTransport(transport);
    const dispose = await installEventBridge();
    try {
      await loadProcessesForProject("C:/demo");
      const { sessionId, runtime } = lifecycleActive;
      emit("kz:lifecycle", lifecycleActive);
      expect(sessionStore.getState().processes[0]).toMatchObject({ running: true, runtime });

      emit("kz:lifecycle", { sessionId, runtime: { ...runtime, revision: 1, phase: "idle" } });
      emit("kz:lifecycle", { sessionId, runtime: { ...runtime, actor_epoch: "old", revision: 99, phase: "idle" } });
      emit("kz:idle", { sessionId });
      expect(sessionStore.getState().processes[0]?.running).toBe(true);

      emit("kz:lifecycle", { sessionId, runtime: { ...runtime, revision: 3, phase: "idle" } });
      emit("kz:turn", { sessionId });
      expect(sessionStore.getState().processes[0]?.running).toBe(false);
    } finally {
      dispose();
    }
  });

  it("关闭最后一个对话后，侧栏不呈现已关闭会话也不查询它们", async () => {
    let closed = false;
    const { transport: defaults } = createMockTransport();
    const { transport, calls } = createMockTransport({
      process_list: async () => closed ? [] : (await defaults.invoke<ProcessInfo[]>("process_list"))
        .map((p) => ({ ...p, id: "p42|C:/demo", ordinal: 42, label: "新对话" })),
      process_close: () => {
        closed = true;
        return "已关闭";
      },
    });
    setIpcTransport(transport);
    const { container } = render(<App />);
    const sidebar = within(container.querySelector(".sidebar") as HTMLElement);
    const row = await sidebar.findByRole("button", { name: "新对话" });
    expect(sidebar.queryByText(/已关闭对话/)).toBeNull();
    fireEvent.change(sidebar.getByRole("textbox", { name: "筛选项目或当前对话" }), { target: { value: "新对话" } });
    expect(sidebar.getByRole("button", { name: "新对话" })).toBe(row);
    fireEvent.contextMenu(row);
    fireEvent.click(sidebar.getByRole("button", { name: "关闭对话" }));
    await waitFor(() => expect(sidebar.queryByRole("button", { name: "新对话" })).toBeNull());
    expect(calls.some((c) => c.command === "process_closed_list")).toBe(false);
    expect(sidebar.queryByText(/已关闭对话/)).toBeNull();
  });

  it("导出读取完整对话段，不依赖滚动加载的消息", async () => {
    vi.stubGlobal("ResizeObserver", class { observe() {} unobserve() {} disconnect() {} });
    const { transport, calls } = createMockTransport({
      conversation_display_get: () => [
        { role: "user", parts: [{ type: "text", text: "未在界面加载的第一条" }] },
        { role: "assistant", parts: [{ type: "text", text: "末尾回复" }] },
      ],
      save_conversation_export: () => "C:/demo/export.md",
    });
    setIpcTransport(transport);
    render(<App />);
    await waitFor(() => expect(chatStore.getState().hasMore).toBe(true));
    expect(chatStore.getState().messages.some((m) => m.parts?.some((p) => p.text === "未在界面加载的第一条"))).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "更多对话操作" }));
    fireEvent.click(screen.getByRole("button", { name: "导出" }));
    fireEvent.click(await screen.findByRole("button", { name: "直接导出原文" }));
    await waitFor(() => {
      expect(calls.find((c) => c.command === "save_conversation_export")?.args?.content)
        .toMatch(/未在界面加载的第一条[\s\S]*末尾回复/);
    });
  });

  it("后台停机失败时保留窗口，并允许再次退出", async () => {
    const close = vi.fn().mockResolvedValue(undefined);
    window.__TAURI__ = { window: { getCurrentWindow: () => ({
      minimize: vi.fn(), toggleMaximize: vi.fn(),
      isMaximized: vi.fn().mockResolvedValue(false), startDragging: vi.fn(),
      close, onResized: vi.fn().mockResolvedValue(() => {}),
    }) } };
    let attempts = 0;
    const { transport } = createMockTransport({
      app_shutdown: () => {
        if (++attempts === 1) throw new Error("后台未静止");
        return { state: "quiescent", stopped: true };
      },
    });
    setIpcTransport(transport);
    render(<WindowChrome />);
    const button = screen.getByRole<HTMLButtonElement>("button", { name: "关闭" });
    fireEvent.click(button);
    await screen.findByText("后台未静止");
    expect(close).not.toHaveBeenCalled();
    expect(button.disabled).toBe(false);
    fireEvent.click(button);
    await waitFor(() => expect(close).toHaveBeenCalledTimes(1));
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

  it("全局设置模型选择框在已有选中值时仍展示全部服务商模型，并正确解析权限规则与连通测试", async () => {
    const { transport, calls } = createMockTransport({
      settings_get: () => ({
        path: "C:/Users/demo/.kanzei/kanzei.toml",
        primary: "codex:gpt-6-luna",
        fast: "codex:gpt-6-luna",
        compact: "",
        reasoning: "max",
        codexFastMode: true,
        proxy: "env",
        profileDefault: "readonly",
        providers: [
          {
            name: "codex",
            protocol: "openai-responses",
            baseUrl: "https://chatgpt.com/backend-api/codex",
            auth: "codex",
            contextLimit: 272000,
            builtin: true,
            source: "builtin",
          },
          {
            name: "deepseek",
            protocol: "deepseek-responses",
            baseUrl: "https://api.deepseek.com",
            apiKeyEnv: "DEEPSEEK_API_KEY",
            apiKey: null,
            keyPresent: true,
            contextLimit: 1000000,
            builtin: false,
            source: "global",
          },
        ],
        limits: {},
        limitDefaults: { maxTokens: 8192 },
        cadence: { full_test: "entry_close", targeted_test: "every_commit", commit: "per_batch", push: "per_entry" },
        projectModelOverrides: [],
      }),
      models_list: () => ({
        models: [
          { id: "primary", label: "primary → codex:gpt-6-luna" },
          { id: "fast", label: "fast → codex:gpt-6-luna" },
          { id: "codex:gpt-6-luna", label: "codex:gpt-6-luna" },
          { id: "codex:gpt-5.6-luna", label: "codex:gpt-5.6-luna" },
          { id: "deepseek:deepseek-chat", label: "deepseek:deepseek-chat" },
        ],
        errors: [],
      }),
      permission_rules_get: () => ({
        path: "C:/Users/demo/.kanzei/project-trust.json",
        rules: [{ index: 0, action: "bash", resource: "cargo test", effect: "allow" }],
      }),
      provider_test: () => "✗ key 无效(HTTP 401)",
    });
    setIpcTransport(transport);
    appStore.setState({ activeProjectDir: "C:/demo", generalMode: false });

    render(<SettingsView />);

    await waitFor(() => {
      expect((screen.getByLabelText("主模型（primary）") as HTMLInputElement).value).toBe("codex:gpt-6-luna");
    });

    // 展开主模型下拉：即使已有值 codex:gpt-6-luna，仍应显示全部服务商模型，且过滤掉 primary/fast 角色项
    fireEvent.click(screen.getByLabelText("主模型（primary）"));
    await waitFor(() => {
      expect(screen.getByRole("option", { name: /codex:gpt-5\.6-luna/ })).toBeTruthy();
      expect(screen.getByRole("option", { name: /deepseek:deepseek-chat/ })).toBeTruthy();
    });
    expect(screen.queryByRole("option", { name: /^primary$/ })).toBeNull();

    // 选择另一个服务商模型
    fireEvent.click(screen.getByRole("option", { name: /deepseek:deepseek-chat/ }));
    expect((screen.getByLabelText("主模型（primary）") as HTMLInputElement).value).toBe("deepseek:deepseek-chat");

    // 校验思考强度、默认档位、节奏均完整保留后端枚举值
    expect((screen.getByLabelText("默认思考强度（reasoning）") as HTMLSelectElement).value).toBe("max");
    expect((screen.getByLabelText("默认执行档位") as HTMLSelectElement).value).toBe("readonly");
    expect((screen.getByLabelText("推送节奏（push）") as HTMLSelectElement).value).toBe("per_entry");

    // 校验已记住权限规则正确渲染，并可携带精确 expectedRule 删除
    expect(screen.getByText("[bash] cargo test (allow)")).toBeTruthy();

    // 校验 provider_test 纯字符串错误不被误报为“连通正常”
    const testButtons = screen.getAllByRole("button", { name: "连通性测试" });
    fireEvent.click(testButtons[1]);
    await waitFor(() => {
      expect(screen.getByText("✗ key 无效(HTTP 401)")).toBeTruthy();
    });
    expect(calls.some((c) => c.command === "provider_test")).toBe(true);
  });

  it("本项目模型覆盖弹窗正确读取 fields 并仅差分提交变更键", async () => {
    const { transport, calls } = createMockTransport({
      project_models_get: () => ({
        projectDir: "C:/demo",
        configPath: "C:/demo/.kanzei/kanzei.toml",
        exists: false,
        fields: {
          primary: {
            project: "deepseek:deepseek-chat",
            global: "codex:gpt-6-luna",
            inherited: "codex:gpt-6-luna",
            inheritedSource: "global",
            inheritedFollowsPrimary: false,
            effective: "deepseek:deepseek-chat",
            source: "project",
            followsPrimary: false,
          },
          fast: {
            project: null,
            global: null,
            inherited: "deepseek:deepseek-chat",
            inheritedSource: "project",
            inheritedFollowsPrimary: true,
            effective: "deepseek:deepseek-chat",
            source: "project",
            followsPrimary: true,
          },
          compact: {
            project: null,
            global: null,
            inherited: "deepseek:deepseek-chat",
            inheritedSource: "project",
            inheritedFollowsPrimary: true,
            effective: "deepseek:deepseek-chat",
            source: "project",
            followsPrimary: true,
          },
          reasoning: {
            project: "high",
            global: "off",
            inherited: "off",
            inheritedSource: "global",
            inheritedFollowsPrimary: false,
            effective: "high",
            source: "project",
            followsPrimary: false,
          },
          codexFastMode: {
            project: null,
            global: true,
            inherited: true,
            inheritedSource: "global",
            inheritedFollowsPrimary: false,
            effective: true,
            source: "global",
            followsPrimary: false,
          },
        },
      }),
      project_models_save: () => ({}),
    });
    setIpcTransport(transport);
    appStore.setState({
      activeProjectDir: "C:/demo",
      generalMode: false,
      modelsCatalog: [
        { id: "codex:gpt-6-luna", label: "codex:gpt-6-luna" },
        { id: "deepseek:deepseek-chat", label: "deepseek:deepseek-chat" },
      ],
    });
    openProjectModelsDialog("C:/demo");

    render(<ProjectModelsDialogHost />);

    await waitFor(() => {
      expect((screen.getByLabelText("项目主模型（primary）") as HTMLInputElement).value).toBe(
        "deepseek:deepseek-chat"
      );
    });
    expect((screen.getByLabelText("思考强度（reasoning）") as HTMLSelectElement).value).toBe("high");
    expect((screen.getByRole("button", { name: "打开配置文件" }) as HTMLButtonElement).disabled).toBe(true);

    // 仅把 codexFastMode 改为关、把 reasoning 恢复继承；primary 未改动不应重复发送
    fireEvent.change(screen.getByLabelText("思考强度（reasoning）"), { target: { value: "" } });
    fireEvent.change(screen.getByLabelText("Codex Fast Mode"), { target: { value: "off" } });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => {
      const saveCall = calls.find((c) => c.command === "project_models_save");
      expect(saveCall).toBeTruthy();
      expect(saveCall?.args).toEqual({
        projectDir: "C:/demo",
        set: { codexFastMode: false },
        unset: ["reasoning"],
      });
    });
  });

  it("对话控制栏切回默认模型/思考强度发送空串清除覆盖，权限审批独立于自动推进直接位于底栏", async () => {
    const { transport, calls } = createMockTransport({
      process_update: (args) => ({
        id: "d|C:/demo",
        origin_project: "C:/demo",
        project_dir: "C:/demo",
        worktree_path: null,
        branch: "main",
        session_id: "ses-demo",
        model: args?.model === "" ? null : (args?.model as string | null) ?? "codex:gpt-6-luna",
        profile: (args?.profile as string | null) ?? "dev-auto",
        research_topic: null,
        reasoning: args?.reasoning === "" ? null : (args?.reasoning as string | null) ?? "high",
        manual_models: [],
        subagent_mode: (args?.subagentMode as string | null) ?? "auto",
        tracker_writes: false,
        stage: "空闲",
        running: false,
        label: "主对话",
        title: null,
        title_custom: false,
        kind: "main",
        ordinal: null,
        updated_at: 1000,
      }),
      run_prompt: () => "started",
    });
    setIpcTransport(transport);
    appStore.setState({
      activeProjectDir: "C:/demo",
      generalMode: false,
      modelsCatalog: [{ id: "codex:gpt-6-luna", label: "codex:gpt-6-luna" }],
    });
    sessionStore.setState({
      projectDir: "C:/demo",
      activeProcessId: "d|C:/demo",
      processes: [
        {
          id: "d|C:/demo",
          origin_project: "C:/demo",
          project_dir: "C:/demo",
          worktree_path: null,
          branch: "main",
          session_id: "ses-demo",
          model: "codex:gpt-6-luna",
          profile: "dev-auto",
          research_topic: null,
          reasoning: "high",
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
      ],
      effectiveModel: {
        model: { ref: "codex:gpt-6-luna", resolved: "codex:gpt-6-luna", source: "line", error: null },
        defaultModel: { ref: "primary", resolved: "openai:gpt-5", source: "global", error: null },
        reasoning: { value: "high", source: "line" },
        defaultReasoning: { value: "xhigh", source: "global" },
      },
      autoRun: { enabled: false, paused: false, goal: null },
    });

    render(<Composer />);

    vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
    const model = screen.getByRole("button", { name: "模型选择" });
    expect(model.title).toContain("本线临时");
    fireEvent.click(model);
    fireEvent.click(screen.getByRole("option", {name: "默认 (openai:gpt-5)"}));
    await waitFor(() => {
      const updateCall = calls.find((c) => c.command === "process_update");
      expect(updateCall?.args).toEqual({ processId: "d|C:/demo", model: "" });
    });

    fireEvent.click(screen.getByRole("button", {name: "协作参数"}));
    fireEvent.click(screen.getByRole("radio", {name:/默认 \(/}));
    await waitFor(() => {
      const reasoningCall = calls.find(
        (c) => c.command === "process_update" && "reasoning" in (c.args ?? {})
      );
      expect(reasoningCall?.args).toEqual({ processId: "d|C:/demo", reasoning: "" });
    });

    // 项目模式同时包含只读讨论 (readonly) 与隔离沙箱 (isolated)
    expect(screen.getByRole("radio", {name:"只读讨论"})).toBeTruthy();
    expect(screen.getByRole("radio", {name:"隔离沙箱"})).toBeTruthy();

    // 权限审批按钮直接暴露在输入框底栏，未开启自动推进即可一键切换并透传 autoAllow
    const permBtn = screen.getByRole("button", { name: "逐项确认" }) as HTMLButtonElement;
    expect(permBtn.disabled).toBe(false);
    expect(permBtn.getAttribute("aria-pressed")).toBe("false");
    fireEvent.click(permBtn);
    const activePermBtn = screen.getByRole("button", { name: "自动放行" }) as HTMLButtonElement;
    expect(activePermBtn.getAttribute("aria-pressed")).toBe("true");
    expect(chatStore.getState().autoAllow).toBe(true);

    fireEvent.change(screen.getByRole("textbox"), { target: { value: "帮我修改测试" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => {
      const runCall = calls.find((c) => c.command === "run_prompt");
      expect(runCall?.args?.autoAllow).toBe(true);
      expect(runCall?.args?.autonomous).toBe(false);
    });
  });

  it("AskOverlay 严格按后端契约发送 once / always / cancel，不再发送无效的 allow", async () => {
    const { transport, calls } = createMockTransport({
      answer_ask: () => null,
    });
    setIpcTransport(transport);
    sessionStore.setState({
      projectDir: "C:/demo",
      activeProcessId: "d|C:/demo",
      processes: [
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
      asks: [
        {
          id: 11,
          sessionId: "ses-demo",
          kind: "permission",
          action: "bash",
          resource: "cargo test",
        },
        {
          id: 12,
          sessionId: "ses-demo",
          kind: "permission",
          action: "write",
          resource: "src/lib.rs",
        },
        {
          id: 13,
          sessionId: "ses-demo",
          kind: "question",
          action: "question",
          resource: "",
          question: "需要继续吗？",
        },
      ],
    });

    render(<AskOverlay />);
    fireEvent.click(screen.getByRole("button", { name: "允许一次" }));
    await waitFor(() => {
      expect(calls.some((c) => c.command === "answer_ask" && c.args?.id === 11 && c.args?.reply === "once")).toBe(true);
    });

    fireEvent.click(screen.getByRole("button", { name: "允许并记住" }));
    await waitFor(() => {
      expect(calls.some((c) => c.command === "answer_ask" && c.args?.id === 12 && c.args?.reply === "always")).toBe(true);
    });

    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    await waitFor(() => {
      expect(calls.some((c) => c.command === "answer_ask" && c.args?.id === 13 && c.args?.reply === "cancel")).toBe(true);
    });
  });

  it("Checkpoint 回退走两阶段 preview + expectedHash 协议，并支持对话与代码联合回滚", async () => {
    const { transport, calls } = createMockTransport({
      conversation_action: (args) => {
        if (args?.action === "preview") {
          return {
            sourceHash: "hash-abc",
            files: [{ path: "src/main.rs", action: "modified" }],
            unhandled: [],
            prompt: "修复登录校验",
            worktree: false,
            keptMessages: 2,
          };
        }
        return { ok: true };
      },
    });
    setIpcTransport(transport);
    sessionStore.setState({
      projectDir: "C:/demo",
      activeProcessId: "d|C:/demo",
      processes: [
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
      ],
    });

    render(
      <>
        <MessageItem
          message={{ role: "user", parts: [{ type: "text", text: "修复登录校验" }] }}
          globalIndex={0}
          toolResultMap={new Map()}
        />
        <RewindDialogHost />
      </>
    );

    fireEvent.click(screen.getByRole("button", { name: "检查点回退" }));
    await waitFor(() => {
      expect(screen.getByText("检查点回退（Checkpoint）")).toBeTruthy();
      expect(screen.getByText("src/main.rs")).toBeTruthy();
    });

    fireEvent.click(screen.getByRole("button", { name: "对话与代码一起回退" }));
    await waitFor(() => {
      const applyCall = calls.find(
        (c) => c.command === "conversation_action" && c.args?.action === "both"
      );
      expect(applyCall?.args?.expectedHash).toBe("hash-abc");
    });
  });

  it("Composer 支持 @文件引用补全、/斜杠命令与 Shift+Tab 循环切换模式，StatusBar 展示上下文占比与压缩建议", async () => {
    const { transport, calls } = createMockTransport({
      project_files: () => ["src/main.rs", "src/app.rs"],
      process_update: (args) => ({
        id: "d|C:/demo",
        origin_project: "C:/demo",
        project_dir: "C:/demo",
        worktree_path: null,
        branch: "main",
        session_id: "ses-demo",
        model: null,
        profile: (args?.profile as string) ?? "dev-pair",
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
      }),
    });
    setIpcTransport(transport);
    appStore.setState({ activeProjectDir: "C:/demo", generalMode: false });
    sessionStore.setState({
      projectDir: "C:/demo",
      activeProcessId: "d|C:/demo",
      processes: [
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
      ],
      effectiveModel: {
        model: { ref: "primary", resolved: "openai:gpt-5", source: "global", error: null },
        contextLimit: 100000,
      },
    });
    chatStore.setState({ lastUsage: { input: 82000, output: 3000 } });

    render(
      <>
        <Composer />
        <StatusBar />
      </>
    );

    // StatusBar 显示上下文占用 85% 与“建议压缩”按钮
    expect(screen.getByText("85%")).toBeTruthy();
    expect(screen.getByRole("button", { name: "建议压缩" })).toBeTruthy();

    // 输入 @main 弹出文件补全并插入反引号路径
    const input = screen.getByPlaceholderText(/输入需求或指令/) as HTMLTextAreaElement;
    fireEvent.change(input, { target: { value: "检查 @main", selectionStart: 8 } });
    await waitFor(() => {
      expect(screen.getByRole("option", { name: "src/main.rs" })).toBeTruthy();
    });
    fireEvent.click(screen.getByRole("option", { name: "src/main.rs" }));
    expect(input.value).toContain("`src/main.rs`");

    // Shift+Tab 循环切换执行模式
    fireEvent.keyDown(input, { key: "Tab", shiftKey: true });
    await waitFor(() => {
      expect(
        calls.some((c) => c.command === "process_update" && c.args?.profile === "dev-pair")
      ).toBe(true);
    });
  });

  it("连续只读探索工具自动折叠分组，edit 工具显示增删行统计与内联 Diff", () => {
    const resultMap = new Map([
      ["t1", { type: "tool_result", tool_use_id: "t1", content: "ok" }],
      ["t2", { type: "tool_result", tool_use_id: "t2", content: "ok" }],
      ["t3", { type: "tool_result", tool_use_id: "t3", content: "edited" }],
    ]);
    render(
      <MessageItem
        message={{
          role: "assistant",
          parts: [
            { type: "tool_use", id: "t1", name: "read", input: { path: "src/a.ts" } },
            { type: "tool_use", id: "t2", name: "grep", input: { pattern: "foo" } },
            {
              type: "tool_use",
              id: "t3",
              name: "edit",
              input: { path: "src/a.ts", old_string: "const a = 1;", new_string: "const a = 2;\nconst b = 3;" },
            },
            { type: "text", text: "已更新完成。" },
          ],
        }}
        globalIndex={1}
        toolResultMap={resultMap}
      />
    );

    expect(screen.getByText("探索 2 步")).toBeTruthy();
    expect(screen.getByText("+2")).toBeTruthy();
    expect(screen.getByText("-1")).toBeTruthy();
    fireEvent.click(screen.getByText("edit"));
    expect(screen.getByText("内联变更预览")).toBeTruthy();
  });

  it("全局收件箱 Inbox 支持跨项目直接放行权限审批", async () => {
    const { transport, calls } = createMockTransport({
      pending_asks_all: () => [
        {
          id: 501,
          sessionId: "ses-bg",
          projectDir: "C:/other",
          processId: "p-bg",
          kind: "permission",
          action: "bash",
          resource: "cargo test",
        },
      ],
      answer_ask: () => null,
    });
    setIpcTransport(transport);
    uiStore.setState({ inboxOpen: true, globalAsks: [], recentCompleted: [] });

    render(<InboxDrawer />);

    await waitFor(() => {
      expect(screen.getByText(/cargo test/)).toBeTruthy();
    });
    fireEvent.click(screen.getByRole("button", { name: "本次允许" }));
    await waitFor(() => {
      expect(calls.some((c) => c.command === "answer_ask" && c.args?.id === 501 && c.args?.reply === "once")).toBe(true);
    });
  });

  it("Composer 支持 !命令 本地 Shell 直通执行并一键附带到输入框", async () => {
    const { transport, calls } = createMockTransport({
      agent_directory_get: () => ({ agents: [] }),
      shell_exec_quick: () => ({
        command: "git status -s",
        cwd: "C:/demo",
        exitCode: 0,
        stdout: "M src/main.rs\n",
        stderr: "",
        durationMs: 42,
        truncated: false,
        timedOut: false,
      }),
    });
    setIpcTransport(transport);
    appStore.setState({ activeProjectDir: "C:/demo", generalMode: false });
    sessionStore.setState({
      projectDir: "C:/demo",
      activeProcessId: "d|C:/demo",
      processes: [
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
      ],
    });

    render(<Composer />);
    const input = screen.getByPlaceholderText(/输入需求或指令/) as HTMLTextAreaElement;
    fireEvent.change(input, { target: { value: "!git status -s", selectionStart: 14 } });
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() => {
      expect(calls.some((c) => c.command === "shell_exec_quick" && c.args?.command === "git status -s")).toBe(true);
      expect(screen.getByText("exit 0")).toBeTruthy();
      expect(screen.getByText(/M src\/main\.rs/)).toBeTruthy();
    });

    fireEvent.click(screen.getByRole("button", { name: "附带到输入框" }));
    expect(input.value).toContain("git status -s");
    expect(input.value).toContain("M src/main.rs");
  });

  it("/btw 旁路只读提问独立应答且支持引用到主对话", async () => {
    const { transport, calls } = createMockTransport({
      side_question: () => ({
        answer: "可以直接使用 Vec::retain 过滤。",
        model: "openai:gpt-5",
        inputTokens: 1200,
        outputTokens: 60,
      }),
    });
    setIpcTransport(transport);
    appStore.setState({ activeProjectDir: "C:/demo" });
    sessionStore.setState({ activeProcessId: "d|C:/demo" });
    uiStore.setState({ btwOpen: true, btwInitialQuestion: "Rust 怎么原地过滤 Vec？" });

    render(<BtwOverlay />);

    await waitFor(() => {
      expect(calls.some((c) => c.command === "side_question")).toBe(true);
      expect(screen.getByText(/Vec::retain/)).toBeTruthy();
    });
  });

  it("StatusBar 上下文构成弹窗兼容真实后端对象 source 且不崩溃", async () => {
    const { transport, calls } = createMockTransport({
      context_snapshot: () => ({
        sessionId: "ses-demo",
        requests: [{ sequence: 12, step: 1, model: "openai:gpt-5" }],
        selected: {
          sequence: 12,
          manifest: {
            model: "openai:gpt-5",
            estimated_tokens: 18000,
            blocks: [
              {
                position: 0,
                kind: "system",
                role: "system",
                label: "agent/system",
                chars: 4000,
                bytes: 4000,
                sha256: "aaa",
                inspectable: true,
                source: { system_index: 0, offset_bytes: 0, length_bytes: 4000 },
              },
              {
                position: 1,
                kind: "runtime_context",
                role: "user",
                label: "memory/recall",
                chars: 2000,
                bytes: 2000,
                sha256: "bbb",
                inspectable: true,
                source: { origin: "engine", scope: "current_request" },
              },
              {
                position: 2,
                kind: "tool_result",
                role: "user",
                label: "read_file",
                chars: 1500,
                bytes: 1500,
                sha256: "ddd",
                inspectable: true,
                source: { call_id: "c1", is_error: false, tool: "read_file", input: { path: "src/main.rs" } },
              },
              {
                position: 3,
                kind: "message",
                role: "user",
                label: "messages/1/1",
                chars: 6000,
                bytes: 6000,
                sha256: "ccc",
                inspectable: true,
                source: { origin: "human" },
              },
              {
                position: 4,
                kind: "attachment",
                role: "user",
                label: "image/png",
                chars: 0,
                bytes: 2048,
                sha256: "eee",
                inspectable: false,
                source: null,
              },
            ],
          },
        },
        web: [],
      }),
      context_content: () => "完整系统提示词原文",
    });
    setIpcTransport(transport);
    appStore.setState({ activeProjectDir: "C:/demo", generalMode: false });
    sessionStore.setState({
      projectDir: "C:/demo",
      activeProcessId: "d|C:/demo",
      processes: [
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
      ],
      effectiveModel: { contextLimit: 100000 },
    });

    chatStore.setState({ lastUsage: { input: 1931, output: 913 } });
    render(<StatusBar />);
    fireEvent.click(screen.getByRole("button", { name: /In 1,931 \/ Out 913/ }));
    await waitFor(() => {
      expect(screen.getByRole("img", { name: "上下文构成占比条" })).toBeTruthy();
      expect(screen.getByText("系统提示与规则")).toBeTruthy();
      expect(screen.getByText("长期记忆与偏好")).toBeTruthy();
      expect(screen.getByText("工具与结果")).toBeTruthy();
      expect(screen.getByText("对话历史与消息")).toBeTruthy();
      expect(screen.getByText("agent/system")).toBeTruthy();
      expect(screen.getByText("read_file")).toBeTruthy();
      expect(screen.getByText("image/png")).toBeTruthy();
    });

    // inspectable=false 的附件块不应渲染“查看原文”按钮（共 4 个可查看块）
    const group = screen.getByText("系统提示与规则").closest("summary")!;
    fireEvent.click(group);
    expect(document.querySelectorAll(".ctx-block-right button")).toHaveLength(4);
    fireEvent.click(group.parentElement!.querySelector("button")!);
    await waitFor(() => {
      expect(calls.some((c) => c.command === "context_content" && c.args?.sha256 === "aaa")).toBe(true);
      expect(uiStore.getState().viewer?.title).toBe("agent/system");
    });
  });

  it("/context 打开并关闭上下文弹窗后，切换会话不会自动重开", async () => {
    const { transport, calls } = createMockTransport({
      context_snapshot: (args) => ({
        sessionId: String(args?.processId ?? ""),
        requests: [],
        selected: null,
        web: [],
      }),
    });
    setIpcTransport(transport);
    appStore.setState({ activeProjectDir: "C:/demo", generalMode: false });
    sessionStore.setState({
      projectDir: "C:/demo",
      activeProcessId: "p-a",
      processes: [
        {
          id: "p-a",
          origin_project: "C:/demo",
          project_dir: "C:/demo",
          worktree_path: null,
          branch: "main",
          session_id: "ses-a",
          model: null,
          profile: "dev-auto",
          research_topic: null,
          reasoning: null,
          manual_models: [],
          subagent_mode: "auto",
          tracker_writes: false,
          stage: "空闲",
          running: false,
          label: "对话 A",
          title: null,
          title_custom: false,
          kind: "main",
          ordinal: 1,
          updated_at: 1000,
        },
        {
          id: "p-b",
          origin_project: "C:/demo",
          project_dir: "C:/demo",
          worktree_path: null,
          branch: "main",
          session_id: "ses-b",
          model: null,
          profile: "dev-auto",
          research_topic: null,
          reasoning: null,
          manual_models: [],
          subagent_mode: "auto",
          tracker_writes: false,
          stage: "空闲",
          running: false,
          label: "对话 B",
          title: null,
          title_custom: false,
          kind: "task",
          ordinal: 2,
          updated_at: 2000,
        },
      ],
    });

    render(<StatusBar />);
    triggerContextInspector();
    await waitFor(() => {
      expect(screen.getByRole("dialog", { name: "本轮请求上下文构成" })).toBeTruthy();
    });
    const beforeSwitchCalls = calls.filter((c) => c.command === "context_snapshot").length;
    expect(beforeSwitchCalls).toBe(1);

    fireEvent.click(screen.getByRole("button", { name: "关闭" }));
    expect(screen.queryByRole("dialog", { name: "本轮请求上下文构成" })).toBeNull();

    await activateProcess("p-b");
    await waitFor(() => {
      expect(sessionStore.getState().activeProcessId).toBe("p-b");
    });
    expect(screen.queryByRole("dialog", { name: "本轮请求上下文构成" })).toBeNull();
    expect(calls.filter((c) => c.command === "context_snapshot").length).toBe(1);

    // 再次显式触发 /context 仍可正常打开会话 B 的快照
    triggerContextInspector();
    await waitFor(() => {
      expect(screen.getByRole("dialog", { name: "本轮请求上下文构成" })).toBeTruthy();
    });
    expect(
      calls.some((c) => c.command === "context_snapshot" && c.args?.processId === "p-b")
    ).toBe(true);
  });

  it("ErrorBoundary 隔离局部渲染异常并在切换上下文后自动恢复", () => {
    const errSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      function MaybeCrash({ crash }: { crash: boolean }): React.ReactElement {
        if (crash) throw new Error("模拟子视图崩溃");
        return <div>视图正常运行</div>;
      }
      const { rerender } = render(
        <div>
          <nav>主导航保持可用</nav>
          <ErrorBoundary scope="测试局部视图" resetKeys={["broken"]}>
            <MaybeCrash crash={true} />
          </ErrorBoundary>
        </div>
      );
      expect(screen.getByText("主导航保持可用")).toBeTruthy();
      expect(screen.getByRole("alert")).toBeTruthy();
      expect(screen.getByText(/测试局部视图暂时不可用/)).toBeTruthy();

      rerender(
        <div>
          <nav>主导航保持可用</nav>
          <ErrorBoundary scope="测试局部视图" resetKeys={["recovered"]}>
            <MaybeCrash crash={false} />
          </ErrorBoundary>
        </div>
      );
      expect(screen.getByText("视图正常运行")).toBeTruthy();
    } finally {
      errSpy.mockRestore();
    }
  });

  it("工作树会话下预览、Diff、保存与回退此块始终指向同一工作树，且冲突绝不误报成功", async () => {
    let writeStep = 0;
    const { transport, calls } = createMockTransport({
      file_preview: (args) => ({
        content: args?.worktreePath ? "line1\nbranch-edited\n" : "line1\nmain-only\n",
        hash: args?.worktreePath ? "hash-wt-1" : "hash-main-1",
        bom: false,
        readonly: null,
        binary: false,
      }),
      file_git_diff: () => ({
        path: "src/app.rs",
        status: "modified",
        diff: "@@ -1,2 +1,2 @@\n line1\n-old\n+branch-edited\n",
        additions: 1,
        deletions: 1,
      }),
      file_write: () => {
        writeStep += 1;
        if (writeStep === 1) {
          return {
            status: "conflict",
            hash: "hash-wt-disk-v2",
            size: 20,
            mtimeMs: 2000,
            exists: true,
            evidence: null,
          };
        }
        return {
          status: "saved",
          hash: `hash-wt-saved-${writeStep}`,
          size: 22,
          mtimeMs: 3000,
          exists: true,
          evidence: writeStep === 2 ? ".kanzei/quarantine/files-overwrite-1/src/app.rs" : null,
        };
      },
    });
    setIpcTransport(transport);
    appStore.setState({ activeProjectDir: "C:/demo", generalMode: false });
    setDrawerContext("C:/demo", "C:/demo-wt/feat-a");

    await openFileInDrawer("src/app.rs", { tab: "edit" });
    expect(drawerStore.getState().preview?.content).toBe("line1\nbranch-edited\n");
    expect(
      calls.some(
        (c) =>
          c.command === "file_preview" &&
          c.args?.projectDir === "C:/demo" &&
          c.args?.worktreePath === "C:/demo-wt/feat-a"
      )
    ).toBe(true);

    // 1. 磁盘冲突时必须返回 false，保持 editDirty=true，记录最新磁盘指纹，不提示成功
    setDrawerEditContent("line1\nuser-new\n");
    const firstSave = await saveDrawerFile(false);
    expect(firstSave).toBe(false);
    expect(drawerStore.getState().editDirty).toBe(true);
    expect(drawerStore.getState().conflictHash).toBe("hash-wt-disk-v2");
    expect(drawerStore.getState().conflictMessage).toContain("尚未写入");
    expect(uiStore.getState().toasts.some((t) => t.text.includes("已保存"))).toBe(false);

    // 2. 点击强制覆盖必须携带冲突返回的最新磁盘指纹与 worktreePath，并开启 evidence=true
    const forcedSave = await saveDrawerFile(true);
    expect(forcedSave).toBe(true);
    expect(drawerStore.getState().editDirty).toBe(false);
    expect(drawerStore.getState().conflictMessage).toBeNull();
    expect(
      calls.some(
        (c) =>
          c.command === "file_write" &&
          c.args?.projectDir === "C:/demo" &&
          c.args?.worktreePath === "C:/demo-wt/feat-a" &&
          c.args?.expectedHash === "hash-wt-disk-v2" &&
          c.args?.evidence === true
      )
    ).toBe(true);

    // 3. Diff「回退此块」在工作树与普通文件上都能通过参数契约并携带同一 worktreePath
    drawerStore.setState({ fileTab: "diff" });
    render(<ContextDrawer />);
    fireEvent.click(screen.getByRole("button", { name: "回退此块" }));
    await waitFor(() => {
      expect(
        calls.some(
          (c) =>
            c.command === "file_write" &&
            c.args?.projectDir === "C:/demo" &&
            c.args?.worktreePath === "C:/demo-wt/feat-a" &&
            c.args?.content === "line1\nold\n" &&
            c.args?.expectedHash === "hash-wt-1"
        )
      ).toBe(true);
    });
  });

  it("新建对话自动加载新会话的 0 条消息并清空上一条会话历史", async () => {
    const processes = [
      {
        id: "p-1",
        origin_project: "C:/demo",
        project_dir: "C:/demo",
        worktree_path: null,
        branch: "main",
        session_id: "ses-1",
        model: null,
        profile: "dev-auto",
        research_topic: null,
        reasoning: null,
        manual_models: [],
        subagent_mode: "auto",
        tracker_writes: false,
        stage: "空闲",
        running: false,
        label: "对话 1",
        title: null,
        title_custom: false,
        kind: "main",
        ordinal: 1,
        updated_at: 1000,
      },
    ];
    const { transport } = createMockTransport({
      process_list: () => [...processes],
      process_create: () => {
        const created = {
          ...processes[0],
          id: "p-2",
          session_id: "ses-2",
          label: "对话 2",
          ordinal: 2,
          updated_at: 2000,
        };
        processes.push(created);
        return created;
      },
      conversation_page_messages: (args) => {
        if (args?.processId === "p-1") {
          return {
            items: [
              { role: "user", parts: [{ type: "text", text: "第一条对话提问" }] },
              { role: "assistant", parts: [{ type: "text", text: "第一条对话回复" }] },
            ],
            total: 2,
            startIndex: 0,
            endIndex: 2,
            hasMore: false,
          };
        }
        return { items: [], total: 0, startIndex: 0, endIndex: 0, hasMore: false };
      },
    });
    setIpcTransport(transport);
    appStore.setState({ activeProjectDir: "C:/demo", generalMode: false });
    sessionStore.setState({ projectDir: "C:/demo", processes: [...processes], activeProcessId: "p-1" });

    await activateProcess("p-1");
    expect(chatStore.getState().messages.length).toBe(2);

    const created = await createConversation("C:/demo");
    expect(created?.id).toBe("p-2");
    expect(sessionStore.getState().activeProcessId).toBe("p-2");
    expect(chatStore.getState().sessionId).toBe("ses-2");
    expect(chatStore.getState().messages).toEqual([]);
  });

  it("快速切换并编辑两个会话的草稿时互不取消落盘保存", async () => {
    vi.useFakeTimers();
    try {
      const savedDrafts = new Map<string, unknown>();
      const { transport } = createMockTransport({
        composer_draft_save: (args) => {
          savedDrafts.set(String(args?.key), args?.draft);
          return null;
        },
      });
      setIpcTransport(transport);

      updateDraft("C:/demo", "p-a", "会话 A 的草稿", []);
      vi.advanceTimersByTime(150);
      updateDraft("C:/demo", "p-b", "会话 B 的草稿", []);
      vi.advanceTimersByTime(250);
      await Promise.resolve();

      // 此时已过 400ms：A 的定时器必须独立触发，不会被 B 取消
      expect(savedDrafts.get(draftKeyFor("C:/demo", "p-a"))).toEqual({
        text: "会话 A 的草稿",
        attachments: [],
      });

      vi.advanceTimersByTime(200);
      await Promise.resolve();
      expect(savedDrafts.get(draftKeyFor("C:/demo", "p-b"))).toEqual({
        text: "会话 B 的草稿",
        attachments: [],
      });

      // 未到期时调用 flushPendingDraftSaves 也能立刻落盘
      updateDraft("C:/demo", "p-a", "退出前刚输入的内容", []);
      await flushPendingDraftSaves();
      expect(savedDrafts.get(draftKeyFor("C:/demo", "p-a"))).toEqual({
        text: "退出前刚输入的内容",
        attachments: [],
      });
    } finally {
      vi.useRealTimers();
    }
  });

  it("工作台 My Quests 看板按四态分组展示跨项目会话并支持就地放行", async () => {
    const { transport, calls } = createMockTransport({
      pending_asks_all: () => [
        {
          id: 701,
          sessionId: "ses-ask",
          projectDir: "C:/demo",
          processId: "p2",
          kind: "permission",
          action: "bash",
          resource: "npm test",
        },
      ],
      workspace_overview: () => ({
        current: "C:/demo",
        observed_at: 1000,
        projects: [
          {
            path: "C:/demo",
            project_id: "C:/demo",
            name: "Demo",
            current: true,
            running_lines: 1,
            lines_total: 2,
            lines: [
              {
                id: "p1",
                label: "后台重构",
                title: "后台重构",
                title_custom: true,
                kind: "task",
                ordinal: 1,
                updated_at: 2000,
                session_id: "ses-run",
                running: true,
                stage: "执行工具",
                branch: "feat-x",
                worktree_path: "C:/demo/.wt/feat-x",
                profile: "dev-auto",
                available: true,
              },
              {
                id: "p2",
                label: "待审批任务",
                title: "待审批任务",
                title_custom: true,
                kind: "task",
                ordinal: 2,
                updated_at: 1900,
                session_id: "ses-ask",
                running: true,
                stage: "等待审批",
                branch: "main",
                worktree_path: null,
                profile: "dev-pair",
                available: true,
              },
            ],
          },
        ],
      }),
      answer_ask: () => null,
    });
    setIpcTransport(transport);
    appStore.setState({
      activeProjectDir: "C:/demo",
      projects: { projects: ["C:/demo"], current: "C:/demo", names: { "C:/demo": "Demo" } },
    });

    render(<WorkspaceView />);
    fireEvent.click(screen.getByRole("tab", { name: /My Quests/ }));

    await waitFor(() => {
      expect(screen.getByText("后台重构")).toBeTruthy();
      expect(screen.getByText("待审批任务")).toBeTruthy();
      expect(screen.getByText(/npm test/)).toBeTruthy();
    });

    fireEvent.click(screen.getByRole("button", { name: "本次允许" }));
    await waitFor(() => {
      expect(calls.some((c) => c.command === "answer_ask" && c.args?.id === 701 && c.args?.reply === "once")).toBe(true);
    });
  });

  it("Repo Wiki 视图展示漂移状态并支持一键 AI 增量同步", async () => {
    const { transport, calls } = createMockTransport({
      repo_wiki_overview: () => ({
        projectDir: "C:/demo",
        wikiDir: "C:/demo/.kanzei/wiki",
        totalPages: 2,
        freshCount: 0,
        staleCount: 1,
        missingCount: 1,
        deps: [{ from: "kanzei-app", to: "kanzei-core" }],
        pages: [
          {
            areaId: "overview",
            title: "仓库全局架构总览",
            kind: "overview",
            parent: null,
            band: 4,
            bandLabel: "全局总览",
            depth: 0,
            status: "stale",
            fileCount: 25,
            sourceFingerprint: "fp-new",
            generatedAt: 1000,
            model: "openai:gpt-5",
          },
          {
            areaId: "kanzei-core",
            title: "core",
            kind: "crate",
            parent: null,
            band: 1,
            bandLabel: "引擎层",
            depth: 1,
            status: "missing",
            fileCount: 12,
            sourceFingerprint: "fp-core",
            generatedAt: null,
            model: null,
          },
        ],
      }),
      repo_wiki_read: () => ({
        areaId: "overview",
        title: "仓库全局架构总览",
        path: "C:/demo/.kanzei/wiki/pages/overview.md",
        status: "stale",
        content: "# 旧版架构总览\n\n待同步。",
        files: ["crates/kanzei-core/src/lib.rs"],
        outboundDeps: [],
        inboundDeps: [],
        sourceFingerprint: "fp-new",
        generatedAt: 1000,
        model: "openai:gpt-5",
      }),
      repo_wiki_generate: () => ({
        areaId: "overview",
        title: "仓库全局架构总览",
        path: "C:/demo/.kanzei/wiki/pages/overview.md",
        status: "fresh",
        content: "# 最新架构总览\n\n已完成增量同步。",
        files: ["crates/kanzei-core/src/lib.rs"],
        outboundDeps: [],
        inboundDeps: [],
        sourceFingerprint: "fp-new",
        generatedAt: 2000,
        model: "openai:gpt-5",
      }),
    });
    setIpcTransport(transport);
    appStore.setState({ activeProjectDir: "C:/demo", generalMode: false });

    render(<RepoWikiView />);

    await waitFor(() => {
      expect(screen.getByText(/旧版架构总览/)).toBeTruthy();
      expect(screen.getByRole("button", { name: "立即增量同步" })).toBeTruthy();
    });

    fireEvent.click(screen.getByRole("button", { name: "立即增量同步" }));
    await waitFor(() => {
      expect(calls.some((c) => c.command === "repo_wiki_generate" && c.args?.areaId === "overview")).toBe(true);
      expect(screen.getByText(/已完成增量同步/)).toBeTruthy();
    });
  });

  it("ActionReportCard 展示变更与工具统计并支持一键运行代码门禁", async () => {
    const { transport, calls } = createMockTransport({
      worktree_post_merge_gate: () => [
        { name: "cargo check", ok: true, summary: "检查通过" },
        { name: "cargo clippy", ok: true, summary: "零警告" },
      ],
    });
    setIpcTransport(transport);
    appStore.setState({ activeProjectDir: "C:/demo", generalMode: false });
    drawerStore.setState({
      gitStatus: {
        repo: "own",
        branch: "main",
        changes: 1,
        last: null,
        additions: 14,
        deletions: 3,
        files: [{ path: "src/app.ts", additions: 14, deletions: 3 }],
      },
      touchedFiles: ["src/app.ts"],
    });

    render(
      <ActionReportCard
        messages={[]}
        toolResultMap={new Map()}
        report={{
          sessionId: "ses-demo",
          completedAt: 1000,
          elapsedMs: 3200,
          inputTokens: 1200,
          outputTokens: 300,
          toolsTotal: 4,
          toolsFailed: 0,
          toolNames: ["read", "edit"],
          modifiedFiles: ["src/app.ts"],
          touchedFiles: ["src/app.ts"],
        }}
      />
    );

    const receipt = screen.getByRole("button", { name: /本轮交付/ });
    expect(receipt.getAttribute("aria-expanded")).toBe("false");
    fireEvent.click(receipt);
    expect(screen.getByText("工具 4/4")).toBeTruthy();
    expect(screen.getByText("src/app.ts")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "运行门禁" }));
    await waitFor(() => {
      expect(calls.some((c) => c.command === "worktree_post_merge_gate")).toBe(true);
      expect(screen.getByText(/cargo check/)).toBeTruthy();
      expect(screen.getByText(/cargo clippy/)).toBeTruthy();
    });
  });

  it("交付验收单不把工作树原有改动算作本轮交付，纯问答默认隐藏，零项门禁明确提示验证未执行", async () => {
    const { transport } = createMockTransport({
      worktree_post_merge_gate: () => [],
    });
    setIpcTransport(transport);
    appStore.setState({ activeProjectDir: "C:/demo", generalMode: false });
    const gitWithOldChange = {
      repo: "own" as const,
      branch: "main",
      changes: 2,
      last: null,
      additions: 15,
      deletions: 4,
      files: [
        { path: "README.md", additions: 1, deletions: 1 },
        { path: "src/app.ts", additions: 14, deletions: 3 },
      ],
    };
    drawerStore.setState({ gitStatus: gitWithOldChange, touchedFiles: ["README.md"] });

    // 1. 已有未提交改动、无适用检查的仓库里完成纯问答：本轮修改 0 个文件，验证未执行，且默认不展开卡片
    const qaSummary = buildRoundReportSummary(
      [
        { role: "user", parts: [{ type: "text", text: "你好" }] },
        { role: "assistant", parts: [{ type: "text", text: "你好，有什么可以帮你？" }] },
      ],
      new Map(),
      {
        sessionId: "ses-qa",
        completedAt: 1000,
        elapsedMs: 500,
        inputTokens: 100,
        outputTokens: 20,
        toolsTotal: 0,
        toolsFailed: 0,
        toolNames: [],
        touchedFiles: [],
        modifiedFiles: [],
      },
      gitWithOldChange
    );
    expect(qaSummary.roundFiles).toEqual([]);
    expect(qaSummary.otherGitFilesCount).toBe(2);
    expect(qaSummary.shouldDisplay).toBe(false);
    expect(evaluateGateVerdict([]).state).toBe("not_executed");

    const { container: qaContainer } = render(
      <ActionReportCard
        messages={[
          { role: "user", parts: [{ type: "text", text: "你好" }] },
          { role: "assistant", parts: [{ type: "text", text: "你好" }] },
        ]}
        toolResultMap={new Map()}
        report={{
          sessionId: "ses-qa",
          completedAt: 1000,
          elapsedMs: 500,
          inputTokens: 100,
          outputTokens: 20,
          toolsTotal: 0,
          toolsFailed: 0,
          toolNames: [],
          touchedFiles: [],
          modifiedFiles: [],
        }}
      />
    );
    expect(qaContainer.innerHTML).toBe("");

    // 2. 实际修改 src/app.ts 时：不把 README.md 混进本轮修改；零项门禁明确提示“验证未执行”
    render(
      <ActionReportCard
        messages={[]}
        toolResultMap={new Map()}
        report={{
          sessionId: "ses-edit",
          completedAt: 1000,
          elapsedMs: 1200,
          inputTokens: 300,
          outputTokens: 80,
          toolsTotal: 1,
          toolsFailed: 0,
          toolNames: ["edit"],
          touchedFiles: ["src/app.ts"],
          modifiedFiles: ["src/app.ts"],
        }}
      />
    );

    expect(screen.getByText("本轮 1 文件")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /本轮交付/ }));
    expect(screen.getByText("src/app.ts")).toBeTruthy();
    expect(screen.queryByRole("button", { name: /README\.md/ })).toBeNull();
    expect(screen.getByText(/工作树另有 1 个非本轮未提交改动/)).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "运行门禁" }));
    await waitFor(() => {
      expect(screen.getByText("验证未执行（0 项）")).toBeTruthy();
      expect(uiStore.getState().toasts.some((t) => t.text.includes("全部通过"))).toBe(false);
      expect(uiStore.getState().toasts.some((t) => t.text.includes("验证未执行"))).toBe(true);
    });
  });

  it("记忆页支持数组搜索结果、新建携带类别、读取 messages 历史以及诊断摘要折叠 JSON", async () => {
    const stored = [
      {
        id: "M-001",
        scope: "project",
        category: "fact",
        title: "部署端口规范",
        description: "统一使用 8080 端口",
        status: "active",
        body: "服务监听 8080。",
      },
    ];
    const { transport, calls } = createMockTransport({
      memory_entries: (args) => (args?.scope === "project" ? [...stored] : []),
      memory_search_page: (args) =>
        stored.filter((e) => e.title.includes(String(args?.query ?? ""))),
      memory_entry_save: (args) => {
        const created = {
          id: "M-002",
          scope: String(args?.scope ?? "project"),
          category: String(args?.category ?? "fact"),
          title: String(args?.title ?? ""),
          description: String(args?.description ?? ""),
          status: "active",
          body: String(args?.body ?? ""),
        };
        stored.push(created);
        return created;
      },
      memory_chat_history: () => ({
        messages: [
          { role: "user", text: "帮我检查历史记忆" },
          { role: "assistant", text: "已核对 M-001 有效。" },
        ],
        requestId: null,
      }),
      memory_overview: () => ({ scopes: [{ scope: "project", total: 2, integrity: [] }] }),
      memory_control_plane: () => ({
        promotion_gaps: 0,
        recall: { recalled: 3, injected: 2, read: 1, events_orphaned: 0 },
      }),
      memory_context_bill: () => ({ bill: [], episodes: [] }),
      memory_recalls: () => ({
        rounds_total: 1,
        rounds: [
          {
            recall_id: "r1",
            hits: [{ id: "M-001", title: "部署端口规范", category: "fact", injected: true, read: true }],
          },
        ],
      }),
    });
    setIpcTransport(transport);
    appStore.setState({ activeProjectDir: "C:/demo", generalMode: false });

    render(<MemoryView />);

    await waitFor(() => {
      expect(screen.getByText("部署端口规范")).toBeTruthy();
    });

    // 1. 搜索输入完整标题后仍命中（后端返回数组）
    fireEvent.change(screen.getByPlaceholderText("搜索记忆标题或内容…"), {
      target: { value: "部署端口规范" },
    });
    await waitFor(() => {
      expect(calls.some((c) => c.command === "memory_search_page")).toBe(true);
      expect(screen.getByText("部署端口规范")).toBeTruthy();
    });

    // 2. 新建记忆并选择 sop 类别，请求携带 category 且创建成功
    fireEvent.click(screen.getByRole("button", { name: "新建记忆" }));
    const selects = screen.getAllByRole("combobox");
    const categorySelect = selects[selects.length - 1];
    fireEvent.change(categorySelect, { target: { value: "sop" } });
    const inputs = screen.getAllByRole("textbox");
    fireEvent.change(inputs[1], { target: { value: "发布前验证流程" } });
    fireEvent.change(inputs[3], { target: { value: "发布前执行 scripts/verify.ps1 -Full。" } });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => {
      expect(
        calls.some(
          (c) =>
            c.command === "memory_entry_save" &&
            c.args?.id === "" &&
            c.args?.category === "sop" &&
            c.args?.title === "发布前验证流程"
        )
      ).toBe(true);
    });

    // 3. 切到管理对话 Tab，读取后端返回的 messages 字段
    fireEvent.click(screen.getByRole("tab", { name: "管理对话" }));
    await waitFor(() => {
      expect(screen.getByText("帮我检查历史记忆")).toBeTruthy();
      expect(screen.getByText("已核对 M-001 有效。")).toBeTruthy();
    });

    // 4. 切到诊断 Tab，首屏显示可读摘要且原始 JSON 默认折叠
    fireEvent.click(screen.getByRole("tab", { name: "概览与诊断" }));
    await waitFor(() => {
      expect(screen.getByText("最近一次召回与健康摘要")).toBeTruthy();
      expect(screen.getByText("已注入")).toBeTruthy();
      expect(screen.getByText("已读取")).toBeTruthy();
    });
    const details = document.querySelectorAll("details.diag-card");
    expect(details.length).toBeGreaterThan(0);
    details.forEach((el) => {
      expect(el.hasAttribute("open")).toBe(false);
    });
  });

  it("个人 Skill 连续两次编辑使用最新 revision 且保留 userInvocable=false", async () => {
    let version = 1;
    const { transport, calls } = createMockTransport({
      skills_list: () => [
        {
          id: "personal:my-skill",
          name: "my-skill",
          displayName: "my-skill",
          description: "个人流程",
          source: "personal",
          editable: true,
          enabled: true,
          manualOnly: false,
          userInvocable: false,
        },
      ],
      skills_read: () => ({
        id: "personal:my-skill",
        name: "my-skill",
        displayName: "my-skill",
        description: "个人流程",
        source: "personal",
        editable: true,
        enabled: true,
        manualOnly: false,
        userInvocable: false,
        instructions: version === 1 ? "第一版正文" : "第二版正文",
        revision: `rev-${version}`,
      }),
      skills_save: (args) => {
        const draft = args?.draft as Record<string, unknown>;
        version += 1;
        return {
          id: "personal:my-skill",
          name: "my-skill",
          displayName: "my-skill",
          description: String(draft?.description ?? ""),
          source: "personal",
          editable: true,
          enabled: true,
          manualOnly: Boolean(draft?.manualOnly),
          userInvocable: Boolean(draft?.userInvocable),
          instructions: String(draft?.instructions ?? ""),
          revision: `rev-${version}`,
        };
      },
    });
    setIpcTransport(transport);

    render(<SkillsView />);
    await waitFor(() => {
      expect(screen.getByText("个人流程")).toBeTruthy();
    });
    fireEvent.click(screen.getByText("个人流程"));
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "编辑" })).toBeTruthy();
    });

    // 第一次编辑保存
    fireEvent.click(screen.getByRole("button", { name: "编辑" }));
    const bodyBox1 = screen.getByDisplayValue("第一版正文");
    fireEvent.change(bodyBox1, { target: { value: "第二版正文" } });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => {
      expect(
        calls.some(
          (c) =>
            c.command === "skills_save" &&
            c.args?.expectedHash === "rev-1" &&
            (c.args?.draft as Record<string, unknown>)?.userInvocable === false
        )
      ).toBe(true);
    });

    // 紧接着第二次点编辑：必须带出第二版正文与 rev-2
    fireEvent.click(screen.getByRole("button", { name: "编辑" }));
    const bodyBox2 = screen.getByDisplayValue("第二版正文");
    fireEvent.change(bodyBox2, { target: { value: "第三版正文" } });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => {
      expect(
        calls.some(
          (c) =>
            c.command === "skills_save" &&
            c.args?.expectedHash === "rev-2" &&
            (c.args?.draft as Record<string, unknown>)?.userInvocable === false
        )
      ).toBe(true);
    });
  });

  it("设置页添加外部打开方式后底部保存栏标记未保存并可统一保存", async () => {
    const { transport, calls } = createMockTransport({
      settings_get: () => ({
        path: "~/.kanzei/kanzei.toml",
        primary: "openai:gpt-5",
        fast: "",
        compact: "",
        proxy: "env",
        reasoning: "off",
        codexFastMode: false,
        profileDefault: "dev-auto",
        providers: [],
        limits: {},
        limitDefaults: {},
        cadence: {},
        hooks: [],
      }),
      open_tools_list: () => [],
      open_tools_save: () => null,
      fast_model_status: () => ({ managed: false }),
      agent_directory_get: () => ({ agents: [] }),
      update_receipt: () => null,
    });
    setIpcTransport(transport);

    render(<SettingsView />);
    await waitFor(() => {
      expect(screen.getByText("所有设置已同步")).toBeTruthy();
    });

    fireEvent.click(screen.getByRole("button", { name: "添加打开方式" }));
    expect(screen.getByText("存在未保存的外部打开方式修改")).toBeTruthy();
    const saveAllBtn = screen.getByRole("button", { name: "保存全局设置" }) as HTMLButtonElement;
    expect(saveAllBtn.disabled).toBe(false);

    fireEvent.change(screen.getByPlaceholderText("名称，如 Cursor"), {
      target: { value: "Zed" },
    });
    fireEvent.change(screen.getByPlaceholderText("可执行程序路径"), {
      target: { value: "C:/Tools/zed.exe" },
    });
    fireEvent.click(saveAllBtn);

    await waitFor(() => {
      expect(calls.some((c) => c.command === "open_tools_save")).toBe(true);
      expect(screen.getByText("所有设置已同步")).toBeTruthy();
    });
  });

  it("全局收件箱读取失败时显示故障告警而非空态", async () => {
    const { transport } = createMockTransport({
      pending_asks_all: () => {
        throw new Error("IPC 服务连接断开");
      },
    });
    setIpcTransport(transport);
    uiStore.setState({ inboxOpen: true, globalAsks: [], globalAsksError: null, recentCompleted: [] });

    render(<InboxDrawer />);

    await waitFor(() => {
      expect(screen.getByRole("alert")).toBeTruthy();
      expect(screen.getByText(/无法确认是否存在待审批任务：IPC 服务连接断开/)).toBeTruthy();
      expect(screen.queryByText("当前没有待处理的权限请求或提问。")).toBeNull();
    });
  });

  it("工作台移除重复收件箱入口，任务看板不把空白空闲会话列为待验收", async () => {
    const { transport } = createMockTransport({
      pending_asks_all: () => [],
      workspace_overview: () => ({
        current: "C:/demo",
        observed_at: 1000,
        projects: [
          {
            path: "C:/demo",
            project_id: "C:/demo",
            name: "Demo",
            current: true,
            running_lines: 0,
            lines_total: 2,
            lines: [
              {
                id: "p-idle",
                label: "刚新建的空对话",
                title: "刚新建的空对话",
                title_custom: true,
                kind: "task",
                ordinal: 1,
                updated_at: 1000,
                session_id: "ses-idle",
                running: false,
                stage: "空闲",
                branch: "main",
                worktree_path: null,
                profile: "dev-auto",
                available: true,
              },
              {
                id: "p-done",
                label: "已完工重构",
                title: "已完工重构",
                title_custom: true,
                kind: "task",
                ordinal: 2,
                updated_at: 2000,
                session_id: "ses-done",
                running: false,
                stage: "已完成",
                branch: "feat-done",
                worktree_path: null,
                profile: "dev-auto",
                available: true,
              },
            ],
          },
        ],
      }),
    });
    setIpcTransport(transport);
    appStore.setState({
      activeProjectDir: "C:/demo",
      projects: { projects: ["C:/demo"], current: "C:/demo", names: { "C:/demo": "Demo" } },
    });

    render(<WorkspaceView />);
    expect(screen.queryByRole("button", { name: /收件箱/ })).toBeNull();
    expect(screen.queryByText(/显示 1 \/ 1 个项目/)).toBeNull();

    fireEvent.click(screen.getByRole("tab", { name: /My Quests/ }));
    await waitFor(() => {
      expect(screen.getByText("已完工重构")).toBeTruthy();
      expect(screen.queryByText("刚新建的空对话")).toBeNull();
      expect(screen.getByText(/另有 1 条空闲会话未进入任务看板/)).toBeTruthy();
    });
  });

  it("文件保存进行中继续输入时，旧回执不会把新内容误标为已保存", async () => {
    let resolveFirstWrite!: (val: unknown) => void;
    let writeCount = 0;
    const { transport, calls } = createMockTransport({
      file_preview: () => ({
        content: "initial\n",
        hash: "h-init",
        bom: false,
        readonly: null,
        binary: false,
      }),
      file_git_diff: () => ({ path: "src/a.txt", diff: "" }),
      file_write: () => {
        writeCount += 1;
        if (writeCount === 1) {
          return new Promise((resolve) => {
            resolveFirstWrite = resolve;
          });
        }
        return { status: "saved", hash: "h-two", size: 8, mtimeMs: 2000, exists: true, evidence: null };
      },
    });
    setIpcTransport(transport);
    setDrawerContext("C:/demo", null);

    await openFileInDrawer("src/a.txt", { tab: "edit" });
    setDrawerEditContent("edit one");
    const firstSavePromise = saveDrawerFile(false);

    // 在第一次保存回执到达前继续输入第二版
    setDrawerEditContent("edit two");
    resolveFirstWrite({ status: "saved", hash: "h-one", size: 8, mtimeMs: 1000, exists: true, evidence: null });
    expect(await firstSavePromise).toBe(true);

    // 磁盘是 edit one，但编辑器已是 edit two，必须保持 editDirty=true，且基准指纹已推进到 h-one
    expect(drawerStore.getState().editContent).toBe("edit two");
    expect(drawerStore.getState().preview?.content).toBe("edit one");
    expect(drawerStore.getState().editHash).toBe("h-one");
    expect(drawerStore.getState().editDirty).toBe(true);

    // 紧接着保存 edit two 携带 h-one，完成后才清除 editDirty
    expect(await saveDrawerFile(false)).toBe(true);
    expect(drawerStore.getState().editDirty).toBe(false);
    expect(drawerStore.getState().editHash).toBe("h-two");
    expect(
      calls.some(
        (c) =>
          c.command === "file_write" &&
          c.args?.content === "edit two" &&
          c.args?.expectedHash === "h-one"
      )
    ).toBe(true);
  });

  it("同会话连续草稿保存严格串行，且退出刷盘会等待在途写入全部完成", async () => {
    const resolvers: Array<() => void> = [];
    const writeOrder: string[] = [];
    let persistedText = "";
    const { transport } = createMockTransport({
      composer_draft_save: (args) =>
        new Promise((resolve) => {
          const text = String((args?.draft as { text?: string } | null)?.text ?? "");
          writeOrder.push(text);
          resolvers.push(() => {
            persistedText = text;
            resolve(null);
          });
        }),
    });
    setIpcTransport(transport);

    const key = draftKeyFor("C:/demo", "p-1");
    updateDraft("C:/demo", "p-1", "draft v1", []);
    const firstPersist = persistDraftNow(key);
    expect(writeOrder).toEqual(["draft v1"]);

    // 第一次写入尚在途时，连续更新到 v2 并模拟退出窗口调用 flushPendingDraftSaves
    updateDraft("C:/demo", "p-1", "draft v2", []);
    let flushed = false;
    const flushPromise = flushPendingDraftSaves().then(() => {
      flushed = true;
    });

    await Promise.resolve();
    await Promise.resolve();
    // 第一次写入未完成前：第二次不得并发乱序发出，且 flush 绝不能提前返回
    expect(writeOrder).toEqual(["draft v1"]);
    expect(flushed).toBe(false);

    // 放行第一次写入 -> 自动串行发出第二次写入，此时 flush 仍需等待第二次写入完成
    resolvers[0]();
    await waitFor(() => {
      expect(writeOrder).toEqual(["draft v1", "draft v2"]);
    });
    expect(flushed).toBe(false);

    // 放行第二次写入 -> firstPersist 与 flush 一起完成，且落盘最终一定是最新文本 draft v2
    resolvers[1]();
    await Promise.all([firstPersist, flushPromise]);
    expect(flushed).toBe(true);
    expect(persistedText).toBe("draft v2");
  });

  it("交付验收单不把同文件相对 HEAD 的累计增删行数算作本轮行数", async () => {
    vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
    let exportedContent = "";
    const { transport } = createMockTransport({
      save_conversation_export: (args) => {
        exportedContent = String(args?.content ?? "");
        return "C:/demo/report.md";
      },
    });
    setIpcTransport(transport);
    appStore.setState({ activeProjectDir: "C:/demo", generalMode: false });
    drawerStore.setState({
      gitStatus: {
        repo: "own",
        branch: "main",
        changes: 1,
        last: null,
        additions: 101,
        deletions: 0,
        files: [{ path: "src/app.ts", additions: 101, deletions: 0 }],
      },
    });

    render(
      <ActionReportCard
        messages={[]}
        toolResultMap={new Map()}
        report={{
          sessionId: "ses-lines",
          completedAt: 1000,
          elapsedMs: 800,
          inputTokens: 200,
          outputTokens: 50,
          toolsTotal: 1,
          toolsFailed: 0,
          toolNames: ["edit"],
          touchedFiles: ["src/app.ts"],
          modifiedFiles: ["src/app.ts"],
        }}
      />
    );

    expect(screen.getByText("本轮 1 文件")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /本轮交付/ }));
    expect(screen.getByText("src/app.ts")).toBeTruthy();
    expect(screen.queryByText(/\+101/)).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "更多交付操作" }));
    fireEvent.click(screen.getByRole("button", { name: "导出" }));
    await waitFor(() => {
      expect(exportedContent).toContain("`src/app.ts`");
      expect(exportedContent).not.toContain("+101");
    });
  });

  it("Diff 回退此块正确处理末尾无换行标记，恢复后与原文件逐字节一致", () => {
    // 1. 原文件无末尾换行，修改并加上末尾换行后回退：必须去掉多余换行
    const diffAddedNewline = [
      "@@ -1 +1 @@",
      "-alpha",
      "\\ No newline at end of file",
      "+beta",
    ].join("\n");
    const hunks1 = parseUnifiedDiffHunks(diffAddedNewline).hunks;
    expect(applyReverseHunk("beta\n", hunks1[0])).toBe("alpha");
    expect(applyReverseHunk("beta\r\n", hunks1[0])).toBe("alpha");

    // 2. 原文件有末尾换行，修改并删掉末尾换行后回退：必须补回末尾换行
    const diffRemovedNewline = [
      "@@ -1 +1 @@",
      "-alpha",
      "+beta",
      "\\ No newline at end of file",
    ].join("\n");
    const hunks2 = parseUnifiedDiffHunks(diffRemovedNewline).hunks;
    expect(applyReverseHunk("beta", hunks2[0])).toBe("alpha\n");

    // 3. 原文件与新文件均无末尾换行：回退保持无末尾换行
    const diffBothNoNewline = [
      "@@ -1 +1 @@",
      "-alpha",
      "\\ No newline at end of file",
      "+beta",
      "\\ No newline at end of file",
    ].join("\n");
    const hunks3 = parseUnifiedDiffHunks(diffBothNoNewline).hunks;
    expect(applyReverseHunk("beta", hunks3[0])).toBe("alpha");
  });
});
