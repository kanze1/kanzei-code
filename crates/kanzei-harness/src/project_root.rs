//! 项目根发现与文件系统身份判定(R-205 从 config.rs 拆出)。
//!
//! config.rs 原 3,192 行混装四域;本文件承接「项目根发现 + HOME 守卫」一域:
//! 发现式取根、显式主根校验、HOME/全局根碰撞判定(词法折叠 + 文件系统身份)。
//! D-270 四缺口的修复落在这里。config.rs 经 re-export 保持 `config::xxx` 调用点零变更。

use std::path::{Path, PathBuf};

use crate::permission::normalize_resource;

/// 从 cwd 向上找 `.kanzei/kanzei.toml`。
pub fn discover_project_config(cwd: &Path) -> Option<PathBuf> {
    discover_project_root(cwd).map(|root| root.join(".kanzei").join("kanzei.toml"))
}

/// 项目根 = 向上**最近**的含 `.kanzei/` 或 `.git/` 的目录;都没有则 cwd 本身。
///
/// 两条约束都是踩出来的,别再退回去:
/// ① `.kanzei` 不许无视距离压过 `.git`。原实现撞到任何 `.kanzei` 就立即返回,`.git`
///    只记 fallback 且要等循环走完才用,于是 `~/Documents/某仓库`(有 .git、没 .kanzei)
///    会一路走到 HOME,仓库自己的 `.git` 被丢掉。
/// ② HOME 自己的 `.kanzei` 不算项目标记——它是**全局**配置根(kanzei.toml、memory、
///    app.json),必然存在,于是成了 HOME 下所有无标记目录的磁铁。实测后果:`~/.kanzei`
///    里已经躺着 `project/` 与 `state.db` 这类只该出现在项目里的产物。
///    HOME 的 `.git`(dotfiles 仓库)仍然算标记,那是货真价实的仓库。
pub fn discover_project_root(cwd: &Path) -> Option<PathBuf> {
    discover_project_root_with_home(cwd, dirs::home_dir().as_deref())
}

/// 显式主根优先于发现式取根:参数 > 环境变量 > 发现式(现状)。
///
/// R-182 内容②。`explicit` 由调用方按「`--project-root` 参数 > `KANZEI_PROJECT_ROOT`
/// 环境变量」合成后传进来;为 `None` 时逐字节退回今天的发现式行为
/// (`discover_project_root(cwd)`,兜底 cwd 本身)。
///
/// 实测背景(D-267):两棵 worktree 相隔 10 秒各跑一次 `kz defect add`,**都拿到 D-267**——
/// `.kanzei/project/*.md` 被 git 跟踪,`git worktree add` 把它们 checkout 成分支副本,
/// 发现式取根在 worktree 里第一层就命中那份副本,两条线各自在自己的副本上分配编号。
/// 显式主根就是这条路的出口。
///
/// **两个根是正交的两件事,别再混**(D-187 的教训):
/// - `KANZEI_PROJECT_ROOT` 改的是**项目根**——`.kanzei/project/*.md`、state.db、项目记忆;
/// - `KANZEI_HOME` 改的是**全局根**——`~/.kanzei/kanzei.toml`、全局记忆、app.json。
///   设了其中一个不会影响另一个。
///
/// **不做 canonicalize**:与 run.rs 同源的理由——Windows 上 `canonicalize` 产出
/// `\\?\C:\…` 形态,用户已经写下的绝对路径权限规则会一夜之间集体失配。
pub fn resolve_project_root(explicit: Option<&Path>, cwd: &Path) -> anyhow::Result<PathBuf> {
    let Some(explicit) = explicit else {
        return Ok(discover_project_root(cwd).unwrap_or_else(|| cwd.to_path_buf()));
    };
    if !explicit.exists() {
        anyhow::bail!(
            "显式主根(--project-root / KANZEI_PROJECT_ROOT)指向的路径不存在: {}",
            explicit.display()
        );
    }
    if !explicit.is_dir() {
        anyhow::bail!(
            "显式主根(--project-root / KANZEI_PROJECT_ROOT)不是目录: {}",
            explicit.display()
        );
    }
    // worktree 的 `.git` 是**文件**不是目录,所以这里目录/文件都算标记;
    // `.kanzei` 则必须是目录(托管文档挂在它下面)。
    let has_marker = explicit.join(".kanzei").is_dir() || explicit.join(".git").exists();
    if !has_marker {
        anyhow::bail!(
            "显式主根(--project-root / KANZEI_PROJECT_ROOT)不像项目根:{} 下既没有 .kanzei 目录也没有 .git。\n\
             主根是放 .kanzei/project/*.md 与 state.db 的那个目录;确实想把它当项目,就先 mkdir .kanzei。",
            explicit.display()
        );
    }
    Ok(explicit.to_path_buf())
}

