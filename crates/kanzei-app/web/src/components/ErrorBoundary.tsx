import React from "react";
import { appendLog } from "../store/uiStore";

export interface ErrorBoundaryProps {
  scope: string;
  children: React.ReactNode;
  resetKeys?: ReadonlyArray<unknown>;
  onReset?: () => void;
  resetLabel?: string;
  compact?: boolean;
}

interface ErrorBoundaryState {
  error: Error | null;
}

function resetKeysChanged(
  prev: ReadonlyArray<unknown> | undefined,
  next: ReadonlyArray<unknown> | undefined
): boolean {
  if (prev === next) return false;
  if (!prev || !next || prev.length !== next.length) return true;
  for (let i = 0; i < prev.length; i += 1) {
    if (!Object.is(prev[i], next[i])) return true;
  }
  return false;
}

export class ErrorBoundary extends React.Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: unknown): ErrorBoundaryState {
    if (error instanceof Error) return { error };
    return { error: new Error(String(error ?? "未知渲染异常")) };
  }

  componentDidCatch(error: Error): void {
    appendLog(`[界面隔离] ${this.props.scope} 渲染异常: ${error.message}`, "error");
  }

  componentDidUpdate(prevProps: ErrorBoundaryProps): void {
    if (this.state.error && resetKeysChanged(prevProps.resetKeys, this.props.resetKeys)) {
      this.setState({ error: null });
    }
  }

  private handleReset = (): void => {
    this.setState({ error: null });
    this.props.onReset?.();
  };

  render(): React.ReactNode {
    const { error } = this.state;
    if (!error) return this.props.children;
    const { scope, compact, resetLabel } = this.props;
    if (compact) {
      return (
        <div
          role="alert"
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            gap: 8,
            padding: "8px 12px",
            border: "1px solid var(--err, #ef4444)",
            borderRadius: 6,
            fontSize: 12,
          }}
        >
          <span>
            {scope}渲染异常：{error.message}
          </span>
          <button type="button" className="btn btn-sm" onClick={this.handleReset}>
            {resetLabel ?? "重试"}
          </button>
        </div>
      );
    }
    return (
      <div className="empty-state" role="alert" style={{ flex: 1, padding: "24px 16px" }}>
        <h3>{scope}暂时不可用</h3>
        <p style={{ color: "var(--err)", maxWidth: 520, wordBreak: "break-word" }}>
          {error.message}
        </p>
        <p className="field-hint">主窗口与其他会话未受影响，你可以重试或切换到其他视图。</p>
        <div style={{ display: "flex", gap: 8, marginTop: 8 }}>
          <button type="button" className="btn btn-primary btn-sm" onClick={this.handleReset}>
            {resetLabel ?? "重新加载该区域"}
          </button>
        </div>
      </div>
    );
  }
}
