//! 路径形态的唯一归一实现(UI2-0926 #13「工作目录的管理」)。
//!
//! `std::fs::canonicalize` 在 Windows 上返回 `\\?\C:\…` 扩展长度(verbatim)形态。它作为
//! 身份键漏进进程记录后,一路流到了鞭挞轮的系统提示、bash 工作目录、权限规则与取活顺序的
//! 存储键——同一个项目,手动轮用 `C:\…`,自动轮用 `\\?\C:\…`(docs/design/project_workspace.md §3)。
//! 在此之前各处各剥各的前缀(至少 7 份),口径还不一样。
//!
//! 这里给两种形态,别混用:
//!
//! - [`simplify`] / [`canonical`]:**可以继续当路径用**的形态。规则与 `dunce` 相同——只有剥掉前缀
//!   之后语义不变时才剥:长度超过 260(UTF-16 计)、含 DOS 保留名(CON/NUL/COM1…,带扩展名也算)、
//!   组件以点或空格结尾、组件含非法字符、`.`/`..` 组件、没有根目录的 `\\?\C:`,一律原样返回——
//!   这些路径只有 verbatim 形态才能被 Win32 正确解析,剥了反而打不开。比 dunce 多一条:
//!   `\\?\UNC\server\share\…` 在同样的安全条件下化为 `\\server\share\…`。
//! - [`strip_verbatim`]:**只做比较键**的形态,无条件剥前缀(原来 7 份实现的共同部分)。
//!   比较键不拿去打开文件,超长路径剥了也无妨。
//!
//! 本模块纯 std、零依赖,不做大小写与分隔符归一——那是各比较键自己的口径。

use std::borrow::Cow;
use std::path::{Path, PathBuf};

const VERBATIM: &str = r"\\?\";
const VERBATIM_UNC: &str = r"\\?\UNC\";
/// Win32 经典路径上限(MAX_PATH);超过它的路径必须保留 verbatim 形态。
const MAX_PATH: usize = 260;
/// 单个文件名的上限(UTF-16 计)。
const MAX_COMPONENT: usize = 255;
const RESERVED_NAMES: [&str; 22] = [
    "AUX", "NUL", "PRN", "CON", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// 比较键用:无条件剥掉 `\\?\` 与 `\\?\UNC\` 前缀(后者化为 `\\`)。不改大小写与分隔符。
pub fn strip_verbatim(raw: &str) -> Cow<'_, str> {
    if let Some(rest) = raw.strip_prefix(VERBATIM_UNC) {
        return Cow::Owned(format!(r"\\{rest}"));
    }
    match raw.strip_prefix(VERBATIM) {
        Some(rest) => Cow::Borrowed(rest),
        None => Cow::Borrowed(raw),
    }
}

/// 可安全去掉前缀时返回去掉后的形态,否则原样(规则见模块注释)。非 verbatim 路径原样返回;幂等。
pub fn simplify_str(raw: &str) -> Cow<'_, str> {
    if !raw.starts_with(VERBATIM) {
        return Cow::Borrowed(raw);
    }
    if raw.encode_utf16().count() > MAX_PATH {
        return Cow::Borrowed(raw);
    }
    if let Some(rest) = raw.strip_prefix(VERBATIM_UNC) {
        // server\share\… :server 与 share 都必须是合法组件,之后的组件同样检查。
        let mut parts = rest.split('\\');
        let (Some(server), Some(share)) = (parts.next(), parts.next()) else {
            return Cow::Borrowed(raw);
        };
        if !is_safe_component(server) || !is_safe_component(share) {
            return Cow::Borrowed(raw);
        }
        if !components_safe(parts) {
            return Cow::Borrowed(raw);
        }
        return Cow::Owned(format!(r"\\{rest}"));
    }
    let rest = &raw[VERBATIM.len()..];
    // 只认盘符形态 `X:\…`;`\\?\Volume{…}`、`\\?\GLOBALROOT` 这类没有等价的经典写法。
    let bytes = rest.as_bytes();
    if bytes.len() < 3 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' || bytes[2] != b'\\' {
        return Cow::Borrowed(raw);
    }
    if !components_safe(rest[3..].split('\\')) {
        return Cow::Borrowed(raw);
    }
    Cow::Borrowed(rest)
}

/// [`simplify_str`] 的 Path 版。非 UTF-8 的路径原样返回(无法无损地操作)。
pub fn simplify(path: &Path) -> PathBuf {
    match path.to_str() {
        Some(text) => PathBuf::from(simplify_str(text).as_ref()),
        None => path.to_path_buf(),
    }
}

/// `std::fs::canonicalize` + [`simplify`]:解析符号链接与大小写,输出能直接当路径用的经典形态。
pub fn canonical(path: &Path) -> std::io::Result<PathBuf> {
    std::fs::canonicalize(path).map(|resolved| simplify(&resolved))
}

/// [`canonical`] 失败(目录不存在等)时退回 `simplify(path)`。
pub fn canonical_or_simplified(path: &Path) -> PathBuf {
    canonical(path).unwrap_or_else(|_| simplify(path))
}

