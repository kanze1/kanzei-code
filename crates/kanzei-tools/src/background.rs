//! 后台进程注册表(R-097)。
//!
//! bash 工具默认是"跑完才返回",长驻进程(dev server、watch、长测试)因此无法使用:
//! 要么撞超时被杀,要么占满整轮。后台模式把进程交给本注册表托管,立刻返回句柄,
//! 之后用 `process` 工具查输出/探活/停止。
//!
//! 边界:进程按项目根登记,`kill_project` 在运行停止时回收,避免留下孤儿进程。
//!
//! D-174:托管项目里的后台任务额外登记 **owner 身份**(run_id/process_id/写仲裁键)
//! 与 **启动瞬间的托管基线**。没有身份就无法把异步写入归因到某次运行,没有基线就
//! 无法判断托管树的变化是不是这个进程造成的——两者是后台围栏的地基。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Once, OnceLock};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use crate::managed::ManagedSnapshot;

/// 单个后台进程保留的输出上限。保留尾部——长驻进程的最新日志才有诊断价值。
const MAX_BACKGROUND_OUTPUT: usize = 256 * 1024;
/// persistent 日志在内存中的尾部上限；磁盘日志仍按块追加，避免全量驻留。
const MAX_BACKGROUND_FULL_OUTPUT: usize = 4 * 1024 * 1024;

/// 守卫的对账间隔。托管树实测 47 文件 / 1.4 MB,这个频率的成本可以忽略;
/// 它决定的是"越界写入最多能存在多久",不是回滚是否发生。
const GUARD_TICK: std::time::Duration = std::time::Duration::from_millis(300);

/// 后台任务的归属身份(D-174 验收①"可归因")。来自 ToolCtx 的 R-171 双键;
/// CLI 等未绑定身份的路径登记 `unowned`,如实表示"不知道属于谁"。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundOwner {
    pub run_id: String,
    pub process_id: String,
    pub write_key: String,
}

/// 一次越界写入的归因记录。围栏动作(隔离/回滚)之外单独留档,
/// 让 `process output` 与后续工具调用都能把"谁在什么时候写了什么"讲清楚。
#[derive(Debug, Clone)]
pub struct BreachRecord {
    pub at_ms: u128,
    /// 被改动的托管路径(相对项目根)。
    pub touched: Vec<String>,
    /// 改后内容的留证目录。
    pub quarantine: String,
    /// 实际回滚成功的文件数。
    pub restored: usize,
}

pub struct BackgroundProcess {
    pub id: String,
    pub command: String,
    pub project_root: String,
    pub workdir: String,
    /// 归属身份:租约体系里这个后台任务算谁的。
    pub owner: BackgroundOwner,
    /// R-180:长驻档位。false(默认)= 跟随 owner run:owner run 收尾时一并收尾
    /// (D-174 安全降级);true = 生命周期显式脱离 owner run:finish_foreign_owners
    /// 跳过它,跨 run 存活(注册表/日志落盘见 B2/B3)。
    pub persistent: bool,
    /// R-180 B2:落盘日志路径(仅 persistent 服务有)。日志按块异步追加,重启后按路径
    /// 回看完整磁盘记录(验收③)——不碰托管树/不进 git。
    pub log_path: Option<PathBuf>,
    /// persistent 服务的日志尾部(内存有界,磁盘文件保留完整追加记录)。非 persistent
    /// 仍走 output(内存 256K 丢头留尾)。收集线程同时维护两个内存尾部与磁盘追加块。
    full_output: Arc<Mutex<Vec<u8>>>,
    pub started_at_ms: u128,
    pid: Option<u32>,
    output: Arc<Mutex<Vec<u8>>>,
    truncated: Arc<AtomicBool>,
    /// None = 仍在运行;Some(code) = 已退出(code 为 None 表示被信号/强杀结束)。
    exit: Arc<Mutex<Option<Option<i32>>>>,
    /// 托管树对账基线。守卫吸收合法写入时会推进它,所以是可变的。
    baseline: Arc<Mutex<ManagedSnapshot>>,
    breaches: Arc<Mutex<Vec<BreachRecord>>>,
}

impl BackgroundProcess {
    pub fn is_running(&self) -> bool {
        self.exit.lock().unwrap().is_none()
    }

    /// 当前对账基线的副本(守卫用)。
    pub(crate) fn baseline(&self) -> ManagedSnapshot {
        self.baseline.lock().unwrap().clone()
    }

    /// 推进对账基线:合法写入被吸收后调用,越界回滚后也要调用(树已回到基线)。
    pub(crate) fn set_baseline(&self, snapshot: ManagedSnapshot) {
        *self.baseline.lock().unwrap() = snapshot;
    }

    pub(crate) fn record_breach(&self, record: BreachRecord) {
        self.breaches.lock().unwrap().push(record);
    }

    pub fn breaches(&self) -> Vec<BreachRecord> {
        self.breaches.lock().unwrap().clone()
    }

    pub fn exit_code(&self) -> Option<Option<i32>> {
        *self.exit.lock().unwrap()
    }

    /// 当前已捕获的输出(stdout 与 stderr 合并,按到达顺序)。
    pub fn output(&self) -> String {
        let buf = self.output.lock().unwrap();
        String::from_utf8_lossy(&buf).into_owned()
    }

    /// persistent 服务的日志尾部(内存有界,磁盘文件保留完整追加记录)。非 persistent 服务
    /// 仍走 output(内存 256K 丢头留尾)。
    pub fn full_log(&self) -> String {
        let buf = self.full_output.lock().unwrap();
        String::from_utf8_lossy(&buf).into_owned()
    }

    pub fn truncated(&self) -> bool {
        self.truncated.load(Ordering::SeqCst)
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }
}

type Registry = Mutex<HashMap<String, Arc<BackgroundProcess>>>;

fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_id() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    format!("bg{}", SEQ.fetch_add(1, Ordering::SeqCst) + 1)
}

fn append_bounded(buf: &Arc<Mutex<Vec<u8>>>, truncated: &Arc<AtomicBool>, chunk: &[u8]) {
    let mut buf = buf.lock().unwrap();
    buf.extend_from_slice(chunk);
    if buf.len() > MAX_BACKGROUND_OUTPUT {
        // 丢头留尾:长驻进程关心的是最近发生了什么。
        let drop_to = buf.len() - MAX_BACKGROUND_OUTPUT;
        buf.drain(..drop_to);
        truncated.store(true, Ordering::SeqCst);
    }
}

async fn append_log_chunk(path: &Path, chunk: &[u8]) {
    if chunk.is_empty() {
        return;
    }
    let Ok(mut file) = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await
    else {
        return;
    };
    let _ = file.write_all(chunk).await;
    let _ = file.flush().await;
}

async fn read_log_tail(path: &Path) -> Vec<u8> {
    let Ok(mut file) = tokio::fs::File::open(path).await else {
        return Vec::new();
    };
    let Ok(metadata) = file.metadata().await else {
        return Vec::new();
    };
    if metadata.len() > MAX_BACKGROUND_FULL_OUTPUT as u64 {
        let _ = file
            .seek(std::io::SeekFrom::End(-(MAX_BACKGROUND_FULL_OUTPUT as i64)))
            .await;
    }
    let mut output = Vec::new();
    let _ = file.read_to_end(&mut output).await;
    output
}

/// 托管一个已 spawn 的子进程,立刻返回句柄。stdout/stderr 由后台任务持续抽取。
///
/// `owner` 是归属身份,`baseline` 必须是 **spawn 之前** 拍下的托管镜像——
/// 晚一刻拍就会把这个进程自己的副作用算进基线,围栏从此永远看不见它。
pub fn register(
    mut child: tokio::process::Child,
    command: String,
    project_root: &Path,
    workdir: &Path,
    owner: BackgroundOwner,
    baseline: ManagedSnapshot,
    persistent: bool,
) -> Arc<BackgroundProcess> {
    let id = next_id();
    let output = Arc::new(Mutex::new(Vec::new()));
    let truncated = Arc::new(AtomicBool::new(false));
    let exit: Arc<Mutex<Option<Option<i32>>>> = Arc::new(Mutex::new(None));
    let pid = child.id();
    // R-180 B2:persistent 服务的落盘路径——系统 temp 下按项目根区分,不碰托管树。
    // 跨 run 可定位:同项目根 → 同目录,重启后按 project_root 找到全部历史日志。
    let log_path = if persistent {
        let dir = std::env::temp_dir()
            .join("kanzei-bg-logs")
            .join(project_hash(project_root));
        std::fs::create_dir_all(&dir).ok();
        Some(dir.join(format!("{id}.log")))
    } else {
        None
    };
    let full_output = Arc::new(Mutex::new(Vec::new()));
    // R-180 B3:persistent 服务登记跨 run 注册表(与日志同目录,atomic_file 原语)。
    // 强杀 kzapp 后 wait 任务没机会跑,条目残留在磁盘——正是"重启后能列出上次
    // 未终结长驻服务"的数据来源;自然退出/显式 stop 时从注册表移除(见下)。
    if persistent {
        let entry = PersistentEntry {
            id: id.clone(),
            command: command.clone(),
            project_root: project_root.display().to_string(),
            workdir: workdir.display().to_string(),
            owner: owner.clone(),
            started_at_ms: now_ms(),
            pid: pid.unwrap_or(0),
            log: format!("{id}.log"),
        };
        let mut entries = load_registry(project_root);
        entries.retain(|e| e.id != id);
        entries.push(entry);
        save_registry(project_root, &entries);
    }

    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();

    for stream in [stdout.take().map(Ok), stderr.take().map(Err)]
        .into_iter()
        .flatten()
    {
        let output = output.clone();
        let truncated = truncated.clone();
        let full_output = full_output.clone();
        let log_path = log_path.clone();
        tokio::spawn(async move {
            let mut chunk = [0u8; 8192];
            let mut pending_log = Vec::new();
            let mut since_flush = std::time::Instant::now();
            let mut stream = stream;
            loop {
                let read = match &mut stream {
                    Ok(out) => out.read(&mut chunk).await,
                    Err(err) => err.read(&mut chunk).await,
                };
                match read {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        append_bounded(&output, &truncated, &chunk[..n]);
                        append_bounded(&full_output, &truncated, &chunk[..n]);
                        if log_path.is_some() {
                            pending_log.extend_from_slice(&chunk[..n]);
                            let due = pending_log.len() >= 64 * 1024
                                || since_flush.elapsed() >= std::time::Duration::from_secs(2);
                            if due {
                                if let Some(path) = &log_path {
                                    let pending = std::mem::take(&mut pending_log);
                                    append_log_chunk(path, &pending).await;
                                }
                                since_flush = std::time::Instant::now();
                            }
                        }
                    }
                }
            }
            if let Some(path) = &log_path {
                append_log_chunk(path, &pending_log).await;
            }
        });
    }

    let process = Arc::new(BackgroundProcess {
        id: id.clone(),
        command,
        project_root: project_root.display().to_string(),
        workdir: workdir.display().to_string(),
        owner,
        persistent,
        log_path,
        full_output,
        started_at_ms: now_ms(),
        pid,
        output,
        truncated,
        exit,
        baseline: Arc::new(Mutex::new(baseline)),
        breaches: Arc::new(Mutex::new(Vec::new())),
    });
    registry().lock().unwrap().insert(id, process.clone());
    // 必须在内存注册表插入后再启动 wait 任务：否则瞬时退出的子进程可能
    // 先完成 wait、删除一个尚不存在的条目，随后又被插入成幽灵。
    {
        let exit = process.exit.clone();
        let reg_root = project_root.to_path_buf();
        let reg_id = process.id.clone();
        let reg_persistent = process.persistent;
        tokio::spawn(async move {
            let status = child.wait().await.ok().and_then(|s| s.code());
            *exit.lock().unwrap() = Some(status);
            registry().lock().unwrap().remove(&reg_id);
            if reg_persistent {
                remove_registry_entry(&reg_root, &reg_id);
            }
        });
    }
    // 托管项目才需要守卫;非托管项目没有托管树可对账,不必空转。
    if crate::managed::managed_scope_exists(project_root) {
        install_window_observer_once();
        spawn_guard(process.clone());
    }
    process
}

