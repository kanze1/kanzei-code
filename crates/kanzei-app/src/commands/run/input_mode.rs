//! Promoted queue inputs carry their own execution contract, rather than inheriting
//! the first turn's autonomous mode for the whole scheduler drain.

use std::path::Path;

use anyhow::{ensure, Context};
use kanzei_core::{AdmittedInput, SessionStore};

#[derive(Debug, PartialEq, Eq)]
pub(super) struct InputExecution {
    pub execution_batch: bool,
    pub autonomous: bool,
    pub callback: bool,
}

pub(super) fn resolve(
    root: &Path,
    session_id: &str,
    input: Option<&AdmittedInput>,
    execution_batch: bool,
    autonomous: bool,
) -> anyhow::Result<InputExecution> {
    let Some(input) = input else {
        return Ok(InputExecution {
            execution_batch,
            autonomous,
            callback: false,
        });
    };
    ensure!(input.session_id == session_id, "排队输入不属于当前会话");
    let state_path = kanzei_core::project_state_path(root);
    ensure!(
        state_path.is_file(),
        "排队输入运行模式读取失败：项目状态库不存在"
    );
    let store = SessionStore::open(&state_path).context("排队输入运行模式读取失败")?;
    ensure!(
        store
            .input_status(&input.input_id)
            .context("排队输入元数据读取失败")?
            .is_some(),
        "排队输入运行模式读取失败：输入记录不存在"
    );
    let batch = store
        .input_is_execution_batch(session_id, &input.input_id)
        .context("排队输入运行模式读取失败")?;
    let callback = input.input_id.starts_with("async:");
    Ok(InputExecution {
        execution_batch: batch,
        autonomous: !callback && autonomous && batch,
        callback,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_core::Delivery;
    use std::path::PathBuf;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "kz-queued-input-mode-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let store = SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
            store
                .create_session("s", &root.display().to_string(), None)
                .unwrap();
            Self(root)
        }

        fn store(&self) -> SessionStore {
            SessionStore::open(&kanzei_core::project_state_path(&self.0)).unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    #[test]
    fn queued_research_does_not_inherit_autonomy_from_an_execution_round() {
        let f = Fixture::new();
        let store = f.store();
        store
            .admit_batch_input("s", "implementation", "implement", Delivery::Queue)
            .unwrap();
        store
            .admit_input("s", "research", "prepare prior art", Delivery::Queue)
            .unwrap();
        let first = store.promote_next_input("s").unwrap().unwrap();
        assert_eq!(first.input_id, "implementation");
        let original = resolve(&f.0, "s", Some(&first), true, true).unwrap();
        assert!(original.execution_batch && original.autonomous);
        store.finish_input(&first.input_id, true).unwrap();
        let research = store.promote_next_input("s").unwrap().unwrap();
        assert_eq!(research.input_id, "research");
        let mode = resolve(&f.0, "s", Some(&research), true, true).unwrap();
        assert_eq!(
            mode,
            InputExecution {
                execution_batch: false,
                autonomous: false,
                callback: false
            }
        );
        assert_eq!(
            store.input_status("research").unwrap().as_deref(),
            Some("promoted")
        );
    }

    #[test]
    fn explicit_queued_batch_and_work_binding_keep_their_own_contract() {
        let f = Fixture::new();
        let store = f.store();
        store
            .admit_batch_input("s", "batch", "continue batch", Delivery::Queue)
            .unwrap();
        store
            .admit_work_input("s", "work", "continue R-1", "R-001")
            .unwrap();
        for id in ["batch", "work"] {
            let input = store.promote_next_input("s").unwrap().unwrap();
            assert_eq!(input.input_id, id);
            let manual = resolve(&f.0, "s", Some(&input), false, false).unwrap();
            assert!(manual.execution_batch);
            assert!(!manual.autonomous);
            let automatic = resolve(&f.0, "s", Some(&input), false, true).unwrap();
            assert!(automatic.execution_batch && automatic.autonomous);
            store.finish_input(&input.input_id, true).unwrap();
        }
        assert_eq!(
            store.input_work_item("s", "work").unwrap().as_deref(),
            Some("R-001")
        );
    }

    #[test]
    fn fresh_direct_submission_keeps_explicit_autonomous_and_batch_options() {
        for (batch, autonomous) in [(false, false), (true, false), (false, true), (true, true)] {
            let mode = resolve(Path::new("unused"), "s", None, batch, autonomous).unwrap();
            assert_eq!(
                mode,
                InputExecution {
                    execution_batch: batch,
                    autonomous,
                    callback: false
                }
            );
        }
    }

    #[test]
    fn async_callback_keeps_pipeline_suppression_without_weakening_persisted_batch() {
        let f = Fixture::new();
        let store = f.store();
        store
            .admit_batch_input("s", "async:reply", "saved answer", Delivery::Queue)
            .unwrap();
        let input = store.promote_next_input("s").unwrap().unwrap();
        let mode = resolve(&f.0, "s", Some(&input), true, true).unwrap();
        assert_eq!(
            mode,
            InputExecution {
                execution_batch: true,
                autonomous: false,
                callback: true
            }
        );
    }

    #[test]
    fn missing_metadata_is_an_error_instead_of_a_guessed_batch_mode() {
        let f = Fixture::new();
        let unknown = AdmittedInput {
            input_id: "missing".into(),
            session_id: "s".into(),
            prompt: "prepare".into(),
            delivery: Delivery::Queue,
            created_at: 0,
        };
        for (batch, autonomous) in [(false, false), (true, true)] {
            let error = resolve(&f.0, "s", Some(&unknown), batch, autonomous).unwrap_err();
            assert!(error.to_string().contains("输入记录不存在"));
        }
    }

    #[test]
    fn corrupt_metadata_stops_both_preparation_and_execution_modes() {
        let f = Fixture::new();
        let store = f.store();
        store
            .admit_input("s", "research", "prepare", Delivery::Queue)
            .unwrap();
        let input = store.promote_next_input("s").unwrap().unwrap();
        drop(store);
        std::fs::write(
            kanzei_core::project_state_path(&f.0),
            b"not a sqlite database",
        )
        .unwrap();
        for (batch, autonomous) in [(false, false), (true, true)] {
            let error = resolve(&f.0, "s", Some(&input), batch, autonomous).unwrap_err();
            assert!(error.to_string().contains("运行模式读取失败"));
        }
    }
}
