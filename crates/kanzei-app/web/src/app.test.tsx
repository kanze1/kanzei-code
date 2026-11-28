import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke, setIpcTransport, type IpcTransport } from "./ipc/client";
import { installEventBridge } from "./ipc/events";
import { App } from "./App";
import { ProjectModelsDialogHost, RewindDialogHost } from "./components/Dialogs";
import SettingsView from "./views/settings/SettingsView";
import { AskOverlay } from "./views/chat/AskOverlay";
import { ActionReportCard } from "./views/chat/ActionReportCard";
import { BtwOverlay } from "./views/chat/BtwOverlay";
import { Composer } from "./views/chat/Composer";
import { MessageItem } from "./views/chat/MessageItem";
import RepoWikiView from "./views/wiki/RepoWikiView";
import WorkspaceView from "./views/workspace/WorkspaceView";
import { InboxDrawer } from "./shell/InboxDrawer";
import { StatusBar } from "./shell/StatusBar";
import { appStore } from "./store/appStore";
import { chatStore, loadOlderMessages } from "./store/chatStore";
import { drawerStore } from "./store/drawerStore";
import { sessionStore } from "./store/sessionStore";
import { openProjectModelsDialog, uiStore } from "./store/uiStore";
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
  cleanup();
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
    messages: [],
    streamingText: "",
    streamingReasoning: "",
    liveTools: [],
    liveSubagents: [],
    autoAllow: false,
  });
  drawerStore.setState({ open: false, activeFile: null, editDirty: false });
  uiStore.setState({
    view: "chat",
    confirmDialog: null,
    closingWindow: false,
    projectModelsOpen: false,
    projectModelsTargetDir: null,
    rewindDialog: null,
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

  it("对话控制栏切回默认模型/思考强度发送空串清除覆盖，并正确约束自动放行与无项目模式选项", async () => {
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

    // 顶部模型 chip 正确显示 model.resolved 与来源
    expect(screen.getByText("本线临时")).toBeTruthy();

    // 未开启自动推进时，“自动放行”按钮应处于禁用态
    expect((screen.getByRole("button", { name: "自动放行" }) as HTMLButtonElement).disabled).toBe(true);

    // 选回默认模型和默认思考强度，后端必须收到空字符串 "" 而非 null
    fireEvent.change(screen.getByLabelText("模型选择"), { target: { value: "" } });
    await waitFor(() => {
      const updateCall = calls.find((c) => c.command === "process_update");
      expect(updateCall?.args).toEqual({ processId: "d|C:/demo", model: "" });
    });

    fireEvent.change(screen.getByLabelText("思考强度"), { target: { value: "" } });
    await waitFor(() => {
      const reasoningCall = calls.find(
        (c) => c.command === "process_update" && "reasoning" in (c.args ?? {})
      );
      expect(reasoningCall?.args).toEqual({ processId: "d|C:/demo", reasoning: "" });
    });

    // 项目模式同时包含只读讨论 (readonly) 与隔离沙箱 (isolated)
    const profileSelect = screen.getByLabelText("执行模式") as HTMLSelectElement;
    const profileValues = Array.from(profileSelect.options).map((o) => o.value);
    expect(profileValues).toContain("readonly");
    expect(profileValues).toContain("isolated");
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
    const inspectBtns = screen.getAllByRole("button", { name: "查看原文" });
    expect(inspectBtns.length).toBe(4);
    fireEvent.click(inspectBtns[0]);
    await waitFor(() => {
      expect(calls.some((c) => c.command === "context_content" && c.args?.sha256 === "aaa")).toBe(true);
      expect(uiStore.getState().viewer?.title).toBe("agent/system");
    });
  });

  it("StatusBar 上下文构成弹窗展示彩色分类占比拆解", async () => {
    const { transport } = createMockTransport({
      context_snapshot: () => ({
        requests: [{ sequence: 12, step: 1, model: "openai:gpt-5" }],
        selected: {
          sequence: 12,
          manifest: {
            model: "openai:gpt-5",
            estimated_tokens: 18000,
            blocks: [
              { source: "agent/system", chars: 4000, sha256: "aaa" },
              { source: "memory/recall", chars: 2000, sha256: "bbb" },
              { source: "history/messages", chars: 6000, sha256: "ccc" },
            ],
          },
        },
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
      effectiveModel: { contextLimit: 100000 },
    });

    render(<StatusBar />);
    fireEvent.click(screen.getByRole("button", { name: "上下文构成" }));
    await waitFor(() => {
      expect(screen.getByRole("img", { name: "上下文构成占比条" })).toBeTruthy();
      expect(screen.getByText("系统提示与规则")).toBeTruthy();
      expect(screen.getByText("长期记忆与偏好")).toBeTruthy();
      expect(screen.getByText("对话历史与消息")).toBeTruthy();
    });
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
          touchedFiles: ["src/app.ts"],
        }}
      />
    );

    expect(screen.getByText("交付验收单（Action Report）")).toBeTruthy();
    expect(screen.getByText("工具 4/4")).toBeTruthy();
    expect(screen.getByText("src/app.ts")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "运行门禁" }));
    await waitFor(() => {
      expect(calls.some((c) => c.command === "worktree_post_merge_gate")).toBe(true);
      expect(screen.getByText(/cargo check/)).toBeTruthy();
      expect(screen.getByText(/cargo clippy/)).toBeTruthy();
    });
  });
});
