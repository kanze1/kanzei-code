import { COMMAND_PARAMETERS, type CommandArgsMap, type CommandName } from "./generated";

export interface IpcTransport {
  invoke<T = unknown>(command: string, args?: Record<string, unknown>): Promise<T>;
  listen<T = unknown>(event: string, handler: (payload: T) => void): Promise<() => void>;
}

interface GlobalTauriApi {
  core?: {
    invoke<T = unknown>(command: string, args?: Record<string, unknown>): Promise<T>;
  };
  event?: {
    listen<T = unknown>(
      event: string,
      handler: (ev: { payload: T }) => void
    ): Promise<() => void>;
  };
  window?: {
    getCurrentWindow(): TauriCurrentWindow;
  };
}

export interface TauriCurrentWindow {
  minimize(): Promise<void>;
  toggleMaximize(): Promise<void>;
  isMaximized(): Promise<boolean>;
  startDragging(): Promise<void>;
  close(): Promise<void>;
  onResized(handler: () => void): Promise<() => void>;
}

declare global {
  interface Window {
    __TAURI__?: GlobalTauriApi;
    __KANZEI_MOCK_TRANSPORT__?: IpcTransport;
  }
}

let customTransport: IpcTransport | null = null;
const errorListeners = new Set<(command: string, message: string) => void>();

export function setIpcTransport(transport: IpcTransport | null): void {
  customTransport = transport;
}

export function onIpcError(listener: (command: string, message: string) => void): () => void {
  errorListeners.add(listener);
  return () => errorListeners.delete(listener);
}

function getTransport(): IpcTransport | null {
  if (customTransport) return customTransport;
  if (typeof window !== "undefined" && window.__KANZEI_MOCK_TRANSPORT__) {
    return window.__KANZEI_MOCK_TRANSPORT__;
  }
  const tauri = typeof window !== "undefined" ? window.__TAURI__ : undefined;
  if (tauri?.core?.invoke) {
    return {
      invoke: (command, args) => tauri.core!.invoke(command, args),
      listen: async (event, handler) => {
        if (!tauri.event?.listen) return () => {};
        return tauri.event.listen(event, (ev) => handler(ev.payload as never));
      },
    };
  }
  return null;
}

export function hasDesktopRuntime(): boolean {
  return getTransport() !== null;
}

export function getCurrentWindow(): TauriCurrentWindow | null {
  if (typeof window === "undefined") return null;
  return window.__TAURI__?.window?.getCurrentWindow?.() ?? null;
}

function validateArgs(command: CommandName, args: Record<string, unknown> | undefined): void {
  const schema = COMMAND_PARAMETERS[command];
  if (!schema) return;
  const payload = args ?? {};
  for (const key of Object.keys(payload)) {
    if (!(key in schema)) {
      throw new Error(`IPC 参数未在契约中声明: ${command}.${key}`);
    }
  }
  for (const [key, required] of Object.entries(schema)) {
    if (required && payload[key] === undefined) {
      throw new Error(`IPC 缺少必填参数: ${command}.${key}`);
    }
  }
}

export function formatError(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  try {
    return JSON.stringify(error);
  } catch {
    return String(error);
  }
}

export async function invoke<T = unknown, K extends CommandName = CommandName>(
  command: K,
  ...rest: Record<string, never> extends CommandArgsMap[K]
    ? [args?: CommandArgsMap[K]]
    : [args: CommandArgsMap[K]]
): Promise<T> {
  const args = (rest[0] ?? {}) as Record<string, unknown>;
  validateArgs(command, args);
  const transport = getTransport();
  if (!transport) {
    throw new Error(`桌面运行时未就绪，无法调用 ${command}`);
  }
  try {
    return await transport.invoke<T>(command, args);
  } catch (error) {
    const message = formatError(error);
    for (const listener of errorListeners) {
      listener(command, message);
    }
    throw new Error(message);
  }
}

export async function listenEvent<T = unknown>(
  event: string,
  handler: (payload: T) => void
): Promise<() => void> {
  const transport = getTransport();
  if (!transport) return () => {};
  return transport.listen<T>(event, handler);
}
