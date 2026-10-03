//! 托管写入窗口(D-174):把"这次托管路径变化是不是合法写入"变成可机械判定的事实。
//!
//! 后台任务的围栏是结果侧的——守卫比对托管树,变了就回滚。但托管树也会被**专用
//! 文档工具**合法改动(`defect` 写 defects.md、`memory_add` 写 .kanzei/memory/…),
//! 守卫必须能区分二者,否则合法写入会被误伤(D-174 验收③)。
//!
//! 区分依据不是猜内容,而是**执行事实**:专用工具执行期间,本模块开一个窗口,
//! 声明"此刻允许改动哪些托管前缀"。窗口关闭时把这些前缀内的变化**吸收**进后台
//! 守卫的基线;窗口之外的任何托管变化都没有合法解释,归后台任务。
//!
//! 为什么是进程级共享而不是 task-local(对照 `progress` 模块):进度是"谁在报、
//! 报给谁"的点对点接力,task-local 天然合适;而后台守卫跑在**另一个 tokio 任务**
//! 里,它要观察的是"此刻整个进程有没有专用工具在写",task-local 跨任务看不见。
//!
//! 本模块提供窗口身份与项目归一；文件快照/回滚由 kanzei-tools 侧持有
//! (harness 不依赖 tools,吸收动作经 `set_observer` 注入的回调回调过去)。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// 一个专用文档工具被允许改动的托管路径前缀(相对项目根,'/' 分隔)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManagedWriterSpec {
    pub tool: &'static str,
    pub prefixes: &'static [&'static str],
}

/// 托管文档的专用写通道白名单。
///
/// 粒度选择(有意为之):按**托管根**而不是按单个文件声明。两类错误的代价不对称——
/// 声明过窄会把专用工具的合法写入判成越界并回滚(误伤、可能丢数据),声明过宽只是
/// 要求后台进程恰好在某次专用工具执行的毫秒窗口内、且写同一棵托管子树才能蒙混。
/// 前者严重得多,所以宁可宽。真正承重的判据是"此刻有没有专用工具在执行",
/// 前缀只是二次收窄。
///
/// 不在表内的工具即使写了托管路径也算越界——尤其是 `bash` 自己:前台 bash 有
/// D-173 的执行前后围栏管自己的副作用,把它放进白名单等于给后台越界开了个庇护所。
/// `write`/`edit` 对托管路径本就是权限硬 deny,同样不入表。
const MANAGED_WRITERS: &[ManagedWriterSpec] = &[
    // tracker 四件套(名字来自 profiles.rs 的 tool_name)与架构索引、测试记录,
    // 全部落在 .kanzei/project/ 下(含各自的 -archive.md 与 architecture/ 子树)。
    ManagedWriterSpec {
        tool: "req",
        prefixes: &[".kanzei/project/"],
    },
    ManagedWriterSpec {
        tool: "defect",
        prefixes: &[".kanzei/project/"],
    },
    ManagedWriterSpec {
        tool: "idea",
        prefixes: &[".kanzei/project/"],
    },
    ManagedWriterSpec {
        tool: "decision",
        prefixes: &[".kanzei/project/"],
    },
    ManagedWriterSpec {
        tool: "architecture",
        prefixes: &[".kanzei/project/"],
    },
    ManagedWriterSpec {
        tool: "test_record",
        prefixes: &[".kanzei/project/"],
    },
    // memory 写工具:条目落 .kanzei/memory/,索引与旧版 memory.md 落 .kanzei/project/。
    // memory_search / memory_stats 是只读的,不入表。
    ManagedWriterSpec {
        tool: "memory_add",
        prefixes: &[".kanzei/memory/", ".kanzei/project/memory.md"],
    },
    ManagedWriterSpec {
        tool: "memory_update",
        prefixes: &[".kanzei/memory/", ".kanzei/project/memory.md"],
    },
    ManagedWriterSpec {
        tool: "memory_merge",
        prefixes: &[".kanzei/memory/", ".kanzei/project/memory.md"],
    },
    ManagedWriterSpec {
        tool: "memory_promote",
        prefixes: &[".kanzei/memory/", ".kanzei/project/memory.md"],
    },
    ManagedWriterSpec {
        tool: "memory_stale",
        prefixes: &[".kanzei/memory/", ".kanzei/project/memory.md"],
    },
    ManagedWriterSpec {
        tool: "memory_inbox_clear",
        prefixes: &[".kanzei/memory/", ".kanzei/project/memory.md"],
    },
    ManagedWriterSpec {
        tool: "memory_note",
        prefixes: &[".kanzei/memory/", ".kanzei/project/memory.md"],
    },
];

