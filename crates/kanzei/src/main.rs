//! kz CLI 入口(R-256 批3):命令分发 + 装配。
//!
//! 独立理由:命令分发收敛到 `cli::main_entry`(原 main.rs 的 match),各子命令
//! (run/eval/tracker/work/config/worktree/lock/memory)与共享 helper(取根/身份键/
//! 交互判定/run 参数解析)在 `cli/` 目录;本文件只留入口,加一条命令不必读懂
//! 另一条的装配(照 files_view.rs 模式)。

mod cli;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    cli::main_entry(&args).await
}