fn components_safe<'a>(parts: impl Iterator<Item = &'a str>) -> bool {
    let parts: Vec<&str> = parts.collect();
    let last = parts.len().saturating_sub(1);
    for (index, part) in parts.iter().enumerate() {
        // 末尾分隔符(`C:\a\`)留下的空组件无害;中间的空组件(`a\\b`)在 verbatim 下是字面量。
        if part.is_empty() {
            if index == last {
                continue;
            }
            return false;
        }
        if !is_safe_component(part) {
            return false;
        }
    }
    true
}

fn is_safe_component(name: &str) -> bool {
    if name.is_empty() || name == "." || name == ".." {
        return false;
    }
    if name.encode_utf16().count() > MAX_COMPONENT {
        return false;
    }
    if name.bytes().any(|c| {
        matches!(
            c,
            0..=31 | b'<' | b'>' | b':' | b'"' | b'/' | b'\\' | b'|' | b'?' | b'*'
        )
    }) {
        return false;
    }
    if name.ends_with(' ') || name.ends_with('.') {
        return false;
    }
    !is_reserved(name)
}

/// DOS 保留名:`con`、`con.txt`、`CON .txt` 都是 CON。
fn is_reserved(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).trim_end_matches(' ');
    stem.len() <= 4
        && RESERVED_NAMES
            .iter()
            .any(|reserved| stem.eq_ignore_ascii_case(reserved))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 盘符形态去掉前缀() {
        assert_eq!(simplify_str(r"\\?\C:\a\b"), r"C:\a\b");
        assert_eq!(simplify_str(r"\\?\C:\"), r"C:\");
        assert_eq!(
            simplify_str(r"\\?\C:\Users\kanzei\Desktop\MD文件保存"),
            r"C:\Users\kanzei\Desktop\MD文件保存"
        );
        assert_eq!(simplify_str(r"\\?\C:\a\b\"), r"C:\a\b\");
    }

    #[test]
    fn unc_形态化为双反斜杠() {
        assert_eq!(simplify_str(r"\\?\UNC\srv\share\x"), r"\\srv\share\x");
        assert_eq!(simplify_str(r"\\?\UNC\srv\share"), r"\\srv\share");
        // 没有 share 的 UNC 不是完整的网络路径,保留原样。
        assert_eq!(simplify_str(r"\\?\UNC\srv"), r"\\?\UNC\srv");
    }

    #[test]
    fn 不安全时原样保留() {
        // 每段都合法、只是总长超过 260:单看组件判据会放行,必须靠总长判据留住。
        let long = format!(r"\\?\C:\{}x", r"abcdefghij\".repeat(30));
        assert!(long.encode_utf16().count() > MAX_PATH);
        assert_eq!(
            simplify_str(&long),
            long.as_str(),
            "超过 260 必须保留 verbatim"
        );
        let short = format!(r"\\?\C:\{}x", r"abcdefghij\".repeat(20));
        assert_eq!(
            simplify_str(&short),
            &short[4..],
            "不到 260 的同形路径照常去前缀"
        );
        let long_name = format!(r"\\?\C:\{}\x", "b".repeat(256));
        assert_eq!(simplify_str(&long_name), long_name.as_str());
        for raw in [
            r"\\?\C:\proj\CON",
            r"\\?\C:\proj\con.txt",
            r"\\?\C:\proj\Nul .log",
            r"\\?\C:\proj\lpt9\x",
            r"\\?\C:\proj\dot.",
            r"\\?\C:\proj\space ",
            r"\\?\C:\proj\..\x",
            r"\\?\C:\proj\.\x",
            r"\\?\C:\proj\\x",
            r"\\?\C:\a/b",
            r"\\?\C:\a*b",
            r"\\?\C:",
            r"\\?\Volume{0000}\x",
            r"\\?\GLOBALROOT\Device\x",
        ] {
            assert_eq!(simplify_str(raw), raw, "{raw} 剥前缀会改变语义,必须原样");
        }
        // 保留名只按主干判断:含保留名的更长名字照常可剥。
        assert_eq!(simplify_str(r"\\?\C:\console\conf"), r"C:\console\conf");
    }

    #[test]
    fn 非_verbatim_原样且幂等() {
        for raw in [r"C:\a\b", r"\\srv\share\x", "/home/u/p", "relative\\x", ""] {
            assert_eq!(simplify_str(raw), raw);
        }
        let once = simplify_str(r"\\?\C:\a\b").into_owned();
        assert_eq!(simplify_str(&once), once.as_str());
        let kept = format!(r"\\?\C:\{}", "a".repeat(300));
        assert_eq!(simplify_str(simplify_str(&kept).as_ref()), kept.as_str());
    }

    #[test]
    fn 比较键无条件剥前缀() {
        assert_eq!(strip_verbatim(r"\\?\C:\a"), r"C:\a");
        assert_eq!(strip_verbatim(r"\\?\UNC\s\sh\x"), r"\\s\sh\x");
        let long = format!(r"\\?\C:\{}", "a".repeat(300));
        assert_eq!(strip_verbatim(&long), &long[4..]);
        assert_eq!(strip_verbatim(r"C:\a"), r"C:\a");
    }

    #[test]
    fn canonical_不带前缀() {
        let dir = std::env::temp_dir();
        let resolved = canonical(&dir).expect("temp dir canonicalize");
        assert!(
            !resolved.display().to_string().starts_with(VERBATIM),
            "{}",
            resolved.display()
        );
        let missing = dir.join("kz-path-form-missing-dir-xyz");
        assert_eq!(canonical_or_simplified(&missing), missing);
    }
}