/// 安装"观察合法写入窗口"的回调(harness 侧的窗口开合回调过来)。
///
/// harness 不依赖 tools,快照与基线都在这一侧,所以只能反向注入。回调里做文件 IO
/// 是有意的:窗口关闭发生在工具执行完成之后,此刻磁盘状态才是最终的;而且注册表
/// 为空时立刻返回,没有后台任务的绝大多数场景零成本。
///
/// D-258 精确吸收:窗口**打开**时给每个运行中的后台进程拍一张「打开前」快照,
/// **关闭**时拍「关闭后」快照,只吸收两张快照之间**实际变化**的路径(还要落在
/// 本窗口声明的前缀内),而不是把整个前缀的当前状态吸收进基线——整前缀吸收会
/// 把后台进程在窗口内偷写的文件(专用工具没碰)一起固化。
fn install_window_observer_once() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        kanzei_harness::managed_fence::set_observer(|phase, spec| {
            for process in running_processes() {
                let root = PathBuf::from(&process.project_root);
                let key = (process.id.clone(), spec.tool);
                match phase {
                    kanzei_harness::managed_fence::WindowPhase::Opened => {
                        // 先拍照再入栈:capture 是文件 IO,别占着快照 map 的锁。
                        let snapshot = ManagedSnapshot::capture(&root);
                        window_open_snapshots()
                            .lock()
                            .unwrap()
                            .entry(key)
                            .or_default()
                            .push(snapshot);
                    }
                    kanzei_harness::managed_fence::WindowPhase::Closed => {
                        // 弹栈(短临界区),capture 在锁外做(文件 IO)。
                        let opened = {
                            let mut snapshots = window_open_snapshots().lock().unwrap();
                            let popped = snapshots.get_mut(&key).and_then(|stack| stack.pop());
                            if let Some(stack) = snapshots.get(&key) {
                                if stack.is_empty() {
                                    snapshots.remove(&key);
                                }
                            }
                            popped
                        };
                        let current = ManagedSnapshot::capture(&root);
                        let mut baseline = process.baseline();
                        // 有打开快照:只吸收「打开→关闭」之间实际变化的路径 ∩ 本窗口前缀。
                        // 没有(窗口打开期间才 spawn 的后台进程):吸收「基线→当前」的前缀内
                        // 变化——窗口关闭时补账,避免它把专用工具此刻的合法写入误判成越界。
                        let before = opened.as_ref().unwrap_or(&baseline);
                        if let Some(change) = crate::managed::diff(before, &current) {
                            let paths: Vec<&str> = change
                                .touched()
                                .into_iter()
                                .map(|s| s.as_str())
                                .filter(|p| kanzei_harness::managed_fence::covers(spec, p))
                                .collect();
                            if !paths.is_empty() {
                                baseline.absorb_paths(&current, &paths);
                                process.set_baseline(baseline);
                            }
                        }
                    }
                }
            }
        });
    });
}

/// 每个后台进程按 (process_id, tool) 存放的「窗口打开前」快照栈。
///
/// 按 tool 分栈而不是单一栈:不同专用工具(如 defect 与 memory_add)的窗口可以
/// 并发打开、交错关闭,LIFO 单栈会错配各自的打开快照。同一 tool 的嵌套窗口
/// (并行 wave)仍按 LIFO 匹配——同一工具名同时开多个窗口是罕见路径,接受该语义。
type WindowSnapshotMap = Mutex<HashMap<(String, &'static str), Vec<ManagedSnapshot>>>;

fn window_open_snapshots() -> &'static WindowSnapshotMap {
    static SNAPSHOTS: OnceLock<WindowSnapshotMap> = OnceLock::new();
    SNAPSHOTS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 全部仍在运行的后台进程(跨项目)。锁只在克隆期间持有,后续文件 IO 不占锁。
fn running_processes() -> Vec<Arc<BackgroundProcess>> {
    registry()
        .lock()
        .unwrap()
        .values()
        .filter(|p| p.is_running())
        .cloned()
        .collect()
}

/// 后台守卫:周期性把托管树与基线对账,越界即隔离 + 回滚 + 终止进程树。
///
/// 进程退出后还会再对账一次——命令可能在最后一刻写盘,漏掉这一次就等于漏掉整次越界。
fn spawn_guard(process: Arc<BackgroundProcess>) {
    tokio::spawn(async move {
        loop {
            let was_running = process.is_running();
            reconcile(&process, true).await;
            if !was_running {
                break;
            }
            tokio::time::sleep(GUARD_TICK).await;
        }
    });
}

/// 一次对账。返回 Some = 检测到越界(已隔离并回滚)。
///
/// `kill_on_breach` 为真时连带终止进程树:后台进程通常会持续重试写入,
/// 只回滚不终止就是无限回滚循环。
async fn reconcile(process: &Arc<BackgroundProcess>, kill_on_breach: bool) -> Option<BreachRecord> {
    let root = PathBuf::from(&process.project_root);
    if !crate::managed::managed_scope_exists(&root) {
        return None;
    }
    let baseline = process.baseline();
    let current = ManagedSnapshot::capture(&root);
    let change = crate::managed::diff(&baseline, &current)?;
    // 分流:此刻有专用工具窗口覆盖的路径有合法解释,本轮放过——真正的吸收在
    // 窗口关闭时做(守卫是周期采样的,整个窗口可能落在两次采样之间)。
    let (legitimate, breach) = change.partition(kanzei_harness::managed_fence::write_in_progress);
    if breach.is_empty() {
        // D-258:变化全部被窗口分流为合法时**不推进基线**。整树推进会把后台进程
        // 在窗口内偷写的文件固化进基线——窗口一关,守卫就再也看不见它,这是蒙混
        // 真正承重的位置。基线唯一的推进方式是窗口关闭时的精确吸收(只吸收窗口
        // 前后实际变化的路径)。窗口开着时反复对账不推进是无害的:变化原样留在
        // diff 里,窗口关闭后要么被吸收(合法),要么在下一轮被报越界(后台蒙混)。
        return None;
    }
    let (quarantine, restored) = crate::managed::quarantine_and_restore(
        &root,
        &baseline,
        &breach,
        &format!("bg-{}", process.id),
    );
    let record = BreachRecord {
        at_ms: now_ms(),
        touched: breach.touched().into_iter().cloned().collect(),
        quarantine: quarantine.display().to_string(),
        restored,
    };
    process.record_breach(record.clone());
    if kill_on_breach {
        if let Some(pid) = process.pid {
            crate::shell::kill_tree(pid).await;
        }
    }
    // 回滚 breach 后,只把 legitimate(窗口覆盖的合法写入)吸收进基线,不再整树
    // set_baseline(capture)——整树推进同样会把窗口内未回滚的蒙混写入固化掉。
    // legitimate 里若混有后台进程的写入(残余边界:与专用工具同窗口写同前缀文件),
    // 快照层面无法区分,如实记录在 D-258。
    let mut new_baseline = baseline;
    let legitimate_paths: Vec<&str> = legitimate
        .touched()
        .into_iter()
        .map(|s| s.as_str())
        .collect();
    new_baseline.absorb_paths(&current, &legitimate_paths);
    process.set_baseline(new_baseline);
    Some(record)
}

/// 收掉不属于当前 run 的后台任务(先终止,再做终态对账)。返回收尾的任务数。
///
/// **为什么现在要收掉跨 run 的后台任务**:D-174 的安全支点是「后台任务生命周期
/// ⊆ owner run 生命周期」。有了这条,加上项目级单 writer,后台任务存活期间唯一
/// 可能合法写托管路径的主体就是 owner run 的前台工具——守卫才有资格把"没有窗口
/// 解释的变化"一律判给后台进程。跨 run 存活会直接击穿这个前提:新 run 的专用
/// 工具写入会被上一个 run 的守卫当成越界回滚,正是验收③要防的误伤。
///
/// **这是本轮的安全降级,不是终态语义。** 跨轮长驻的受管后台服务由 **R-180** 承接
/// (子代理后台化 R-175/R-176 是它的需求来源):那时后台任务要显式继承 writer owner、
/// 转为受管任务并在租约之间正确交接,而不是像现在这样被下一个 run 收掉。
/// 更强的内核级隔离(受限令牌/ACL)是 **D-258**,与本函数无关但同属 D-174 的残余。
pub async fn finish_foreign_owners(project_root: &Path, current_run_id: Option<&str>) -> usize {
    let current = current_run_id.unwrap_or("unowned");
    let mut finished = 0usize;
    for process in list(project_root) {
        if !process.is_running() || process.owner.run_id == current {
            continue;
        }
        // R-180:长驻服务显式脱离 owner run——收尾时跳过,跨 run 存活。
        if process.persistent {
            continue;
        }
        if let Some(pid) = process.pid {
            crate::shell::kill_tree(pid).await;
        }
        reconcile(&process, false).await;
        finished += 1;
    }
    finished
}

pub(crate) fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default()
}