/// 目录比较用的形态:剥 Windows 扩展长度前缀、统一分隔符、折叠 `.` / `..`、去尾分隔符,
/// Windows 上再小写。
///
/// 裸 `==` 比较不够(D-194):`dirs::home_dir()` 给 `C:\Users\kanzei`,而走上来的祖先
/// 可能是 `c:\users\kanzei`(shell 里键入的大小写)或 `\\?\C:\Users\kanzei`(canonicalize
/// 的产物)——任一形态对不上,HOME 判断就静默失效,`~/.kanzei` 立刻变回项目根磁铁。
/// 同一个坑 kanzei-core 的 `session_identity` 已经踩过一次(同一项目裂成两条会话线)。
/// 这里是纯比较、不进哈希,所以可以比那边更狠:分隔符也一并统一。
///
/// **`.` / `..` 必须折叠**,而且这是 R-182 新入口打开的洞、不是历史遗留:根从
/// `current_dir()` 来的时候不可能带 `.`/`..` 段,`--project-root` / `KANZEI_PROJECT_ROOT`
/// 收的却是用户任意书写的路径串。`C:\Users\kanzei\.` 与 `C:\Users\kanzei\Documents\..`
/// 在文件系统看来就是 HOME,`resolve_project_root` 的标记校验对它们照样成立
/// (HOME 下有 `.kanzei`),于是两道拦截**全部静默通过**,project 级 state.db 被写进
/// `~/.kanzei`——实测发生过。
///
/// 折叠**复用 `permission::normalize_resource`**(权限决策的同一份实现,D-050),不另写
/// 第二份:两份词法折叠一旦漂移,就是"权限那边算出来的路径"和"取根这边算出来的路径"
/// 指向两个地方。
///
/// **本函数自身仍然是纯词法的**:不碰文件系统、不解符号链接,也不会把用户写的裸路径
/// 变成 `\\?\` 形态。词法折叠单独用赢不了别名(见 [`is_same_dir`]),但它是**永远可用**
/// 的那一层:路径不存在或读不到时 `canonicalize` 给不出身份,这里的结论就是最后的兜底。
pub(crate) fn dir_key(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let stripped = raw
        .strip_prefix(r"\\?\UNC\")
        .map(|rest| format!(r"\\{rest}"))
        .or_else(|| raw.strip_prefix(r"\\?\").map(str::to_string))
        .unwrap_or_else(|| raw.to_string());
    // 分隔符与大小写只在 Windows 上等价;Linux 下 `C:` 与 `c:` 是两个目录,
    // 归一过头会把不同路径判成同一个。
    #[cfg(windows)]
    let unified = stripped.replace('/', "\\").to_lowercase();
    #[cfg(not(windows))]
    let unified = stripped;
    let key = normalize_resource(&unified);
    key.trim_end_matches(['\\', '/']).to_string()
}

/// 两个路径串指的是不是**同一个目录**。
///
/// D-194 补漏二:**词法规则补不完**,别再往 [`dir_key`] 里加规则了。实测(Windows 11)
/// 下面这些写法在磁盘上就是同一个目录,而纯词法折叠一条都认不出来:
/// - `C:\Users\kanzei.` —— Windows 剥掉末段的尾随点;`kanzei.` 对折叠来说是个普通段名,
///   既不是 `.` 也不是 `..`,于是 `c:/users/kanzei.` ≠ `c:/users/kanzei`。
/// - `\\localhost\C$\Users\kanzei` / `\\127.0.0.1\C$\Users\kanzei` —— 归一成
///   `//localhost/c$/users/kanzei`,与盘符形态永不相等。这**不纯是对抗构造**:网络/漫游
///   profile 下 UNC 本来就是合法写法。
/// - 符号链接、junction、`subst` 虚拟盘、8.3 短名 —— 凡「词法不同、文件系统同一」的别名,
///   再补多少条词法规则也补不完。
///
/// 所以改用**文件系统身份**:两侧各做一次 `std::fs::canonicalize`。实测它把 junction、
/// 8.3 短名、`subst` 虚拟盘、尾随点、大小写、`\\?\` 前缀一律解成同一个 `\\?\C:\…` 形态。
///
/// **这与「显式主根不做 canonicalize」不矛盾——两条说的是不同的事。**
/// 那条顾虑(见 [`resolve_project_root`] 与测试 `显式主根不做canonicalize`)反对的是把
/// canonicalize 的产物**当作项目根存下去 / 传下去**:`\\?\` 形态会让用户已经写在配置里的
/// 绝对路径权限规则集体失配。而这里的 canonicalize **只活在本次相等判断内部**——不返回、
/// 不存储、不进哈希、不传给任何下游;`resolve_project_root` 返回的仍是用户写下的原串,
/// 下游看到的形态一个字节没变(那条测试原样保留,正是用来钉住存储/传播侧没变的)。
///
/// canonicalize 失败(路径不存在、无读权限)时**回落到词法折叠**——拿不到身份不等于放行。
pub(crate) fn is_same_dir(a: &Path, b: &Path) -> bool {
    // ① 词法层永远先跑:它不需要路径存在,也是 canonicalize 失败时的兜底。
    //    只加不减——加上身份层之后,今天能认出来的写法一条都不会变得认不出来。
    if dir_key(a) == dir_key(b) {
        return true;
    }
    let (Ok(ca), Ok(cb)) = (std::fs::canonicalize(a), std::fs::canonicalize(b)) else {
        return false;
    };
    // ② 身份层:同一命名空间内 canonicalize 就是唯一名字,别名到这里全部坍缩。
    if dir_key(&ca) == dir_key(&cb) {
        return true;
    }
    // ③ 跨命名空间:canonicalize **不**把 UNC 归到盘符形态(实测
    //    `\\localhost\C$\Users\kanzei` → `\\?\UNC\localhost\C$\Users\kanzei`),所以 ② 在
    //    「一侧 UNC、一侧盘符」时必然判不等。只在这种情况下再走一层判据。
    if !is_unc_key(&dir_key(&ca)) && !is_unc_key(&dir_key(&cb)) {
        return false;
    }
    same_dir_by_volume_metadata(&ca, &cb)
}

/// [`dir_key`] 产出的 UNC 形态(`\\host\share\…` → `//host/share/…`)。
pub(crate) fn is_unc_key(key: &str) -> bool {
    key.starts_with("//")
}

/// 跨命名空间的同一性:比对目录**自身**的卷级元数据(创建时间 + 修改时间,均 100ns 精度)。
/// 这两个属性存在卷上,透过盘符还是透过 UNC 读到的是同一份——实测一致。
///
/// **诚实说明它不是句柄级身份**:句柄级身份要 `GetFileInformationByHandle` 的
/// `(volume_serial_number, file_index)`,std 里对应 `windows_by_handle`,**至今未稳定**
/// (rustc 1.97 实测 E0658),而本 crate 不引入 winapi 依赖去换这一处判据。
///
/// 判错方向是可陈述的:元数据相等 → 判成同一个目录 → **多拦一个 HOME**(更严,可见地报错);
/// 元数据不等 → 判成不同 → 退回 ② 的结论。D-270 缺口②修正:读失败/非目录/取不到时间
/// 时**保守判同**(返回 true)——拿不到身份就当作可能相同,由上层保守处置,不再放行
/// (原实现 return false 是 fail-open,与它自己的注释「只会偏保守」相悖)。
/// 唯一会偏放行的窗口是「两次读之间 HOME 自身的 mtime 被别的进程改掉」,所以读两轮:
/// 第一轮不等就整组重读一次,那个窗口需要连续两次都撞上才成立。
pub(crate) fn same_dir_by_volume_metadata(a: &Path, b: &Path) -> bool {
    fn fingerprint(path: &Path) -> Option<(Option<std::time::SystemTime>, std::time::SystemTime)> {
        let meta = std::fs::metadata(path).ok()?;
        if !meta.is_dir() {
            return None;
        }
        Some((meta.created().ok(), meta.modified().ok()?))
    }
    for _ in 0..2 {
        let (Some(fa), Some(fb)) = (fingerprint(a), fingerprint(b)) else {
            return true; // 拿不到身份 → 可能相同 → 保守判同
        };
        if fa == fb {
            return true;
        }
    }
    false
}

/// 解析出的项目根是不是 HOME 本身。
///
/// D-189 让 HOME 的 `.kanzei` 不再把子目录吸上去,但在 HOME 里**直接**开跑这条路还通着:
/// 一路向上找不到任何标记时兜底返回 cwd,而 cwd 就是 HOME。此时项目级产物(state.db、
/// project/、memory/)会落进 `~/.kanzei`——那是全局配置根,两边数据就此混在一起
/// (D-186 的残留正是这么来的)。调用方拿这个判据在开跑前拦下来。
///
/// 相等判断委托 [`is_same_dir`](词法折叠 + 文件系统身份),别名形态在那里说明。
/// D-270 缺口③:`KANZEI_HOME` 也参与比较——全局根(KANZEI_HOME 或默认 `~/.kanzei`)
/// 与 root 本身或 root 的 `.kanzei` 同目录时都算碰撞,项目产物会写进全局根。
pub fn is_home_root(root: &Path) -> bool {
    is_home_root_with(
        root,
        dirs::home_dir().as_deref(),
        crate::home::kanzei_home().as_deref(),
    )
}

/// [`is_home_root`] 的可测内核:home 与全局根都作为参数注入,测试不碰进程级
/// `KANZEI_HOME`(与 home.rs 的顺序测试并行跑会互踩环境变量)。
pub(crate) fn is_home_root_with(root: &Path, home: Option<&Path>, kh: Option<&Path>) -> bool {
    if home.is_some_and(|h| is_same_dir(h, root)) {
        return true;
    }
    let Some(kh) = kh else {
        return false;
    };
    // root 本身就是全局根,或 root 的 `.kanzei` 就是全局根(KANZEI_HOME 指到
    // 项目自己的 `.kanzei` 的场景):两种都是项目产物落进全局配置根的碰撞。
    is_same_dir(kh, root) || is_same_dir(kh, &root.join(".kanzei"))
}

pub(crate) fn discover_project_root_with_home(cwd: &Path, home: Option<&Path>) -> Option<PathBuf> {
    discover_project_root_with_roots(cwd, home, Some(&std::env::temp_dir()))
}

/// D-630:系统临时根自己的 `.kanzei` 与 HOME 同理不算项目标记——%TEMP% 是所有
/// 进程共用的倾倒场,任何进程/测试在临时根落一个 `.kanzei`,它就成为全部无标记
/// 临时子目录的磁铁(实测:conversation/process 测试的 state.db 全被并进
/// `%TEMP%\.kanzei\state.db`,跨测试互相污染、跨运行持久)。临时根的 `.git`
/// 沿用 HOME 规则仍算标记。`temp` 参数只为测试可注入,产品路径恒为
/// `std::env::temp_dir()`。
pub(crate) fn discover_project_root_with_roots(
    cwd: &Path,
    home: Option<&Path>,
    temp: Option<&Path>,
) -> Option<PathBuf> {
    let home_key = home.map(dir_key);
    let temp_key = temp.map(dir_key);
    let mut dir = Some(cwd);
    while let Some(d) = dir {
        let kanzei_marker = d.join(".kanzei").is_dir();
        let git_marker = d.join(".git").is_dir();
        let lexically_home = home_key.as_ref().is_some_and(|h| *h == dir_key(d));
        let lexically_temp = temp_key.as_ref().is_some_and(|t| *t == dir_key(d));
        // D-270 缺口①:发现式取根对别名形态的 HOME 也要拦得住——`.kanzei` 标记层
        // 若是 HOME 的别名(词法不同但文件系统身份相同,如尾随点 / UNC),同样跳过
        // 继续向上,不再把别名 HOME 当项目根返回。身份比较(`is_same_dir`)只发生在
        // 词法不等**且有 `.kanzei` 标记**的层:普通层仍是纯词法 `dir_key`,不会给
        // 每次配置加载引入 O(深度) 次 canonicalize 系统调用。临时根按同一模式拦别名。
        let alias_home =
            !lexically_home && kanzei_marker && home.is_some_and(|h| is_same_dir(h, d));
        let alias_temp = !lexically_temp
            && !lexically_home
            && !alias_home
            && kanzei_marker
            && temp.is_some_and(|t| is_same_dir(t, d));
        if (kanzei_marker && !lexically_home && !alias_home && !lexically_temp && !alias_temp)
            || git_marker
        {
            return Some(d.to_path_buf());
        }
        dir = d.parent();
    }
    Some(cwd.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// D-194:真实 HOME 必须被 `is_home_root` 认出来——CLI 靠它在开跑前拦下
    /// "项目级产物落进全局配置根"。
    #[test]
    fn is_home_root_recognizes_real_home_in_any_form() {
        let Some(home) = dirs::home_dir() else {
            return; // 无 HOME 的环境跳过,不是被测行为。
        };
        assert!(is_home_root(&home));
        #[cfg(windows)]
        {
            assert!(is_home_root(&PathBuf::from(format!(
                "{}\\",
                home.display()
            ))));
            assert!(is_home_root(&PathBuf::from(
                home.display().to_string().replace('\\', "/")
            )));
            assert!(is_home_root(&PathBuf::from(
                home.display().to_string().to_uppercase()
            )));
            // canonicalize 的产物形态:剥了 `\\?\` 才认得出来。
            assert!(is_home_root(&PathBuf::from(format!(
                r"\\?\{}",
                home.display()
            ))));
        }
        assert!(!is_home_root(&home.join("projects")));
    }

    /// D-194 补漏:`dir_key` 不折叠 `.` / `..` 时,`C:\Users\kanzei\.` 这类写法让 HOME
    /// 拦截静默失效——而 `resolve_project_root` 的标记校验对它照样成立(HOME 下有
    /// `.kanzei`),两道拦截一起被绕过,project 级 state.db 被写进全局配置根 `~/.kanzei`
    /// (实测发生过)。
    ///
    /// 这条路是 R-182 的显式主根入口打开的:在那之前根恒来自 `current_dir()`,不含
    /// `.`/`..` 段,写不出这种串;新入口收的正是用户任意书写的路径。
    #[test]
    fn is_home_root_folds_dot_and_dotdot_segments() {
        let Some(home) = dirs::home_dir() else {
            return; // 无 HOME 的环境跳过,不是被测行为。
        };
        let sep = std::path::MAIN_SEPARATOR;
        let text = home.display().to_string();
        let mut forms = vec![
            // 尾随 `.`:文件系统里就是 HOME 自己。
            PathBuf::from(format!("{text}{sep}.")),
            // 下一级再 `..` 弹回来。折叠是纯词法的,所以那一级存不存在都一样。
            PathBuf::from(format!("{text}{sep}Documents{sep}..")),
            // `.` 后面还跟着尾分隔符。
            PathBuf::from(format!("{text}{sep}.{sep}")),
            // 多段叠加,一路弹回 HOME。
            PathBuf::from(format!("{text}{sep}a{sep}..{sep}.{sep}b{sep}..")),
        ];
        #[cfg(windows)]
        {
            let slash = text.replace('\\', "/");
            forms.push(PathBuf::from(format!("{slash}/./")));
            forms.push(PathBuf::from(format!("{slash}/Documents/..")));
            // 大小写 + `.` 段 + `\\?\` 前缀三者叠加,任一环节漏了都拦不住。
            forms.push(PathBuf::from(format!(
                r"\\?\{}\.",
                text.to_lowercase().trim_end_matches('\\')
            )));
        }
        for form in forms {
            assert!(
                is_home_root(&form),
                "含 . / .. 的写法必须被认成 HOME: {}",
                form.display()
            );
        }
    }

    /// 折叠不许过头:路径里带 `.` 的**合法目录名**(`v1.0`、`.config`)不是 `.` 段,
    /// 正常项目根不能被误拦;`..` 也必须真的向上一级,而不是被吞掉。
    #[test]
    fn dir_key_keeps_dotted_directory_names() {
        let app = PathBuf::from(r"C:\proj\v1.0\app");
        // `v1.0` / `.config` 是目录名,不是 `.` 段:各自都还在。
        assert_ne!(dir_key(&app), dir_key(&PathBuf::from(r"C:\proj\v1.0")));
        assert_ne!(
            dir_key(&app),
            dir_key(&PathBuf::from(r"C:\proj\v1.0\app\.config"))
        );
        assert_ne!(dir_key(&app), dir_key(&PathBuf::from(r"C:\proj\app")));
        // 而真正的 `.` 段确实被折掉。
        assert_eq!(
            dir_key(&app),
            dir_key(&PathBuf::from(r"C:\proj\v1.0\app\."))
        );
        assert_eq!(
            dir_key(&app),
            dir_key(&PathBuf::from(r"C:\proj\v1.0\app\sub\.."))
        );

        let Some(home) = dirs::home_dir() else {
            return;
        };
        // 正常项目根(哪怕就在 HOME 底下、哪怕名字里带点)不被误判成 HOME。
        assert!(!is_home_root(&home.join("proj").join("v1.0").join("app")));
        assert!(!is_home_root(&home.join("v1.0")));
        assert!(!is_home_root(&home.join(".config")));
        // `..` 真的向上一级:HOME 的父目录不是 HOME。
        assert!(!is_home_root(&home.join("..")));
        assert!(!is_home_root(&home.join("a").join("..").join("..")));
    }

    /// 优先级定死:参数 > 环境变量 > 发现式。本函数只看「显式还是没有」这一层;
    /// 参数与环境变量的先后由 CLI 侧合成(main.rs 的 `explicit_main_root`)。
    #[test]
    fn resolve_project_root显式优先() {
        let root = std::env::temp_dir().join(format!(
            "kanzei-r182-resolve-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let sub = root.join("sub");
        std::fs::create_dir_all(root.join(".kanzei")).unwrap();
        std::fs::create_dir_all(sub.join(".kanzei")).unwrap();

        // 显式给了根:cwd 在 sub 里也照样返回 root。
        assert_eq!(
            resolve_project_root(Some(&root), &sub).unwrap(),
            root.clone()
        );
        // 没给:逐字节退回 discover_project_root——这同时证明本批没去改它。
        assert_eq!(
            resolve_project_root(None, &sub).unwrap(),
            discover_project_root(&sub).unwrap()
        );
        assert_eq!(resolve_project_root(None, &sub).unwrap(), sub.clone());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn 显式主根必须是真项目根() {
        let root = std::env::temp_dir().join(format!(
            "kanzei-r182-marker-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let empty = root.join("empty");
        let file = root.join("a-file.txt");
        let worktree = root.join("worktree");
        std::fs::create_dir_all(&empty).unwrap();
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(&file, "not a directory").unwrap();
        // worktree 的 `.git` 是文件,必须照样算标记。
        std::fs::write(worktree.join(".git"), "gitdir: ../repo/.git/worktrees/w\n").unwrap();

        for bad in [root.join("does-not-exist"), empty.clone(), file.clone()] {
            let error = resolve_project_root(Some(&bad), &root)
                .unwrap_err()
                .to_string();
            // 错误必须点名来源键名,否则用户不知道该去改哪个开关/变量。
            assert!(
                error.contains("--project-root") && error.contains("KANZEI_PROJECT_ROOT"),
                "错误文本要点名来源: {error}"
            );
        }
        assert_eq!(
            resolve_project_root(Some(&worktree), &root).unwrap(),
            worktree
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    /// 不做 canonicalize:`\\?\` 形态会让用户已写的绝对路径权限规则一夜失配。
    #[test]
    fn 显式主根不做canonicalize() {
        let root = std::env::temp_dir().join(format!(
            "kanzei-r182-nocanon-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".kanzei")).unwrap();

        let mut forms = vec![PathBuf::from(format!(
            "{}{}",
            root.display(),
            std::path::MAIN_SEPARATOR
        ))];
        #[cfg(windows)]
        forms.push(PathBuf::from(
            root.display().to_string().to_lowercase().replace('\\', "/"),
        ));
        for form in forms {
            let resolved = resolve_project_root(Some(&form), &root).unwrap();
            assert!(
                !resolved.display().to_string().starts_with(r"\\?\"),
                "不该 canonicalize: {}",
                resolved.display()
            );
            // 原样返回:用户写下什么就是什么。
            assert_eq!(resolved, form);
        }

        std::fs::remove_dir_all(root).unwrap();
    }

    /// D-630:临时根自己的 `.kanzei` 不算项目标记——否则它像 HOME 磁铁一样吸走
    /// 全部无标记临时子目录,多个测试/进程的 state.db 并进同一份互相污染。
    #[test]
    fn 临时根的kanzei不算项目标记() {
        let fake_temp = std::env::temp_dir().join(format!(
            "kanzei-d630-temp-root-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        // 结构:base/.git(终止层)> base/faketemp/.kanzei(被排除的假临时根)>
        // base/faketemp/session-a(无标记)。向上走越过假临时根的 `.kanzei` 停在
        // `.git` 层,证明排除生效;不依赖真实 %TEMP%/HOME 的现场状态。
        let base = fake_temp;
        std::fs::create_dir_all(base.join(".git")).unwrap();
        let inner_temp = base.join("faketemp");
        std::fs::create_dir_all(inner_temp.join(".kanzei")).unwrap();
        let plain_sub = inner_temp.join("session-a");
        std::fs::create_dir_all(&plain_sub).unwrap();
        assert_eq!(
            discover_project_root_with_roots(&plain_sub, None, Some(&inner_temp)),
            Some(base.clone()),
            "无标记子目录不得被临时根的 .kanzei 吸走"
        );
        // 自带 `.kanzei` 的子目录仍是正常项目根。
        let marked_sub = inner_temp.join("session-b");
        std::fs::create_dir_all(marked_sub.join(".kanzei")).unwrap();
        assert_eq!(
            discover_project_root_with_roots(&marked_sub, None, Some(&inner_temp)),
            Some(marked_sub.clone())
        );
        std::fs::remove_dir_all(base).unwrap();
    }
}