pub fn writer_spec(tool: &str) -> Option<&'static ManagedWriterSpec> {
    MANAGED_WRITERS.iter().find(|spec| spec.tool == tool)
}

/// 每次工具调用的窗口身份；同名并发调用也必须配对各自的打开快照。
#[derive(Debug, Clone)]
pub struct ManagedWriteWindow {
    pub id: u64,
    pub project_root: PathBuf,
    pub spec: &'static ManagedWriterSpec,
    project_key: String,
}

impl ManagedWriteWindow {
    pub fn applies_to(&self, project_root: &Path) -> bool {
        let root = kanzei_base::path_form::canonical_or_simplified(project_root);
        self.project_key == crate::project_root::dir_key(&root)
    }
}

/// 当前开着的窗口，按项目和调用身份区分。
fn active() -> &'static Mutex<Vec<ManagedWriteWindow>> {
    static ACTIVE: OnceLock<Mutex<Vec<ManagedWriteWindow>>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(Vec::new()))
}

/// 窗口生命周期阶段。打开与关闭各通知一次,让吸收方在两侧都能拍快照:
/// D-258 的精确吸收需要「打开前」与「关闭后」两张镜像来算窗口内实际变化的路径。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowPhase {
    Opened,
    Closed,
}

type Observer = Box<dyn Fn(WindowPhase, &ManagedWriteWindow) + Send + Sync>;

fn observer() -> &'static OnceLock<Observer> {
    static OBSERVER: OnceLock<Observer> = OnceLock::new();
    &OBSERVER
}

/// 注入窗口观察者。kanzei-tools 在登记首个后台任务时安装:
/// harness 不依赖 tools,快照与基线都在 tools 侧,所以吸收动作只能回调过去。
/// 幂等——重复安装保留首次注入的实现。
///
/// `Opened` 在窗口登记之后、工具执行之前回调;`Closed` 在窗口注销**之前**回调
/// (此刻窗口仍对守卫可见,守卫采样会把窗口覆盖的变化当合法分流,不会误伤)。
pub fn set_observer(f: impl Fn(WindowPhase, &ManagedWriteWindow) + Send + Sync + 'static) {
    let _ = observer().set(Box::new(f));
}

/// 合法写入窗口。Drop 时关闭并触发吸收——RAII 保证正常返回、错误、超时取消
/// (future 被 drop)任何路径都不会把窗口永久开着。
pub struct ToolWindow {
    window: Option<ManagedWriteWindow>,
}

impl ToolWindow {
    /// 非专用写工具返回一个空窗口(Drop 无副作用),调用方不必分支。
    pub fn open(project_root: &Path, tool: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let window = writer_spec(tool).map(|spec| {
            let project_root = kanzei_base::path_form::canonical_or_simplified(project_root);
            ManagedWriteWindow {
                id: NEXT.fetch_add(1, Ordering::Relaxed),
                project_key: crate::project_root::dir_key(&project_root),
                project_root,
                spec,
            }
        });
        if let Some(window) = &window {
            active().lock().unwrap().push(window.clone());
            // 登记之后再通知 Opened:观察者此刻拍到的快照就是"工具还没写"的状态。
            if let Some(observe) = observer().get() {
                observe(WindowPhase::Opened, window);
            }
        }
        ToolWindow { window }
    }
}

