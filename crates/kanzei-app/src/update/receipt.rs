//! Durable update result. Installer exit is an intermediate state; only the
//! build actually running after restart can confirm completion.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Phase {
    Prepared,
    Installing,
    AwaitingRestart,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct UpdateReceipt {
    pub(crate) expected_build: String,
    pub(crate) source_build: String,
    pub(crate) started_at_ms: u128,
    pub(crate) installed_exe: PathBuf,
    pub(super) phase: Phase,
    pub(crate) message: String,
    pub(super) owner_pid: u32,
    pub(super) owner_started_at: u64,
}

impl UpdateReceipt {
    pub(super) fn transition(&mut self, phase: Phase, message: String) {
        self.phase = phase;
        self.message = message;
    }

    pub(super) fn reconcile(&mut self, running_build: &str, owner_alive: bool) -> bool {
        if self.phase == Phase::Completed {
            return false;
        }
        if running_build == self.expected_build {
            self.transition(
                Phase::Completed,
                format!("已更新到构建 {}，当前运行版本已核验", self.expected_build),
            );
            return true;
        }
        match self.phase {
            Phase::AwaitingRestart => {
                self.transition(Phase::Failed, format!("安装器已退出，但当前运行构建为 {running_build}，预期为 {}。请从 {} 启动并核对安装结果。", self.expected_build, self.installed_exe.display()));
                true
            }
            Phase::Prepared | Phase::Installing if !owner_alive => {
                self.transition(
                    Phase::Failed,
                    "更新进程已结束，未留下完成记录。请查看更新日志后重新安装。".into(),
                );
                true
            }
            _ => false,
        }
    }
}

pub(super) fn receipt_path() -> Result<PathBuf, String> {
    kanzei_harness::kanzei_home()
        .map(|home| home.join("update/receipt.json"))
        .ok_or_else(|| "无法定位更新结果存储目录".into())
}

pub(super) fn read(path: &Path) -> Result<Option<UpdateReceipt>, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| format!("读取更新结果失败: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("读取更新结果失败: {e}")),
    }
}

pub(super) fn write(path: &Path, receipt: &UpdateReceipt) -> Result<(), String> {
    let _lock = kanzei_base::atomic_file::lock_exclusive(path).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec_pretty(receipt).map_err(|e| e.to_string())?;
    kanzei_base::atomic_file::write_atomic_bytes(path, &bytes)
        .map_err(|e| format!("保存更新结果失败: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pending(phase: Phase) -> UpdateReceipt {
        UpdateReceipt {
            expected_build: "1234567".into(),
            source_build: "abcdef0".into(),
            started_at_ms: 0,
            installed_exe: "kzapp.exe".into(),
            phase,
            message: String::new(),
            owner_pid: 0,
            owner_started_at: 0,
        }
    }
    #[test]
    fn installer_success_requires_running_expected_build() {
        let mut result = pending(Phase::AwaitingRestart);
        assert!(result.reconcile("abcdef0", false));
        assert_eq!(result.phase, Phase::Failed);
        assert!(result.message.contains("预期为 1234567"));
        assert!(result.reconcile("1234567", false));
        assert_eq!(result.phase, Phase::Completed);
    }
    #[test]
    fn live_installer_stays_pending_and_abandoned_attempt_fails() {
        for phase in [Phase::Prepared, Phase::Installing] {
            let mut result = pending(phase);
            // Prepared belongs to the helper before the parent exits, so a
            // new window during process startup keeps the live handoff intact.
            assert!(!result.reconcile("abcdef0", true));
            assert!(result.reconcile("abcdef0", false));
            assert_eq!(result.phase, Phase::Failed);
        }
    }
    #[test]
    fn receipt_roundtrips_and_corruption_is_not_hidden() {
        let dir = std::env::temp_dir().join(format!(
            "kz-receipt-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = dir.join("receipt.json");
        assert!(read(&path).unwrap().is_none());
        write(&path, &pending(Phase::AwaitingRestart)).unwrap();
        assert_eq!(read(&path).unwrap().unwrap().expected_build, "1234567");
        std::fs::write(&path, b"broken").unwrap();
        assert!(read(&path).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