/// R-180 B2:项目根的稳定短标识(落盘日志目录用)。同一项目根永远映射到同一
/// 目录,跨 run/跨进程可定位历史日志;不同项目根不互相污染。
fn project_hash(root: &Path) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    root.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// R-180 B3:跨 run 注册表条目——persistent 服务的持久化登记。
///
/// 落盘于 `<temp>/kanzei-bg-logs/<项目hash>/registry.json`(与日志同目录,项目级
/// 发现基于该目录)。全部字段可序列化,重启后按此重建内存对象(接管/杀掉/标失败)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistentEntry {
    pub id: String,
    pub command: String,
    pub project_root: String,
    pub workdir: String,
    pub owner: BackgroundOwner,
    pub started_at_ms: u128,
    pub pid: u32,
    /// 日志文件名(registry 同目录下)。
    pub log: String,
}

fn registry_path(project_root: &Path) -> PathBuf {
    std::env::temp_dir()
        .join("kanzei-bg-logs")
        .join(project_hash(project_root))
        .join("registry.json")
}

fn load_registry(project_root: &Path) -> Vec<PersistentEntry> {
    let path = registry_path(project_root);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// 写注册表走 atomic_file 原语(验收⑤:全仓不出现第二套写原语)。
fn save_registry(project_root: &Path, entries: &[PersistentEntry]) {
    let path = registry_path(project_root);
    if let Ok(text) = serde_json::to_string_pretty(entries) {
        let _ = crate::atomic_file::write_atomic(&path, &text);
    }
}

/// 从注册表移除指定条目。进程自然退出/显式停止/被杀后调用,不留幽灵条目。
fn remove_registry_entry(project_root: &Path, id: &str) {
    let mut entries = load_registry(project_root);
    let before = entries.len();
    entries.retain(|e| e.id != id);
    if entries.len() != before {
        save_registry(project_root, &entries);
    }
}

/// R-180 验收②:列出跨 run 注册表中上次登记的 persistent 服务,并给出 pid 活性。
///
/// 返回 `(条目, pid 是否存活)`。幽灵条目(pid 已死——强杀 kzapp 后进程没能活下来)
/// 由调用方用 [`mark_registry_failed`] 标失败并清理,本函数只读不写。
pub fn discover_persistent(project_root: &Path) -> Vec<(PersistentEntry, bool)> {
    load_registry(project_root)
        .into_iter()
        .map(|entry| {
            let alive = crate::shell::process_alive(entry.pid);
            (entry, alive)
        })
        .collect()
}

/// 把注册表条目标记为失败并移除(pid 已死的幽灵条目)。返回是否命中。
pub fn mark_registry_failed(project_root: &Path, id: &str) -> bool {
    let mut entries = load_registry(project_root);
    let before = entries.len();
    entries.retain(|e| e.id != id);
    if entries.len() != before {
        save_registry(project_root, &entries);
        true
    } else {
        false
    }
}

/// R-180 验收②"接管":把注册表里 pid 仍存活的长驻服务接回当前进程的内存注册表,
/// 之后可用 process output/stop 操作。返回 None = 条目不存在或 pid 已死。
///
/// 接管后重新拍基线并挂守卫:长驻服务脱离 owner run 不等于脱离文件隔离(D-174
/// 归因/回滚约束原样生效,验收④)。
pub async fn adopt_persistent(project_root: &Path, id: &str) -> Option<Arc<BackgroundProcess>> {
    let entries = load_registry(project_root);
    let entry = entries.iter().find(|e| e.id == id)?.clone();
    if !crate::shell::process_alive(entry.pid) {
        return None;
    }
    let log_path = std::env::temp_dir()
        .join("kanzei-bg-logs")
        .join(project_hash(project_root))
        .join(&entry.log);
    let full_output = Arc::new(Mutex::new(read_log_tail(&log_path).await));
    let output = Arc::new(Mutex::new(Vec::new()));
    let truncated = Arc::new(AtomicBool::new(false));
    let exit: Arc<Mutex<Option<Option<i32>>>> = Arc::new(Mutex::new(None));
    // 没有子进程句柄可 wait,用 pid 活性轮询推进 exit:pid 消失即视为终止。
    let exit_watch = exit.clone();
    let watch_pid = entry.pid;
    let watch_root = project_root.to_path_buf();
    let watch_id = entry.id.clone();
    tokio::spawn(async move {
        loop {
            if !crate::shell::process_alive(watch_pid) {
                *exit_watch.lock().unwrap() = Some(None);
                registry().lock().unwrap().remove(&watch_id);
                remove_registry_entry(&watch_root, &watch_id);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
    let baseline = Arc::new(Mutex::new(ManagedSnapshot::capture(project_root)));
    let process = Arc::new(BackgroundProcess {
        id: entry.id.clone(),
        command: entry.command.clone(),
        project_root: entry.project_root.clone(),
        workdir: entry.workdir.clone(),
        owner: entry.owner.clone(),
        persistent: true,
        log_path: Some(log_path),
        full_output,
        started_at_ms: entry.started_at_ms,
        pid: Some(entry.pid),
        output,
        truncated,
        exit,
        baseline,
        breaches: Arc::new(Mutex::new(Vec::new())),
    });
    registry()
        .lock()
        .unwrap()
        .insert(process.id.clone(), process.clone());
    if crate::managed::managed_scope_exists(project_root) {
        install_window_observer_once();
        spawn_guard(process.clone());
    }
    Some(process)
}

/// R-180 验收②"杀掉":终止注册表里长驻服务的进程树并移除条目。
///
/// 若该服务已接回内存注册表(adopt 过),先做终态对账再清出磁盘注册表;
/// 内存对象保留在注册表供 output 回看最后日志(与 stop 语义一致)。
pub async fn kill_registered(project_root: &Path, id: &str) -> bool {
    let entries = load_registry(project_root);
    let Some(entry) = entries.iter().find(|e| e.id == id).cloned() else {
        return false;
    };
    if crate::shell::process_alive(entry.pid) {
        crate::shell::kill_tree(entry.pid).await;
    }
    if let Some(p) = get(id) {
        reconcile(&p, false).await;
    }
    remove_registry_entry(project_root, id);
    true
}

pub fn get(id: &str) -> Option<Arc<BackgroundProcess>> {
    registry().lock().unwrap().get(id).cloned()
}

/// 本项目登记的全部后台进程(含已退出的,便于事后查看输出)。
pub fn list(project_root: &Path) -> Vec<Arc<BackgroundProcess>> {
    let root = project_root.display().to_string();
    let mut items: Vec<Arc<BackgroundProcess>> = registry()
        .lock()
        .unwrap()
        .values()
        .filter(|p| p.project_root == root)
        .cloned()
        .collect();
    items.sort_by(|a, b| a.id.cmp(&b.id));
    items
}

/// 停止单个后台进程(连同子进程树)。已退出时返回 false。
///
/// 终止后立刻做一次终态对账:进程可能在被杀的前一刻写了托管文件,
/// 而守卫的下一跳最多要等一个 GUARD_TICK——停止路径不能把这个缺口留给运气。
pub async fn stop(id: &str) -> bool {
    let Some(process) = get(id) else {
        return false;
    };
    if !process.is_running() {
        return false;
    }
    if let Some(pid) = process.pid {
        crate::shell::kill_tree(pid).await;
    }
    reconcile(&process, false).await;
    // R-180 B3:显式停止 persistent 服务 = 终态,同步清出跨 run 注册表,不留幽灵。
    if process.persistent {
        remove_registry_entry(Path::new(&process.project_root), &process.id);
    }
    true
}

/// 回收本项目的全部后台进程并做终态对账。
///
/// 调用点:①运行停止(app/run.rs),避免留下孤儿 dev server;②writer 租约释放前的
/// 收尾——设计基线 parallel_read_serial_write_orchestration.md 要求"writer 结束前
/// 收尾或显式转为受管任务,不能提前释放租约",本函数就是那个收尾入口(R-173 接线)。
///
/// 返回实际终止的进程数。
pub async fn kill_project(project_root: &Path) -> usize {
    let mut killed = 0usize;
    for process in list(project_root) {
        // R-180 B3:长驻服务不被"运行停止回收"误杀——生命周期已显式脱离 owner
        // run(验收①),只能由 process stop/kill 或注册表处置显式终止。
        if process.persistent {
            continue;
        }
        if process.is_running() {
            if let Some(pid) = process.pid {
                crate::shell::kill_tree(pid).await;
                killed += 1;
            }
            reconcile(&process, false).await;
        }
    }
    killed
}

/// 只回收指定线路拥有的后台进程。多线路共享同一个主项目根，停止一条线路时不能
/// 使用 [`kill_project`]，否则会把其它线路的 dev server、watch 和长测试一起杀掉。
pub async fn kill_process(project_root: &Path, process_id: &str) -> usize {
    let mut killed = 0usize;
    for process in list(project_root)
        .into_iter()
        .filter(|process| process.owner.process_id == process_id)
    {
        // R-180 B3:线路停止同样不回收长驻服务——persistent 的生命周期已显式
        // 脱离 owner run(验收①),与 kill_project 同一口径。
        if process.persistent {
            continue;
        }
        if process.is_running() {
            if let Some(pid) = process.pid {
                crate::shell::kill_tree(pid).await;
                killed += 1;
            }
            reconcile(&process, false).await;
        }
    }
    killed
}

#[cfg(test)]
mod tests {
    use super::*;

    use kanzei_harness::{Tool, ToolCtx};

    fn test_owner() -> BackgroundOwner {
        BackgroundOwner {
            run_id: "run-test".into(),
            process_id: "proc-test".into(),
            write_key: "key-test".into(),
        }
    }

    const ORIGINAL_DEFECTS: &str = "# Defects\n\n## D-001 原始内容 [open]\n";

    /// 合法写入窗口(`managed_fence`)是**进程级**状态,守卫要跨任务观察它。
    /// 因此凡是断言"越界被抓 / 合法写入没被误伤"的测试都必须串行:否则 A 测试
    /// 开着的 defect 窗口会给 B 测试的越界写入当挡箭牌,断言随机假绿。
    fn serial() -> &'static tokio::sync::Mutex<()> {
        static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
    }

    /// 跨进程围栏锁(D-268):`serial()` 只挡**同进程**;任务级并行时两条线共享
    /// 同一个 CARGO_TARGET_DIR 并行跑 `cargo test -p kanzei-tools`,两个 OS 进程的
    /// `managed_fence` 窗口(进程内 static)互不可见,可以交错——越界写落在另一进程
    /// 的合法窗口里→假绿;自己的合法写被另一进程的窗口边界切断→假红。
    ///
    /// 与 D-261 给 `test_record` 的做法同源:用 `atomic_file::FileLock` 做跨进程互斥。
    /// FileLock 是 `!Send`、不能跨 await,所以由**持锁线程**持有,本 guard 只持
    /// channel 发送端;`Drop` 时通知持锁线程释放,线程退出时 OS 自动关句柄。
    struct FenceGuard {
        release: std::sync::mpsc::Sender<()>,
    }

    impl Drop for FenceGuard {
        fn drop(&mut self) {
            let _ = self.release.send(());
        }
    }

    /// 取跨进程围栏锁。锁文件路径固定(不随 pid/时间变),两条线才锁到同一把。
    /// 同步阻塞直到拿到——测试之间本来就该互等,拿不到锁的进程一直等。
    fn fence_guard() -> FenceGuard {
        let lock_path = std::env::temp_dir().join("kanzei-bgfence-tests.lock");
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<()>();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        std::thread::spawn(move || {
            loop {
                match crate::atomic_file::try_lock_exclusive(
                    &lock_path,
                    std::time::Duration::from_millis(500),
                ) {
                    Ok(Some(_lock)) => break,
                    Ok(None) | Err(_) => std::thread::sleep(std::time::Duration::from_millis(50)),
                }
            }
            let _ = ready_tx.send(());
            let _ = release_rx.recv(); // 等 guard Drop 后线程退出,FileLock 随之释放
        });
        ready_rx.recv().expect("持锁线程应就绪");
        FenceGuard {
            release: release_tx,
        }
    }

    fn temp_managed_project(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-bgfence-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        std::fs::write(dir.join(".kanzei/project/defects.md"), ORIGINAL_DEFECTS).unwrap();
        dir
    }

    fn ctx_for(root: &Path, run_id: &str) -> ToolCtx {
        ToolCtx {
            cwd: root.to_path_buf(),
            project_root: root.to_path_buf(),
            run_id: Some(run_id.into()),
            process_id: Some(format!("proc-{run_id}")),
            ..Default::default()
        }
    }

    /// 起一个后台任务,返回它的 process id。走真实的 bash 工具路径,
    /// 因为围栏的装配(owner、基线、守卫)全在那条路径上。
    async fn start_background(root: &Path, command: &str, run_id: &str) -> String {
        let out = crate::bash::BashTool
            .execute(
                serde_json::json!({ "command": command, "background": true }),
                &ctx_for(root, run_id),
            )
            .await;
        assert!(!out.is_error, "后台启动不该失败: {}", out.content);
        out.content
            .lines()
            .find_map(|line| line.strip_prefix("process_id: "))
            .expect("输出里应有 process_id")
            .trim()
            .to_string()
    }

    /// 让后台命令先等一会儿再写文件:等待期给守卫拍下基线的机会,
    /// 也让"写入发生在工具早已返回之后"这个 D-174 的核心场景成真。
    fn delayed_write_command(target: &Path, content: &str) -> String {
        let target = target.display().to_string().replace('\\', "/");
        match crate::shell::detected_shell().name {
            "pwsh" | "powershell" => format!(
                "Start-Sleep -Milliseconds 400; [System.IO.File]::WriteAllText('{target}', '{content}')"
            ),
            "cmd" => format!("ping -n 2 127.0.0.1 >nul & echo {content}> \"{target}\""),
            _ => format!("sleep 0.4; printf {content} > '{target}'"),
        }
    }

    async fn wait_until(mut ready: impl FnMut() -> bool, max_ms: u64) -> bool {
        for _ in 0..(max_ms / 50) {
            if ready() {
                return true;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        ready()
    }

    /// 命令片段的连接符(cmd 不认 `;`)。
    fn joiner() -> &'static str {
        if crate::shell::detected_shell().name == "cmd" {
            " & "
        } else {
            "; "
        }
    }

    /// 命令前奏:起一个孙进程,并把它的 pid 写进临时文件回传。返回 `(片段, 临时文件)`。
    ///
    /// D-262 之后凡是断言"进程树真的没了"的测试都要用这个形态,原因有三条:
    /// ①必须有孙进程——`taskkill /t` 存在的全部理由就是后代,只查根 pid 查不出 `/t` 有没有生效;
    /// ②前奏必须排在命令**最前面**,孙进程要早于被测事件(停止/越界)就位,否则测试拿不到
    ///   断言对象,只能退化成"只查根"而看不见真正的孤儿残留;
    /// ③孙 pid 走临时文件回传而不是 stdout:后台任务的 stdout 由注册表持续抽着,别去抢。
    ///
    /// 回传用 `WriteAllText` 而不是 `Set-Content`:后者被 bash 工具的整文件覆写规则拦掉。
    fn tree_prelude(tag: &str) -> (String, PathBuf) {
        let marker = std::env::temp_dir().join(format!(
            "kz-d262-bg-{tag}-{}-{}.txt",
            std::process::id(),
            now_ms()
        ));
        let marker_str = marker.display().to_string().replace('\\', "/");
        let prelude = match crate::shell::detected_shell().name {
            "pwsh" | "powershell" => format!(
                "$c = Start-Process -NoNewWindow -PassThru -FilePath (Get-Process -Id $PID).Path \
                 -ArgumentList '-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 300'; \
                 [System.IO.File]::WriteAllText('{marker_str}', \"$($c.Id)\")"
            ),
            "cmd" => format!(
                "start /b ping -n 300 127.0.0.1 >nul & echo 0 > \"{marker_str}\""
            ),
            _ => format!("sleep 300 & echo $! > '{marker_str}'"),
        };
        (prelude, marker)
    }

    /// 命令尾巴:活得远比测试长。
    ///
    /// 会自然退出的命令会让"已终止"断言因为**错误的原因**通过——这正是 D-174 当时
    /// 拆掉断言的顾虑,所以这里一律 300 秒。
    fn linger() -> &'static str {
        match crate::shell::detected_shell().name {
            "pwsh" | "powershell" => "Start-Sleep -Seconds 300",
            "cmd" => "ping -n 300 127.0.0.1 >nul",
            _ => "sleep 300",
        }
    }

    /// 读回孙进程 pid;拿不到就返回 None(cmd/POSIX 分支不回传真 pid)。
    async fn grandchild_pid(marker: &Path) -> Option<u32> {
        for _ in 0..200 {
            if let Ok(text) = std::fs::read_to_string(marker) {
                if let Ok(pid) = text.trim().parse::<u32>() {
                    let _ = std::fs::remove_file(marker);
                    return if pid == 0 { None } else { Some(pid) };
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        None
    }

    /// pwsh/powershell 下孙 pid 必须真的拿到手。
    ///
    /// 拿不到就说明测试形态坏了(比如前奏没排在最前面),断言会静默退化成"只查根进程"
    /// ——那正好是 D-262 里孤儿后代能一直活下去的那个盲区,不能让它悄悄发生。
    fn require_grandchild(pid: Option<u32>) -> Option<u32> {
        if matches!(crate::shell::detected_shell().name, "pwsh" | "powershell") {
            assert!(
                pid.is_some(),
                "pwsh 下必须回传孙进程 pid,否则本测试会静默退化成只查根进程"
            );
        }
        pid
    }

    /// 断言整棵树都不在了(D-262 验收①的口径:问 pid 的活性,不问函数的返回值)。
    ///
    /// 终止是异步的,所以这里是"等到没"而不是"看一眼";等不到就如实红。
    async fn assert_tree_gone(root: u32, grandchild: Option<u32>, whose: &str) {
        let pids: Vec<u32> = [Some(root), grandchild].into_iter().flatten().collect();
        let gone = wait_until(
            || !pids.iter().copied().any(crate::shell::process_alive),
            15_000,
        )
        .await;
        let alive: Vec<u32> = pids
            .iter()
            .copied()
            .filter(|p| crate::shell::process_alive(*p))
            .collect();
        // 残留会占着测试机 300 秒,先收拾再断言。
        for pid in &alive {
            crate::shell::kill_tree(*pid).await;
        }
        assert!(gone, "{whose}:进程树必须真的消失,残留 pid {alive:?}");
    }

    /// 场景①启动:托管项目里的后台任务必须带 owner、带启动瞬间的基线、并挂上守卫。
    #[tokio::test]
    async fn 场景启动_托管项目后台任务登记owner与基线() {
        let _serial = serial().lock().await;
        let _fence = fence_guard();
        let root = temp_managed_project("start");
        let id = start_background(&root, "echo started", "run-start").await;

        let process = get(&id).expect("应登记在注册表");
        assert_eq!(process.owner.run_id, "run-start", "必须带 owner run_id");
        assert_eq!(process.owner.process_id, "proc-run-start");
        assert!(
            list(&root).iter().any(|p| p.id == id),
            "应出现在本项目的进程列表里"
        );
        // 基线是 spawn 之前拍的:此刻托管树没被动过,基线应与当前一致。
        assert!(
            crate::managed::diff(&process.baseline(), &ManagedSnapshot::capture(&root)).is_none(),
            "启动瞬间的基线应与托管树当前状态一致"
        );
        assert!(process.breaches().is_empty(), "刚启动不该有越界记录");
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn 按线路停止只回收目标owner的后台进程() {
        let _serial = serial().lock().await;
        let _fence = fence_guard();
        let root = temp_managed_project("stop-owner");
        let target = start_background(&root, linger(), "line-a").await;
        let sibling = start_background(&root, linger(), "line-b").await;
        let target_process = get(&target).expect("目标进程应登记");
        let sibling_process = get(&sibling).expect("同线路进程应登记");
        assert_eq!(kill_process(&root, "proc-line-a").await, 1);
        let stopped = wait_until(|| !target_process.is_running(), 5_000).await;
        assert!(stopped, "目标线路后台进程应停止");
        assert!(sibling_process.is_running(), "其它线路后台进程必须保持运行");
        assert!(stop(&sibling).await);
        std::fs::remove_dir_all(&root).ok();
    }

    /// 场景②轮询:正常后台任务被反复观察时,托管树逐字节不变、不产生越界记录。
    /// 这条挡的是"围栏太敏感,把没碰托管树的任务也报成越界"。
    #[tokio::test]
    async fn 场景轮询_读输出不误判托管树也不产生越界记录() {
        let _serial = serial().lock().await;
        let _fence = fence_guard();
        let root = temp_managed_project("poll");
        let id = start_background(
            &root,
            &format!("echo polled{}{}", joiner(), linger()),
            "run-poll",
        )
        .await;
        let process = get(&id).expect("轮询中的进程应登记");
        let ctx = ctx_for(&root, "run-poll");

        // 反复轮询,跨过多个守卫周期。
        for _ in 0..4 {
            let out = crate::process::ProcessTool
                .execute(
                    serde_json::json!({"action": "output", "id": id.clone()}),
                    &ctx,
                )
                .await;
            assert!(!out.is_error, "{}", out.content);
            assert!(
                !out.content.contains("[managed-files]"),
                "没碰托管树的任务不该被报成越界: {}",
                out.content
            );
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        }
        let listed = crate::process::ProcessTool
            .execute(serde_json::json!({"action": "list"}), &ctx)
            .await;
        assert!(
            listed.content.contains("owner=run-poll"),
            "{}",
            listed.content
        );
        assert!(
            !listed.content.contains("managed-breaches"),
            "{}",
            listed.content
        );
        assert_eq!(
            std::fs::read_to_string(root.join(".kanzei/project/defects.md")).unwrap(),
            ORIGINAL_DEFECTS,
            "托管文件必须逐字节不变"
        );
        assert!(process.breaches().is_empty());
        assert!(stop(&id).await, "轮询夹具应显式停止以清理进程");
        std::fs::remove_dir_all(&root).ok();
    }

    /// 场景③停止:stop 走终止路径、做终态对账,而且**进程树真的消失**。
    ///
    /// D-174 交付时这条测试有意不断言"进程真的死了":当时 `shell::kill_tree` 恒定 2.008 秒
    /// 后返回而目标毫发无损(已单独登记为 D-262),断言只会长期挂红,而且当时用的是 5 秒
    /// 短命令——就算断言也会因为"命令自然退出"这个**错误的原因**通过。
    ///
    /// D-262 修复后按正确形态补回,三处都换了口径:①命令改成 300 秒长驻,自然退出不可能
    /// 冒充击杀成功;②带一个孙进程,`taskkill /t` 没生效就会留下残留;③断言问的是 pid 的
    /// 活性(`process_alive`),不是 `stop` 的返回值——旧实现能让"断言函数返回"的测试满分通过。
    #[tokio::test]
    async fn 场景停止_走终止路径并做终态对账_进程树真的消失() {
        let _serial = serial().lock().await;
        let _fence = fence_guard();
        let root = temp_managed_project("stop");
        let (prelude, marker) = tree_prelude("stop");
        let command = format!("{prelude}{}{}", joiner(), linger());
        let id = start_background(&root, &command, "run-stop").await;
        let process = get(&id).unwrap();
        assert!(process.is_running(), "命令此刻应在运行");
        let root_pid = process.pid.expect("后台任务应有 pid");
        let grandchild = require_grandchild(grandchild_pid(&marker).await);
        assert!(
            crate::shell::process_alive(root_pid),
            "测试前提:根进程此刻应在运行"
        );
        if let Some(gc) = grandchild {
            assert!(
                crate::shell::process_alive(gc),
                "测试前提:孙进程此刻应在运行"
            );
        }

        let ctx = ctx_for(&root, "run-stop");
        let out = crate::process::ProcessTool
            .execute(
                serde_json::json!({"action": "stop", "id": id.clone()}),
                &ctx,
            )
            .await;
        // stop 找到了活着的进程并走了终止 + 终态对账这条路径。
        assert!(out.content.contains("stopped"), "{}", out.content);
        // D-262 验收③:`stop` 报了 stopped,进程就必须真的没了——影响①正是"名义 stopped、实则还在跑"。
        assert_tree_gone(root_pid, grandchild, "process stop").await;
        // 终态对账不该凭空造出越界:这个任务没碰过托管树。
        assert!(
            get(&id).unwrap().breaches().is_empty(),
            "没碰托管树的任务不该在停止对账时被报成越界"
        );
        assert_eq!(
            std::fs::read_to_string(root.join(".kanzei/project/defects.md")).unwrap(),
            ORIGINAL_DEFECTS,
            "停止路径不得改动托管树"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// 场景④越界写入:后台任务在工具早已返回之后写托管文档,
    /// 必须被隔离、回滚、终止进程树,并留下能追到 owner 的归因记录。
    #[tokio::test]
    async fn 场景越界_后台写托管文档被隔离回滚并归因到owner_且进程树被终止() {
        let _serial = serial().lock().await;
        let _fence = fence_guard();
        let root = temp_managed_project("breach");
        let target = root.join(".kanzei/project/defects.md");
        // D-262 之前这里用的是"写完就自然退出"的命令,所以"越界即终止"根本无法断言
        // (进程反正会自己走,断言会因为错误的原因通过)。现在的形态是
        // 「先起孙进程 → 再越界写 → 然后长驻 300 秒」:孙进程必须早于越界事件就位,
        // 否则守卫在孙进程出生前就把根杀了,测试既拿不到孙 pid,也看不见孤儿残留。
        let (prelude, marker) = tree_prelude("breach");
        let command = format!(
            "{prelude}{j}{}{j}{}",
            delayed_write_command(&target, "BREACH"),
            linger(),
            j = joiner()
        );
        let id = start_background(&root, &command, "run-breach").await;
        let breach_root_pid = get(&id).unwrap().pid.expect("后台任务应有 pid");
        // 趁树还活着把孙 pid 拿到手:守卫杀完之后 marker 就再也不会被写了。
        let breach_grandchild = require_grandchild(grandchild_pid(&marker).await);
        // 工具已经返回,写入还没发生——这正是 D-174 描述的时序。
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            ORIGINAL_DEFECTS,
            "启动时刻还不该有写入"
        );

        let process = get(&id).unwrap();
        assert!(
            wait_until(|| !process.breaches().is_empty(), 10_000).await,
            "守卫应在若干个周期内抓到越界写入"
        );

        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            ORIGINAL_DEFECTS,
            "越界写入必须被回滚到基线内容"
        );
        let breach = process.breaches().remove(0);
        assert!(
            breach.touched.iter().any(|p| p.ends_with("defects.md")),
            "归因记录要点名被改的路径: {:?}",
            breach.touched
        );
        assert!(breach.restored >= 1, "至少回滚一个文件");
        assert!(
            Path::new(&breach.quarantine).is_dir(),
            "改后的内容必须留证在 {}",
            breach.quarantine
        );
        // D-262 补回的断言:越界即终止。命令是 300 秒长驻的,自然退出冒充不了击杀;
        // 守卫的 reconcile(kill_on_breach=true) 必须把整棵树杀掉,否则后台进程会持续
        // 重试写入,变成"只回滚不终止"的无限回滚循环(reconcile 的文档注释即此意)。
        assert_tree_gone(breach_root_pid, breach_grandchild, "越界即终止").await;

        // 进程已经终态并从注册表回收；归因仍由持有的进程句柄保留，供本次操作审计。
        assert!(!process.is_running(), "越界终止后进程句柄应进入终态");
        assert_eq!(process.breaches().len(), 1, "应保留一条越界归因记录");
        std::fs::remove_dir_all(&root).ok();
    }

    /// 场景⑤并发合法写入:后台任务运行期间,专用工具在**合法写入窗口内**改托管
    /// 文档不得被回滚(验收③)。
    ///
    /// 对照组是这条测试的重点:同样的写入放在**窗口外**必须被回滚。少了对照组,
    /// 一个"恒真放行"的实现照样能让上半段假绿。
    #[tokio::test]
    async fn 场景并发_窗口内合法写入不误伤_窗口外同样写入被回滚() {
        let _serial = serial().lock().await;
        let _fence = fence_guard();
        let root = temp_managed_project("concurrent");
        let target = root.join(".kanzei/project/defects.md");
        let long_running = match crate::shell::detected_shell().name {
            "pwsh" | "powershell" => "Start-Sleep -Seconds 30",
            "cmd" => "ping -n 30 127.0.0.1 >nul",
            _ => "sleep 30",
        };
        let id = start_background(&root, long_running, "run-conc").await;
        let process = get(&id).unwrap();

        // —— 窗口内:模拟 `defect` 工具的写入区间。
        const LEGIT: &str = "# Defects\n\n## D-001 专用工具改的 [fixing]\n";
        kanzei_harness::managed_fence::tool_scope("defect", async {
            std::fs::write(&target, LEGIT).unwrap();
        })
        .await;
        // 跨过好几个守卫周期,确认它不会被回滚。
        tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            LEGIT,
            "窗口内的合法写入被误伤了——这正是验收③要防的"
        );
        assert!(
            process.breaches().is_empty(),
            "合法写入不该产生越界记录: {:?}",
            process.breaches()
        );
        assert!(process.is_running(), "合法写入不该牵连后台进程");

        // —— 对照组:同样的写入,不开窗口。
        std::fs::write(&target, "# Defects\n\n## D-001 窗口外偷改 [open]\n").unwrap();
        assert!(
            wait_until(|| !process.breaches().is_empty(), 10_000).await,
            "窗口外的写入必须被判越界,否则上半段的通过毫无意义"
        );
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            LEGIT,
            "窗口外写入应回滚到上一次合法写入的内容(基线已被窗口吸收)"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// D-258 验收核心:窗口开着时守卫**不推进基线**。旧实现里,后台进程只要赶上
    /// 任意一个专用工具窗口,把前缀内文件改成恶意内容,守卫对账看到的变化全被
    /// 分流为"合法",然后 `set_baseline(current)` 把整棵托管树(含恶意内容)固化
    /// 进基线——窗口一关,守卫就再也看不见它,这是"毫秒级蒙混"真正承重的位置。
    ///
    /// 断言口径:窗口开着时手动对账,基线必须**落后于**当前树(变化仍可被看见);
    /// 窗口关闭后精确吸收只吸收窗口内实际变化的路径,基线追上当前、不再报越界。
    #[tokio::test]
    async fn 窗口开着时守卫不推进基线_关闭后精确吸收() {
        let _serial = serial().lock().await;
        let _fence = fence_guard();
        let root = temp_managed_project("window-hold");
        let target = root.join(".kanzei/project/defects.md");
        let long_running = match crate::shell::detected_shell().name {
            "pwsh" | "powershell" => "Start-Sleep -Seconds 30",
            "cmd" => "ping -n 30 127.0.0.1 >nul",
            _ => "sleep 30",
        };
        let id = start_background(&root, long_running, "run-hold").await;
        let process = get(&id).unwrap();

        // 打开窗口并停在 pending(窗口保持开着,工具"还没结束")。
        // 必须用 Box::pin 持有 future 本体:tokio::pin! 只持有 &mut 引用,
        // drop 引用不会 drop future,窗口会一直开到最后,吸收断言必然假失败。
        let mut window: std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> = Box::pin(
            kanzei_harness::managed_fence::tool_scope("defect", std::future::pending::<()>()),
        );
        {
            let _ = tokio::time::timeout(std::time::Duration::from_millis(20), &mut window).await;
        }
        assert!(
            kanzei_harness::managed_fence::write_in_progress(".kanzei/project/defects.md"),
            "窗口应已打开"
        );

        // 窗口内改前缀文件(模拟专用工具写入)。
        const LEGIT: &str = "# Defects\n\n## D-001 窗口内合法写入 [fixing]\n";
        std::fs::write(&target, LEGIT).unwrap();

        // 手动对账:窗口开着,变化被分流为合法,守卫**必须**不推进基线。
        reconcile(&process, false).await;
        assert!(
            process.breaches().is_empty(),
            "窗口内的变化不该报越界: {:?}",
            process.breaches()
        );
        let current = ManagedSnapshot::capture(&root);
        assert!(
            crate::managed::diff(&process.baseline(), &current).is_some(),
            "D-258 蒙混洞:守卫在窗口开着时推进了基线,后台偷写被固化,窗口一关就再也看不见"
        );

        // 关闭窗口:精确吸收把窗口内实际变化的路径推进基线。
        drop(window);
        let current = ManagedSnapshot::capture(&root);
        assert!(
            crate::managed::diff(&process.baseline(), &current).is_none(),
            "窗口关闭的精确吸收后,基线应与当前一致"
        );
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            LEGIT,
            "窗口内合法写入不得被误伤"
        );
        assert!(process.breaches().is_empty(), "吸收完成后不该有越界记录");
        std::fs::remove_dir_all(&root).ok();
    }

    /// D-258 验收③(精确拦截侧):窗口覆盖 project 前缀时,后台进程(此处由主进程
    /// 模拟,语义等价)写**窗口前缀外**的托管文件——窗口关闭的精确吸收不会把它
    /// 带进基线,守卫下一轮必须把它识别为越界并回滚,同时窗口内的合法写入保留。
    #[tokio::test]
    async fn 窗口内后台写窗口外托管文件_关闭后仍被回滚且合法写入保留() {
        let _serial = serial().lock().await;
        let _fence = fence_guard();
        let root = temp_managed_project("window-outer");
        let target = root.join(".kanzei/project/defects.md");
        let memory_target = root.join(".kanzei/memory/M-999-x.md");
        let long_running = match crate::shell::detected_shell().name {
            "pwsh" | "powershell" => "Start-Sleep -Seconds 30",
            "cmd" => "ping -n 30 127.0.0.1 >nul",
            _ => "sleep 30",
        };
        let id = start_background(&root, long_running, "run-outer").await;
        let process = get(&id).unwrap();
        std::fs::create_dir_all(root.join(".kanzei/memory")).unwrap();

        const LEGIT: &str = "# Defects\n\n## D-001 窗口内合法写入 [fixing]\n";
        // 窗口内:专用工具写 project 下的文件;同一时刻"后台进程"写 memory 下的文件。
        kanzei_harness::managed_fence::tool_scope("defect", async {
            std::fs::write(&target, LEGIT).unwrap();
            std::fs::write(&memory_target, "MEMORY BREACH").unwrap();
        })
        .await;

        assert!(
            wait_until(|| !process.breaches().is_empty(), 10_000).await,
            "窗口关闭后,窗口前缀外的越界写入必须被守卫识别"
        );
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            LEGIT,
            "窗口内的合法写入不被误伤"
        );
        assert!(
            !memory_target.exists(),
            "窗口前缀外的越界写入必须被回滚(新建文件被删除)"
        );
        let breaches = process.breaches();
        let breach = breaches.last().unwrap();
        assert!(
            breach.touched.iter().any(|p| p.ends_with("M-999-x.md")),
            "归因记录必须点名越界的 memory 文件: {:?}",
            breach.touched
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// D-258 验收②:后台任务写**非托管路径**(项目根下、.kanzei/ 之外)必须畅通——
    /// 围栏的职责是托管文档,不是把后台功能杀掉。写 scratch 文件不产生越界,
    /// 文件原样保留。
    #[tokio::test]
    async fn 后台写非托管路径畅通不误伤() {
        let _serial = serial().lock().await;
        let _fence = fence_guard();
        let root = temp_managed_project("unmanaged");
        let scratch = root.join("scratch.txt");
        let command = delayed_write_command(&scratch, "SCRATCH");
        let id = start_background(&root, &command, "run-scratch").await;
        let process = get(&id).unwrap();
        assert!(
            wait_until(
                || std::fs::read_to_string(&scratch)
                    .map(|s| s.contains("SCRATCH"))
                    .unwrap_or(false),
                10_000,
            )
            .await,
            "后台任务应能正常写非托管路径"
        );
        // 跨过几个守卫周期,确认写 scratch 不被判越界、文件不被回滚。
        tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
        assert!(
            process.breaches().is_empty(),
            "写非托管路径不该产生越界记录: {:?}",
            process.breaches()
        );
        assert_eq!(
            std::fs::read_to_string(&scratch).unwrap(),
            "SCRATCH",
            "非托管写入不得被回滚"
        );
        assert_eq!(
            std::fs::read_to_string(root.join(".kanzei/project/defects.md")).unwrap(),
            ORIGINAL_DEFECTS,
            "托管树不得被动过"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// D-258 验收④:托管树镜像上限(单文件 >4 MiB / 目录 >2000 文件)被突破时,
    /// 后台任务必须**显式拒绝**而不是静默放行——放行等于告诉后台进程"这棵子树
    /// 随便写,反正没人能回滚你"。
    #[tokio::test]
    async fn 托管树超限时后台任务被显式拒绝而不是静默放行() {
        let _serial = serial().lock().await;
        let _fence = fence_guard();
        let root = temp_managed_project("limit");
        // 造一个超过单文件镜像上限的托管文件。
        let big = root.join(".kanzei/project/big.md");
        std::fs::write(
            &big,
            vec![b'x'; crate::managed::MANAGED_SNAPSHOT_FILE_LIMIT as usize + 1],
        )
        .unwrap();
        let out = crate::bash::BashTool
            .execute(
                serde_json::json!({"command": "echo hi", "background": true}),
                &ctx_for(&root, "run-limit"),
            )
            .await;
        assert!(out.is_error, "超限时必须显式拒绝: {}", out.content);
        assert!(
            out.content.contains("refused before execution"),
            "拒绝信息必须点名原因: {}",
            out.content
        );
        assert!(list(&root).is_empty(), "被拒绝的后台任务不得出现在注册表里");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 输出超上限时丢头留尾并标记截断() {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let truncated = Arc::new(AtomicBool::new(false));
        append_bounded(&buf, &truncated, &vec![b'a'; MAX_BACKGROUND_OUTPUT]);
        assert!(!truncated.load(Ordering::SeqCst));
        append_bounded(&buf, &truncated, b"TAIL");
        assert!(truncated.load(Ordering::SeqCst));
        let got = buf.lock().unwrap();
        assert_eq!(got.len(), MAX_BACKGROUND_OUTPUT);
        // 保留的是尾部
        assert!(got.ends_with(b"TAIL"));
    }

    #[test]
    fn id_递增且不重复() {
        let a = next_id();
        let b = next_id();
        assert_ne!(a, b);
        assert!(a.starts_with("bg"));
    }

    /// 真实起一个子进程,验证托管→捕获输出→登记在册→停止的完整闭环。
    #[tokio::test]
    async fn 后台进程可托管_可读输出_可停止() {
        let shell = crate::shell::detected_shell();
        let root = std::env::temp_dir().join("kanzei-bg-test");
        std::fs::create_dir_all(&root).unwrap();

        // 先跑一个立刻结束、有确定输出的命令,验证捕获与退出状态。
        let mut echo = tokio::process::Command::new(&shell.program);
        echo.args(&shell.args)
            .arg("echo kanzei-bg-ok")
            .current_dir(&root)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        let handle = register(
            echo.spawn().expect("spawn echo"),
            "echo kanzei-bg-ok".into(),
            &root,
            &root,
            test_owner(),
            ManagedSnapshot::capture(&root),
            false,
        );
        assert!(get(&handle.id).is_some(), "进程应登记在注册表");
        assert_eq!(handle.owner, test_owner(), "后台任务必须带 owner 身份");
        assert!(handle.breaches().is_empty(), "未越界时不该有归因记录");
        assert!(
            list(&root).iter().any(|p| p.id == handle.id),
            "应出现在本项目的进程列表里"
        );

        // 等它结束并把输出抽干(读取任务是异步的)。
        for _ in 0..100 {
            if !handle.is_running() && handle.output().contains("kanzei-bg-ok") {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert!(!handle.is_running(), "命令应已退出");
        assert!(
            handle.output().contains("kanzei-bg-ok"),
            "应捕获到 stdout,实际: {:?}",
            handle.output()
        );
        // 已退出的进程再 stop 返回 false,不应报错
        assert!(!stop(&handle.id).await, "已结束的进程不该报告为被终止");
    }

    /// D-268 反证①(假绿根源):`managed_fence::active()` 是**进程内** static,
    /// 另一个 OS 进程开着的窗口在本进程 `write_in_progress` 里**不可见**——
    /// 这就是两条线并行跑同一 crate 测试时窗口交错无保护、越界写可蒙混的根因。
    ///
    /// 子进程 helper 由本测试 spawn,它打开 defect 窗口后写信号文件;本进程等
    /// 到信号后断言 `write_in_progress(".kanzei/project/defects.md")` 为 false。
    /// 若窗口跨进程可见(修复方向②那类"进程身份"方案),这里应为 true;当前
    /// 修复方向①(跨进程文件锁)不改窗口语义,所以仍 false——反证锁的必要性。
    #[test]
    fn 跨进程围栏窗口互不可见_需要跨进程锁() {
        let exe = std::env::current_exe().expect("测试二进制路径");
        let signal = std::env::temp_dir().join(format!(
            "kz-bgfence-cross-{}-{}.ready",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let signal_str = signal.display().to_string();
        let mut child = std::process::Command::new(&exe)
            .args([
                "--ignored",
                "--exact",
                "background::tests::子进程_打开defect窗口并写信号",
                "--nocapture",
            ])
            .env("KZ_BGFENCE_SIGNAL", &signal_str)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn 子进程");
        // 等子进程窗口打开(信号文件出现),最多 15 秒。
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while !signal.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(signal.exists(), "子进程应在 15s 内打开窗口并写信号文件");
        // 关键断言:另一个进程的窗口在本进程不可见 → 交错时无保护(假绿根源)。
        assert!(
            !kanzei_harness::managed_fence::write_in_progress(".kanzei/project/defects.md"),
            "跨进程窗口必须互不可见;若可见,修复方向①的锁就不必要了"
        );
        let _ = child.wait();
        let _ = std::fs::remove_file(&signal);
    }

    /// D-268 子进程 helper:打开 defect 窗口,写信号文件,等 2 秒(让父进程有
    /// 时间断言),然后自然关闭。仅由上面的跨进程反证测试 spawn 调用。
    #[test]
    #[ignore]
    fn 子进程_打开defect窗口并写信号() {
        let signal = std::env::var("KZ_BGFENCE_SIGNAL").expect("父进程应传信号文件路径");
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        rt.block_on(async {
            let _window = kanzei_harness::managed_fence::tool_scope("defect", async {
                std::fs::write(&signal, "ready").expect("写信号文件");
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            })
            .await;
        });
    }

    /// R-180 验收①:persistent 档位脱离 owner run——finish_foreign_owners 跳过它;
    /// 默认档位(background=true 不带 persistent)照常被 owner run 收尾。两档对比。
    #[tokio::test]
    async fn persistent_长驻服务跨owner存活_默认档位照常收尾() {
        let _guard = fence_guard();
        let root = temp_managed_project("persist");
        // 两个进程都登记到 run-A;run-B 收尾时,persistent 必须留下、默认必须被杀。
        let persist_id = crate::bash::BashTool
            .execute(
                serde_json::json!({ "command": "sleep 5", "background": true, "persistent": true }),
                &ctx_for(&root, "run-A"),
            )
            .await;
        assert!(
            !persist_id.is_error,
            "persistent 后台启动失败: {}",
            persist_id.content
        );
        let persist_id = persist_id
            .content
            .lines()
            .find_map(|line| line.strip_prefix("process_id: "))
            .expect("应有 process_id")
            .trim()
            .to_string();

        let default_id = crate::bash::BashTool
            .execute(
                serde_json::json!({ "command": "sleep 5", "background": true }),
                &ctx_for(&root, "run-A"),
            )
            .await;
        assert!(
            !default_id.is_error,
            "默认后台启动失败: {}",
            default_id.content
        );
        let default_id = default_id
            .content
            .lines()
            .find_map(|line| line.strip_prefix("process_id: "))
            .expect("应有 process_id")
            .trim()
            .to_string();

        // 两个都在跑。
        assert!(
            get(&persist_id).map(|p| p.is_running()).unwrap_or(false),
            "persistent 应在跑"
        );
        assert!(
            get(&default_id).map(|p| p.is_running()).unwrap_or(false),
            "默认应在跑"
        );

        // run-B 收尾(run-A 是 foreign owner):persistent 存活,默认被收。
        let finished = crate::background::finish_foreign_owners(&root, Some("run-B")).await;
        assert_eq!(finished, 1, "只应收掉默认档位那一个: {finished}");
        assert!(
            get(&persist_id).map(|p| p.is_running()).unwrap_or(false),
            "persistent 长驻服务在 owner run 结束后仍应存活"
        );
        // 默认档位已退出(收尾杀掉了)——exit 标志由异步 wait 任务设置,轮询等待。
        let gone = wait_until(
            || get(&default_id).map(|p| !p.is_running()).unwrap_or(true),
            3000,
        )
        .await;
        assert!(gone, "默认档位应在 owner run 收尾时被收掉");
        std::fs::remove_dir_all(&root).ok();
    }

    /// R-180 验收③:persistent 服务日志落盘——输出超 256 KiB 不丢头,进程退出后
    /// 落盘文件可回看(含最开头的内容,不是内存丢头留尾)。
    #[tokio::test]
    async fn persistent_日志落盘_超256k不丢头_退出后可回看() {
        let _guard = fence_guard();
        let root = temp_managed_project("logdisk");
        // 输出 ~300KiB(pwsh:3000 行 × 100 字符),超过内存 256KiB 上限。
        let out = crate::bash::BashTool
            .execute(
                serde_json::json!({
                    "command": "1..3000 | ForEach-Object { 'x' * 100 }",
                    "background": true,
                    "persistent": true,
                }),
                &ctx_for(&root, "run-A"),
            )
            .await;
        assert!(!out.is_error, "persistent 后台启动失败: {}", out.content);
        let id = out
            .content
            .lines()
            .find_map(|line| line.strip_prefix("process_id: "))
            .expect("应有 process_id")
            .trim()
            .to_string();

        let process = get(&id).expect("persistent 进程应在自然退出前登记");
        let log_path = process
            .log_path
            .clone()
            .expect("persistent 服务应有落盘路径");
        // 等进程自然退出(输出完即退)。
        let exited = wait_until(|| !process.is_running(), 5000).await;
        assert!(exited, "输出 300KiB 的命令应自然退出");

        // 等退出前最终落盘完成(收集线程在进程退出后写最后一块)。
        let mut content = String::new();
        for _ in 0..40 {
            content = std::fs::read_to_string(&log_path).unwrap_or_default();
            if content.len() > 256 * 1024 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(
            content.len() > 256 * 1024,
            "落盘日志应超过 256KiB(增量追加,不丢头): {}",
            content.len()
        );
        assert!(
            content.starts_with(&"x".repeat(100)),
            "落盘日志应含最开头的内容: {:?}",
            &content[..content.len().min(120)]
        );
        // full_log 是有界内存尾部；磁盘才是 persistent 的完整回看来源。
        let full = process.full_log();
        assert!(
            full.len() <= MAX_BACKGROUND_FULL_OUTPUT,
            "full_log 内存必须有界: len={}",
            full.len()
        );
        assert!(process.truncated(), "超过内存上限后应标记截断");
        std::fs::remove_dir_all(&root).ok();
    }

    /// R-180 验收②:persistent 服务登记跨 run 注册表(磁盘),重启后 discover 能列出
    /// 未终结的服务并给出处置——adopt 接管、kill 杀掉,处置后条目清除不留幽灵。
    #[tokio::test]
    async fn persistent_注册表登记_强杀后重开可发现_可接管可杀掉() {
        let _guard = fence_guard();
        let root = temp_managed_project("registry");
        let out = crate::bash::BashTool
            .execute(
                serde_json::json!({ "command": linger(), "background": true, "persistent": true }),
                &ctx_for(&root, "run-A"),
            )
            .await;
        assert!(!out.is_error, "persistent 后台启动失败: {}", out.content);
        let id = out
            .content
            .lines()
            .find_map(|line| line.strip_prefix("process_id: "))
            .expect("应有 process_id")
            .trim()
            .to_string();

        // 注册表文件已落盘,条目登记了 id/pid/owner/日志名。
        let path = registry_path(&root);
        let text = std::fs::read_to_string(&path).expect("注册表文件应存在");
        assert!(text.contains(&id), "注册表应含条目: {text}");
        assert!(text.contains("run-A"), "条目应带 owner: {text}");

        // 模拟"强杀 kzapp 后重开":内存注册表已经没了,但磁盘注册表还在——
        // discover 从磁盘读,pid 活性判定为 running。
        let entries = discover_persistent(&root);
        let (entry, alive) = entries
            .iter()
            .find(|(e, _)| e.id == id)
            .expect("discover 应列出上次未终结的服务");
        assert!(alive, "pid 存活应为 running");
        assert_eq!(entry.command, linger(), "条目应保留原始命令");

        // 接管:重建内存对象,pid 一致、is_running 成立、日志路径可回看。
        let adopted = adopt_persistent(&root, &id)
            .await
            .expect("running 服务应可接管");
        assert!(adopted.is_running(), "接管后应视为运行中");
        assert_eq!(adopted.pid, Some(entry.pid), "接管应保持原 pid");
        assert!(adopted.log_path.is_some(), "接管后日志路径应可回看");
        assert_eq!(adopted.owner.run_id, "run-A", "接管保留 owner 归因");

        // 杀掉:进程树真的消失,注册表条目清除(不留幽灵)。
        assert!(kill_registered(&root, &id).await, "kill 应命中注册表条目");
        let gone = wait_until(|| !crate::shell::process_alive(entry.pid), 15_000).await;
        assert!(gone, "kill 后进程树必须真的消失");
        assert!(
            discover_persistent(&root).is_empty(),
            "处置后注册表应清空,不留幽灵条目"
        );
        std::fs::remove_file(registry_path(&root)).ok();
        std::fs::remove_dir_all(&root).ok();
    }

    /// R-180 验收②:幽灵条目(pid 已死的注册表残留)被 discover 标失败并清出,
    /// 日志文件保留(回看不受影响)。
    #[tokio::test]
    async fn persistent_幽灵条目_discover标失败并清理不留幽灵() {
        let root = temp_managed_project("ghost");
        // 手工造一个 pid 必死的条目(不存在的 pid)。
        let ghost = PersistentEntry {
            id: "bg-ghost".into(),
            command: "ghost command".into(),
            project_root: root.display().to_string(),
            workdir: root.display().to_string(),
            owner: BackgroundOwner {
                run_id: "run-dead".into(),
                process_id: "proc-dead".into(),
                write_key: "key-dead".into(),
            },
            started_at_ms: now_ms(),
            pid: u32::MAX - 1,
            log: "bg-ghost.log".into(),
        };
        save_registry(&root, &[ghost]);

        let out = crate::process::ProcessTool
            .execute(
                serde_json::json!({"action": "discover"}),
                &ctx_for(&root, "run-new"),
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(
            out.content.contains("bg-ghost [failed (pruned)]"),
            "幽灵条目应标失败并清理: {}",
            out.content
        );
        assert!(
            discover_persistent(&root).is_empty(),
            "幽灵条目清出后注册表不应残留"
        );
        std::fs::remove_file(registry_path(&root)).ok();
        std::fs::remove_dir_all(&root).ok();
    }

    /// R-180 验收④:persistent 档位写托管路径仍被 D-174 的归因/回滚拦下——
    /// 脱离 owner run 不等于脱离文件隔离。越界即终止进程树,归因记录点名 owner。
    #[tokio::test]
    async fn persistent_写托管路径仍被隔离回滚并归因到owner() {
        let _guard = fence_guard();
        let root = temp_managed_project("persist-breach");
        let target = root.join(".kanzei/project/defects.md");
        let (prelude, marker) = tree_prelude("persist-breach");
        let command = format!(
            "{prelude}{j}{}{j}{}",
            delayed_write_command(&target, "PERSIST-BREACH"),
            linger(),
            j = joiner()
        );
        let out = crate::bash::BashTool
            .execute(
                serde_json::json!({ "command": command, "background": true, "persistent": true }),
                &ctx_for(&root, "run-pb"),
            )
            .await;
        assert!(!out.is_error, "persistent 后台启动失败: {}", out.content);
        let id = out
            .content
            .lines()
            .find_map(|line| line.strip_prefix("process_id: "))
            .expect("应有 process_id")
            .trim()
            .to_string();
        let root_pid = get(&id).unwrap().pid.expect("后台任务应有 pid");
        let grandchild = require_grandchild(grandchild_pid(&marker).await);

        let process = get(&id).unwrap();
        assert!(
            wait_until(|| !process.breaches().is_empty(), 10_000).await,
            "persistent 越界写必须被守卫抓到"
        );
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            ORIGINAL_DEFECTS,
            "persistent 越界写入必须被回滚"
        );
        assert!(
            process.breaches()[0]
                .touched
                .iter()
                .any(|p| p.ends_with("defects.md")),
            "归因记录要点名被改的路径"
        );
        assert_tree_gone(root_pid, grandchild, "persistent 越界即终止").await;
        std::fs::remove_dir_all(&root).ok();
    }

    /// R-180 验收④+②:adopt 接管的长驻服务仍受守卫约束——接管后写托管路径
    /// 一样被回滚并终止,脱离 owner run 不等于脱离文件隔离。
    #[tokio::test]
    async fn persistent_adopt后写托管路径仍被回滚_守卫继续生效() {
        let _guard = fence_guard();
        let root = temp_managed_project("adopt-breach");
        let target = root.join(".kanzei/project/defects.md");
        // 起一个**无守卫**的长进程(std::process::Command,不进后台注册表),避免
        // 原 register 的守卫抢占对账——真实重启场景里原守卫已随 kzapp 消亡。
        let shell = crate::shell::detected_shell();
        let mut child = std::process::Command::new(&shell.program)
            .args(&shell.args)
            .arg("Start-Sleep -Seconds 300")
            .current_dir(&root)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn 长进程");
        let pid = child.id();
        let entry = PersistentEntry {
            id: "bg-adopt".into(),
            command: "Start-Sleep -Seconds 300".into(),
            project_root: root.display().to_string(),
            workdir: root.display().to_string(),
            owner: BackgroundOwner {
                run_id: "run-ab".into(),
                process_id: "proc-ab".into(),
                write_key: "key-ab".into(),
            },
            started_at_ms: now_ms(),
            pid,
            log: "bg-adopt.log".into(),
        };
        save_registry(&root, &[entry]);

        // adopt 接管:重建内存对象并重新挂守卫(基于接管时重拍的基线)。
        let adopted = adopt_persistent(&root, "bg-adopt")
            .await
            .expect("running 服务应可接管");
        assert_eq!(adopted.pid, Some(pid), "接管保持原 pid");

        // 接管后写托管路径:adopted 的守卫(唯一的守卫)必须回滚并终止进程树。
        std::fs::write(&target, "ADOPT-BREACH").unwrap();
        assert!(
            wait_until(|| !adopted.breaches().is_empty(), 10_000).await,
            "接管后的守卫必须继续对账"
        );
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            ORIGINAL_DEFECTS,
            "接管后越界写入必须回滚"
        );
        assert!(
            adopted.breaches()[0]
                .touched
                .iter()
                .any(|p| p.ends_with("defects.md")),
            "归因记录要点名被改的路径"
        );
        // 越界即终止:adopted 进程被守卫杀掉(pid 活性轮询把 exit 推进为终止)。
        let gone = wait_until(|| !crate::shell::process_alive(pid), 15_000).await;
        assert!(gone, "adopt 后越界仍应终止进程树");
        let not_running = wait_until(|| !adopted.is_running(), 5_000).await;
        assert!(not_running, "adopted 进程被杀后 is_running 应为 false");
        let _ = child.wait();
        std::fs::remove_file(registry_path(&root)).ok();
        std::fs::remove_dir_all(&root).ok();
    }

    /// R-180 验收①:运行停止回收(kill_project / kill_process)跳过 persistent——
    /// 长驻服务的生命周期已显式脱离 owner run,只能由 process stop/kill 或
    /// 注册表处置显式终止;默认档位照常回收。
    #[tokio::test]
    async fn persistent_项目回收跳过_默认档位照常回收() {
        let _guard = fence_guard();
        let root = temp_managed_project("reap");
        let default_id = start_background(&root, linger(), "run-reap").await;
        let persist_real = crate::bash::BashTool
            .execute(
                serde_json::json!({ "command": linger(), "background": true, "persistent": true }),
                &ctx_for(&root, "run-reap"),
            )
            .await;
        assert!(!persist_real.is_error, "{}", persist_real.content);
        let persist_id = persist_real
            .content
            .lines()
            .find_map(|line| line.strip_prefix("process_id: "))
            .expect("应有 process_id")
            .trim()
            .to_string();

        assert!(get(&default_id).unwrap().is_running());
        assert!(get(&persist_id).unwrap().is_running());

        // kill_project 回收:persistent 必须活,默认档位必须死。
        let killed = kill_project(&root).await;
        assert!(killed >= 1, "默认档位应被回收: {killed}");
        let default_gone = wait_until(
            || get(&default_id).map(|p| !p.is_running()).unwrap_or(true),
            15_000,
        )
        .await;
        assert!(default_gone, "默认档位应在项目回收时被收掉");
        assert!(
            get(&persist_id).map(|p| p.is_running()).unwrap_or(false),
            "persistent 长驻服务不得被项目回收误杀"
        );
        let persist_process = get(&persist_id).expect("persistent 进程应登记");
        assert!(persist_process.is_running());
        assert!(stop(&persist_id).await);
        let gone = wait_until(|| !persist_process.is_running(), 15_000).await;
        assert!(gone, "显式 stop 应能终止 persistent 服务");
        std::fs::remove_dir_all(&root).ok();
    }

    /// process 工具 discover/adopt/kill 全链路走工具面(验收②的消费者视角)。
    #[tokio::test]
    async fn process_工具_discover_adopt_kill_全链路() {
        let _guard = fence_guard();
        let root = temp_managed_project("proc-registry");
        let out = crate::bash::BashTool
            .execute(
                serde_json::json!({ "command": linger(), "background": true, "persistent": true }),
                &ctx_for(&root, "run-proc"),
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        let id = out
            .content
            .lines()
            .find_map(|line| line.strip_prefix("process_id: "))
            .expect("应有 process_id")
            .trim()
            .to_string();
        let ctx = ctx_for(&root, "run-proc");

        let discovered = crate::process::ProcessTool
            .execute(serde_json::json!({"action": "discover"}), &ctx)
            .await;
        assert!(!discovered.is_error, "{}", discovered.content);
        assert!(
            discovered.content.contains(&format!("{id} [running]")),
            "discover 应列出 running 状态: {}",
            discovered.content
        );
        assert!(
            discovered.content.contains("\"action\":\"adopt\"")
                && discovered.content.contains("\"action\":\"kill\""),
            "discover 应给出接管/杀掉两种处置: {}",
            discovered.content
        );

        let adopted = crate::process::ProcessTool
            .execute(serde_json::json!({"action": "adopt", "id": id}), &ctx)
            .await;
        assert!(!adopted.is_error, "{}", adopted.content);
        assert!(adopted.content.contains("adopted"), "{}", adopted.content);
        assert!(get(&id).unwrap().is_running(), "接管后应运行中");

        let killed = crate::process::ProcessTool
            .execute(serde_json::json!({"action": "kill", "id": id}), &ctx)
            .await;
        assert!(!killed.is_error, "{}", killed.content);
        assert!(killed.content.contains("killed"), "{}", killed.content);
        assert!(discover_persistent(&root).is_empty(), "kill 后注册表应清空");
        std::fs::remove_file(registry_path(&root)).ok();
        std::fs::remove_dir_all(&root).ok();
    }
}