impl Drop for ToolWindow {
    fn drop(&mut self) {
        let Some(window) = &self.window else {
            return;
        };
        // 吸收在弹窗**之前**做:此刻窗口仍登记在 active 里,守卫采样会把窗口覆盖的
        // 变化当合法分流(配合守卫"不整树推进"),吸收完成后才弹窗——避免「关窗 →
        // 吸收完成」之间守卫把专用工具的合法写入误判成越界回滚(D-258 时序竞态)。
        if let Some(observe) = observer().get() {
            observe(WindowPhase::Closed, window);
        }
        {
            let mut guard = active().lock().unwrap();
            if let Some(at) = guard.iter().position(|other| other.id == window.id) {
                guard.remove(at);
            }
        }
    }
}

/// 在合法写入窗口内执行工具 future。非专用写工具零开销通过。
pub async fn tool_scope<F: std::future::Future>(
    project_root: &Path,
    tool: &str,
    fut: F,
) -> F::Output {
    let _window = ToolWindow::open(project_root, tool);
    fut.await
}

/// 此刻是否有开着的窗口覆盖这条托管相对路径(守卫采样时用)。
pub fn write_in_progress(project_root: &Path, relative_path: &str) -> bool {
    let root = kanzei_base::path_form::canonical_or_simplified(project_root);
    let project_key = crate::project_root::dir_key(&root);
    active()
        .lock()
        .unwrap()
        .iter()
        .any(|window| window.project_key == project_key && covers(window.spec, relative_path))
}

/// 当前开着的窗口对应的工具名(报告与测试用)。
pub fn active_tools() -> Vec<&'static str> {
    active()
        .lock()
        .unwrap()
        .iter()
        .map(|w| w.spec.tool)
        .collect()
}

pub fn covers(spec: &ManagedWriterSpec, relative_path: &str) -> bool {
    let path = relative_path.replace('\\', "/");
    spec.prefixes
        .iter()
        .any(|prefix| path.starts_with(prefix) || path == prefix.trim_end_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// 窗口是**进程级**共享状态(守卫要跨任务观察它,见模块头注释),两个测试
    /// 并发跑会互相看见对方开的窗口。用一把测试锁串起来——这是全局状态的测试
    /// 纪律,不是被测代码的缺陷。
    fn serial() -> &'static tokio::sync::Mutex<()> {
        static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
    }

    #[test]
    fn 白名单只认专用写工具_bash与只读memory工具不在表内() {
        assert!(writer_spec("defect").is_some());
        assert!(writer_spec("memory_add").is_some());
        assert!(writer_spec("test_record").is_some());
        // bash 有自己的执行前后围栏;放进白名单等于给后台越界开庇护所。
        assert!(writer_spec("bash").is_none());
        assert!(writer_spec("write").is_none());
        assert!(writer_spec("edit").is_none());
        // 只读 memory 工具不该开窗口。
        assert!(writer_spec("memory_search").is_none());
        assert!(writer_spec("memory_stats").is_none());
    }

    #[test]
    fn 前缀覆盖按托管根判定_跨根不越界() {
        let defect = writer_spec("defect").unwrap();
        assert!(covers(defect, ".kanzei/project/defects.md"));
        assert!(covers(defect, ".kanzei/project/defects-archive.md"));
        assert!(covers(defect, ".kanzei/project/architecture/README.md"));
        // 反斜杠路径同样判定(快照键规范化前也不该漏)。
        assert!(covers(defect, ".kanzei\\project\\defects.md"));
        // 跨托管根不覆盖:tracker 工具改不到 memory 树。
        assert!(!covers(defect, ".kanzei/memory/M-001.md"));

        let memory = writer_spec("memory_add").unwrap();
        assert!(covers(memory, ".kanzei/memory/M-001-x.md"));
        assert!(covers(memory, ".kanzei/project/memory.md"));
        assert!(!covers(memory, ".kanzei/project/defects.md"));
    }

    #[tokio::test]
    async fn 窗口开合可见_并在两端触发观察() {
        let _serial = serial().lock().await;
        let opened = Arc::new(AtomicUsize::new(0));
        let closed = Arc::new(AtomicUsize::new(0));
        {
            let opened = opened.clone();
            let closed = closed.clone();
            set_observer(move |phase, _window| match phase {
                WindowPhase::Opened => {
                    opened.fetch_add(1, Ordering::SeqCst);
                }
                WindowPhase::Closed => {
                    closed.fetch_add(1, Ordering::SeqCst);
                }
            });
        }
        assert!(active_tools().is_empty(), "起始不该有开着的窗口");
        tool_scope(Path::new("project-a"), "defect", async {
            assert_eq!(active_tools(), vec!["defect"]);
            assert!(write_in_progress(
                Path::new("project-a"),
                ".kanzei/project/defects.md"
            ));
            // 窗口只覆盖自己的前缀,别的托管根照旧算越界。
            assert!(!write_in_progress(
                Path::new("project-a"),
                ".kanzei/memory/M-001.md"
            ));
        })
        .await;
        assert!(active_tools().is_empty(), "窗口必须随作用域关闭");
        assert!(!write_in_progress(
            Path::new("project-a"),
            ".kanzei/project/defects.md"
        ));
        assert_eq!(
            opened.load(Ordering::SeqCst),
            1,
            "窗口打开必须触发一次 Opened——精确吸收需要「打开前」快照"
        );
        assert_eq!(
            closed.load(Ordering::SeqCst),
            1,
            "窗口关闭必须触发一次 Closed 吸收——守卫是周期采样的,整个窗口可能落在两次采样之间"
        );

        // 非专用写工具零开销通过:不开窗口、不触发观察。
        tool_scope(Path::new("project-a"), "bash", async {
            assert!(active_tools().is_empty());
        })
        .await;
        assert_eq!(opened.load(Ordering::SeqCst), 1);
        assert_eq!(closed.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn future被丢弃时窗口也关闭() {
        let _serial = serial().lock().await;
        // 取消/超时路径把 future 整个 drop:RAII 必须把窗口一起收掉,
        // 否则一次取消就能让托管树永久处于"合法写入中"状态。
        {
            let fut = tool_scope(Path::new("project-a"), "req", std::future::pending::<()>());
            tokio::pin!(fut);
            let _ = tokio::time::timeout(std::time::Duration::from_millis(20), &mut fut).await;
            // 先确认窗口真的开过,否则下面的断言会因为"从来没开"而假绿。
            assert!(active_tools().contains(&"req"), "超时取消前窗口应是开着的");
        }
        assert!(
            active_tools().is_empty(),
            "被丢弃的 future 不该留下开着的窗口"
        );
    }

    #[tokio::test]
    async fn windows_are_scoped_to_the_project_and_closed_by_call_identity() {
        let _serial = serial().lock().await;
        let root = Path::new("project-a");
        let other = Path::new("project-b");
        let first = ToolWindow::open(root, "defect");
        let second = ToolWindow::open(root, "defect");
        let first_id = first.window.as_ref().unwrap().id;
        let second_id = second.window.as_ref().unwrap().id;
        assert_ne!(first_id, second_id);
        assert!(write_in_progress(root, ".kanzei/project/defects.md"));
        assert!(!write_in_progress(other, ".kanzei/project/defects.md"));
        drop(first);
        let active_ids: Vec<_> = active().lock().unwrap().iter().map(|w| w.id).collect();
        assert_eq!(active_ids, vec![second_id]);
        drop(second);
        assert!(!write_in_progress(root, ".kanzei/project/defects.md"));
    }

    #[tokio::test]
    async fn existing_project_aliases_share_the_same_window() {
        let _serial = serial().lock().await;
        let root = std::env::temp_dir().join(format!(
            "kz-window-alias-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let alias = root.join(".");
        let window = ToolWindow::open(&alias, "defect");
        assert!(window.window.as_ref().unwrap().applies_to(&root));
        assert!(write_in_progress(&root, ".kanzei/project/defects.md"));
        drop(window);
        std::fs::remove_dir_all(root).unwrap();
    }
}
