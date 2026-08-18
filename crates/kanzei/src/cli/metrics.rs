//! `kz metrics` 巨石度量口径(R-258)。
//!
//! 独立理由:巨石拆解需要「生产行数/测试行数/最大函数行数/参数个数」的可信口径——
//! 本仓测试与生产码同文件,raw 行数(`wc -l` / GitHub 页面行数)与巨石程度无关;
//! 把 raw 行数做成门禁会惩罚测试写得最足的模块(搬走测试就能过线)。本模块产出
//! 度量入口,不做提交闸门(自用工具,防线放可见性不放闸门)。
//!
//! 口径(验收原文):
//! - 生产行数 = 总行数 − cfg(test) 块行数。cfg(test) 块**按大括号配平**识别:
//!   进入测试块需要 `#[cfg(test)]` 属性后跟**带 `{` 的 item**(如 `mod tests {`);
//!   外挂测试模块声明 `#[cfg(test)] mod scheduling_tests;`(无 `{`)不算测试块,
//!   不能一刀切把后面所有行误报成测试(processes.rs L468 教训)。
//! - 函数度量(函数数/最大函数行数/too_many_arguments 处数)只统计生产码。
//!   too_many_arguments 沿用 clippy 默认阈值:参数 > 7。
//!
//! 危险点:字符串字面量与注释里的 `{`/`}`/`fn` 不计入配平与函数统计;`#[cfg(test)]`
//! 属性行必须 trim 后以 `#[cfg(test)]` 开头(前面可有空白),属性与 item 分行与同行
//! 两种写法都要覆盖;`fn` 检测要求独立 token 边界(前后不是标识符字符)。

use std::path::Path;

use super::{explicit_main_root, main_project_root};

/// 单个文件的度量结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FileMetric {
    /// 总行数(含空行/注释/测试)。
    pub(crate) total_lines: usize,
    /// 生产行数 = 总行数 − cfg(test) 块行数。
    pub(crate) prod_lines: usize,
    /// cfg(test) 块行数(含块内空行/注释)。
    pub(crate) test_lines: usize,
    /// 生产码函数数。
    pub(crate) fn_count: usize,
    /// 生产码最大函数行数(含函数体)。
    pub(crate) max_fn_lines: usize,
    /// 生产码中参数 > 7 的函数数(clippy too_many_arguments 默认阈值)。
    pub(crate) too_many_args_count: usize,
}

/// 度量一个 Rust 源文件(纯函数,可测试)。
pub(crate) fn metric_file(path: &Path) -> std::io::Result<FileMetric> {
    let text = std::fs::read_to_string(path)?;
    Ok(metric_source(&text))
}

/// 行内词法状态(跨行持久:块注释)。
#[derive(Default)]
struct LineLex {
    block_comment: bool,
    in_string: bool,
    in_char: bool,
    /// 本行净花括号增量(字符串/注释外的 `{` +1、`}` −1)。
    brace_delta: i32,
    /// 本行是否有字符串/注释外的 `{`。
    has_open_brace: bool,
    /// 本行是否为纯属性行 `#[cfg(test)]`(trim 后以它开头)。
    cfg_test_attr: bool,
    /// 本行内检测到的生产 fn 起始列(字符串/注释外、token 边界)。
    fns: Vec<usize>,
}

/// 对源码文本做度量(纯函数,与文件系统解耦以便单测)。
pub(crate) fn metric_source(text: &str) -> FileMetric {
    let lines: Vec<&str> = text.lines().collect();
    let total_lines = lines.len();

    // 第一遍:词法逐行扫描,标记测试块范围 + 收集生产 fn 位置。
    let mut lexed: Vec<LineLex> = Vec::with_capacity(lines.len());
    let mut in_block_comment = false;
    for line in &lines {
        let mut lx = LineLex {
            block_comment: in_block_comment,
            ..Default::default()
        };
        let trimmed = line.trim_start();
        lx.cfg_test_attr = trimmed.starts_with("#[cfg(test)]");
        let bytes = line.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            let c = bytes[i] as char;
            if lx.block_comment {
                if c == '*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    lx.block_comment = false;
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }
            if lx.in_string {
                if c == '\\' {
                    i += 2;
                    continue;
                }
                if c == '"' {
                    lx.in_string = false;
                }
                i += 1;
                continue;
            }
            if lx.in_char {
                if c == '\\' {
                    i += 2;
                    continue;
                }
                if c == '\'' {
                    lx.in_char = false;
                }
                i += 1;
                continue;
            }
            match c {
                '/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => break, // 行注释,本行结束
                '/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                    lx.block_comment = true;
                    i += 2;
                    continue;
                }
                '"' => lx.in_string = true,
                '\'' => {
                    // Rust lifetimes (`'static`, `'_`) begin with an identifier and
                    // are not character literals. A character such as `'a'` has a
                    // closing quote immediately after the one-character payload.
                    let next = bytes.get(i + 1).copied();
                    let after_next = bytes.get(i + 2).copied();
                    let starts_lifetime = next
                        .map(|byte| (byte as char).is_ascii_alphabetic() || byte == b'_')
                        .unwrap_or(false)
                        && after_next != Some(b'\'');
                    if !starts_lifetime {
                        lx.in_char = true;
                    }
                }
                '{' => {
                    lx.brace_delta += 1;
                    lx.has_open_brace = true;
                }
                '}' => lx.brace_delta -= 1,
                'f' if bytes.get(i + 1) == Some(&b'n') => {
                    // token 边界:前一个字符不是标识符字符,后一个不是标识符字符。
                    let before = if i == 0 { ' ' } else { bytes[i - 1] as char };
                    let after = bytes.get(i + 2).copied().map(|b| b as char).unwrap_or(' ');
                    if !before.is_alphanumeric()
                        && before != '_'
                        && !after.is_alphanumeric()
                        && after != '_'
                    {
                        lx.fns.push(i);
                    }
                    i += 1;
                    continue;
                }
                _ => {}
            }
            i += 1;
        }
        in_block_comment = lx.block_comment;
        lexed.push(lx);
    }

    // 第二遍:测试块配平(逐行应用 brace_delta),同时给生产 fn 归属行号。
    let mut test_lines = 0usize;
    let mut in_test = false;
    let mut test_depth: i32 = 0;
    // 上一行是否是 `#[cfg(test)]` 属性行(待本行消费:进测试块或当作外挂声明清除)。
    let mut prev_cfg = false;
    // (行号, 起始列) — 行号为 1-based。
    let mut prod_fns: Vec<(usize, usize)> = Vec::new();
    for (idx, lx) in lexed.iter().enumerate() {
        let line_no = idx + 1;
        if in_test {
            test_depth += lx.brace_delta;
            test_lines += 1;
            if test_depth <= 0 {
                in_test = false;
                test_depth = 0;
            }
            continue;
        }
        // 非测试块:cfg(test) 属性后跟带 `{` 的 item → 进入测试块。
        // 同行的 `#[cfg(test)] mod tests {` 也在同一行判定:该行既是属性又是打开。
        if lx.has_open_brace && (prev_cfg || lx.cfg_test_attr) {
            in_test = true;
            test_depth = lx.brace_delta;
            test_lines += 1;
            prev_cfg = false; // 本行已消费 cfg 标记
            if test_depth <= 0 {
                in_test = false;
                test_depth = 0;
            }
            continue;
        }
        // 本行是属性行:挂起标记(下一行判定);若本行同时是外挂声明(无 `{`)
        // 或普通代码,标记在下一轮被消费或清除。
        prev_cfg = lx.cfg_test_attr;
        for col in &lx.fns {
            prod_fns.push((line_no, *col));
        }
    }

    let prod_lines = total_lines.saturating_sub(test_lines);

    // 第三遍:对每个生产 fn 统计参数个数与函数体行数。
    // 函数体从该行起,到最近一层 `{...}` 配平结束(跨行词法,字符串/注释跳过)。
    let fn_count = prod_fns.len();
    let mut max_fn_lines = 0usize;
    let mut too_many_args_count = 0usize;
    for (start_line, start_col) in &prod_fns {
        let arg_count = count_fn_args(&lines, *start_line, *start_col);
        if arg_count > 7 {
            too_many_args_count += 1;
        }
        let body_lines = fn_body_lines(&lines, *start_line, *start_col);
        if body_lines > max_fn_lines {
            max_fn_lines = body_lines;
        }
    }

    FileMetric {
        total_lines,
        prod_lines,
        test_lines,
        fn_count,
        max_fn_lines,
        too_many_args_count,
    }
}

/// 统计一个 fn(行号 1-based、起始列)的参数个数:从 fn 名后第一个 `(` 到匹配的
/// `)`,顶层逗号数 + 1(无参 = 0)。跳过字符串/注释/嵌套括号。
fn count_fn_args(lines: &[&str], start_line: usize, _start_col: usize) -> usize {
    let mut in_parens = false;
    let mut in_string = false;
    let mut in_char = false;
    let mut block_comment = false;
    // 参数个数:初始 1(至少一个参数);空括号 `()` 在闭合处特判返回 0。
    let mut args = 1usize;
    let mut paren_depth = 0i32;
    for line in &lines[start_line - 1..] {
        let bytes = line.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            let c = bytes[i] as char;
            if block_comment {
                if c == '*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    block_comment = false;
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }
            if in_string {
                if c == '\\' {
                    i += 2;
                    continue;
                }
                if c == '"' {
                    in_string = false;
                }
                i += 1;
                continue;
            }
            if in_char {
                if c == '\\' {
                    i += 2;
                    continue;
                }
                if c == '\'' {
                    in_char = false;
                }
                i += 1;
                continue;
            }
            match c {
                '/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => break,
                '/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                    block_comment = true;
                    i += 2;
                    continue;
                }
                '"' => in_string = true,
                '\'' => in_char = true,
                '(' => {
                    if !in_parens {
                        in_parens = true; // fn 参数括号
                    }
                    paren_depth += 1;
                }
                ')' => {
                    paren_depth -= 1;
                    if in_parens && paren_depth == 0 {
                        // 参数括号闭合:空括号 `()` 无参数,否则 args 已含初值 1。
                        let prev = if i == 0 { ' ' } else { bytes[i - 1] as char };
                        return if prev == '(' { 0 } else { args };
                    }
                }
                ',' if in_parens && paren_depth == 1 => {
                    // 只有参数列表最外层的逗号才分隔参数。
                    let prev = if i == 0 { ' ' } else { bytes[i - 1] as char };
                    if prev != '(' {
                        args += 1;
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }
    args
}

/// 函数体行数:从 fn 起始行找到第一个 `{`(字符串/注释外),配平到 `}` 结束。
/// 找不到函数体(如 trait 声明 `fn foo();`)返回 1(仅声明行)。
fn fn_body_lines(lines: &[&str], start_line: usize, _start_col: usize) -> usize {
    let mut in_string = false;
    let mut in_char = false;
    let mut block_comment = false;
    let mut depth: i32 = 0;
    let mut found_open = false;
    let mut body_start = 0usize;
    let total = lines.len();
    for (off, line) in lines[start_line - 1..].iter().enumerate() {
        let line_no = start_line + off;
        let bytes = line.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            let c = bytes[i] as char;
            if block_comment {
                if c == '*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    block_comment = false;
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }
            if in_string {
                if c == '\\' {
                    i += 2;
                    continue;
                }
                if c == '"' {
                    in_string = false;
                }
                i += 1;
                continue;
            }
            if in_char {
                if c == '\\' {
                    i += 2;
                    continue;
                }
                if c == '\'' {
                    in_char = false;
                }
                i += 1;
                continue;
            }
            match c {
                '/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => break,
                '/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                    block_comment = true;
                    i += 2;
                    continue;
                }
                '"' => in_string = true,
                '\'' => in_char = true,
                '{' => {
                    depth += 1;
                    if !found_open {
                        found_open = true;
                        body_start = line_no;
                    }
                }
                '}' => {
                    depth -= 1;
                    if found_open && depth == 0 {
                        return line_no - body_start + 1;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        if !found_open && line_no > start_line && !line.trim().is_empty() {
            // fn 行之后还没看到 `{`:如果遇到 `;`(声明式 fn),直接返回 1。
            if line.trim_end().ends_with(';') {
                return 1;
            }
        }
    }
    if found_open {
        total.saturating_sub(body_start - 1)
    } else {
        1
    }
}

pub(crate) async fn metrics_cli(args: &[String]) -> anyhow::Result<()> {
    let mut top = 20usize;
    let mut root_override = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--top" => {
                i += 1;
                top = args
                    .get(i)
                    .ok_or_else(|| anyhow::anyhow!("--top 需要一个数字"))?
                    .parse()
                    .map_err(|_| anyhow::anyhow!("--top 需要正整数"))?;
            }
            "--project-root" => {
                i += 1;
                root_override = Some(
                    args.get(i)
                        .ok_or_else(|| anyhow::anyhow!("--project-root 需要路径"))?,
                );
            }
            other => anyhow::bail!("未知参数: {other}"),
        }
        i += 1;
    }

    let cwd = std::env::current_dir()?;
    let project_root = main_project_root(
        explicit_main_root(root_override.map(std::path::PathBuf::from).as_deref()).as_deref(),
        &cwd,
    )?;

    let files = sorted_metrics(&project_root)?;
    println!("项目根: {}", project_root.display());
    println!(
        "{:<54} {:>7} {:>7} {:>7} {:>5} {:>7} {:>6}",
        "文件", "总行", "生产", "测试", "函数", "最大fn", ">7参"
    );
    for (rel, m) in files.iter().take(top) {
        println!(
            "{:<54} {:>7} {:>7} {:>7} {:>5} {:>7} {:>6}",
            rel.display().to_string(),
            m.total_lines,
            m.prod_lines,
            m.test_lines,
            m.fn_count,
            m.max_fn_lines,
            m.too_many_args_count,
        );
    }
    println!("(共 {} 个 .rs 文件,Top-{})", files.len(), top);
    Ok(())
}

/// 全仓度量排序结果(生产行数降序);供 CLI 与快照共用。
pub(crate) fn sorted_metrics(
    root: &Path,
) -> std::io::Result<Vec<(std::path::PathBuf, FileMetric)>> {
    let mut files = Vec::new();
    for entry in walk_rust_files(root)? {
        let rel = entry.strip_prefix(root).unwrap_or(&entry).to_path_buf();
        if let Ok(mut m) = metric_file(&entry) {
            // 外挂测试文件(`_tests.rs` 后缀或 tests/ 目录):整体是 `#[cfg(test)]
            // mod x_tests;` 声明的纯测试,无内联 cfg(test) 块,整文件算测试行——
            // 否则 raw 行数会把纯测试文件误诊成生产巨石(R-258 来源 tracker.rs 教训)。
            if is_external_test_file(&rel) {
                m.test_lines = m.total_lines;
                m.prod_lines = 0;
                m.fn_count = 0;
                m.max_fn_lines = 0;
                m.too_many_args_count = 0;
            }
            files.push((rel, m));
        }
    }
    files.sort_by_key(|(_, m)| std::cmp::Reverse(m.prod_lines));
    Ok(files)
}

/// 外挂测试文件判定:文件名以 `_tests.rs` 结尾,或路径含 `tests/` 目录
/// (integration/ 测试夹)。
fn is_external_test_file(rel: &Path) -> bool {
    let name = rel
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    if name.ends_with("_tests.rs") {
        return true;
    }
    rel.components().any(|c| {
        let s = c.as_os_str().to_string_lossy();
        s == "tests"
    })
}

/// 递归收集 crates/ 下全部 .rs 文件(排除 target/ 与 gen 等生成物)。
fn walk_rust_files(root: &Path) -> std::io::Result<Vec<std::path::PathBuf>> {
    let mut out = Vec::new();
    let crates_dir = root.join("crates");
    if crates_dir.is_dir() {
        collect_rs(&crates_dir, &mut out)?;
    }
    Ok(out)
}

fn collect_rs(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if name == "target" || name == "gen" {
                continue;
            }
            collect_rs(&path, out)?;
        } else if name.ends_with(".rs") {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cfg_test_block_external_decl_is_not_block() {
        // processes.rs L468 教训:外挂测试模块声明 `#[cfg(test)] mod x;` 无 `{`,
        // 后面跟的仍是生产码;一刀切会把生产码误报成测试。
        let src = "pub fn real() {}\n\
                   #[cfg(test)]\n\
                   mod external_tests;\n\
                   pub fn still_prod() {}\n\
                   #[cfg(test)]\n\
                   mod tests {\n\
                       #[test]\n\
                       fn t() {}\n\
                   }\n";
        let m = metric_source(src);
        // 总 9 行;测试块只有 `mod tests {`..`}` 4 行;外挂声明不算。
        assert_eq!(m.total_lines, 9);
        assert_eq!(m.test_lines, 4);
        assert_eq!(m.prod_lines, 5);
        assert_eq!(m.fn_count, 2); // real + still_prod;t 在测试块内不计
    }

    #[test]
    fn cfg_test_attr_and_item_same_line() {
        let src = "#[cfg(test)] mod tests { fn t() {} }\nfn prod() {}\n";
        let m = metric_source(src);
        assert_eq!(m.total_lines, 2);
        assert_eq!(m.test_lines, 1);
        assert_eq!(m.prod_lines, 1);
        assert_eq!(m.fn_count, 1);
    }

    #[test]
    fn braces_in_string_and_comment_ignored() {
        let src = "fn f() {\n    let s = \"{ not brace }\"; // } not close\n    let c = '{';\n}\n";
        let m = metric_source(src);
        assert_eq!(m.total_lines, 4);
        assert_eq!(m.test_lines, 0);
        assert_eq!(m.prod_lines, 4);
        assert_eq!(m.fn_count, 1);
        assert_eq!(m.max_fn_lines, 4);
    }

    #[test]
    fn nested_blocks_inside_test_counted_as_test() {
        let src =
            "#[cfg(test)]\nmod tests {\n    fn helper() {\n        if true {\n        }\n    }\n}\nfn prod() {}\n";
        let m = metric_source(src);
        // 测试块从 `mod tests {`(L2)到 `}`(L7),共 6 行。
        assert_eq!(m.test_lines, 6);
        assert_eq!(m.prod_lines, m.total_lines - 6);
        assert_eq!(m.fn_count, 1); // 只有 prod
    }

    #[test]
    fn lifetime_is_not_a_character_literal_and_does_not_end_cfg_test_block() {
        let src = "#[cfg(test)]\n\
                   mod tests {\n\
                       fn helper() {\n\
                           let value: &'static str = \"ok\";\n\
                       }\n\
                   }\n\
                   fn prod() {}\n";
        let m = metric_source(src);
        assert_eq!(m.test_lines, 5);
        assert_eq!(m.prod_lines, 2);
        assert_eq!(m.fn_count, 1);
    }

    #[test]
    fn too_many_args_detected() {
        let src = "fn small(a: i32) {}\n\
                   fn large(a: i32, b: i32, c: i32, d: i32, e: i32, f: i32, g: i32, h: i32) {}\n";
        let m = metric_source(src);
        assert_eq!(m.fn_count, 2);
        assert_eq!(m.too_many_args_count, 1);
        assert_eq!(m.max_fn_lines, 1);
    }

    #[test]
    fn fn_decl_without_body_counts_one_line() {
        let src = "trait T {\n    fn declared(&self) -> i32;\n}\n";
        let m = metric_source(src);
        assert_eq!(m.fn_count, 1);
        assert_eq!(m.max_fn_lines, 1);
    }

    #[test]
    fn external_test_file_detected_by_name_and_dir() {
        assert!(is_external_test_file(std::path::Path::new(
            "crates/kanzei-app/src/worktree_tests.rs"
        )));
        assert!(!is_external_test_file(std::path::Path::new(
            "crates/kanzei-app/src/state.rs"
        )));
        assert!(is_external_test_file(std::path::Path::new(
            "crates/kanzei/tests/integration/bash_action_literal.rs"
        )));
        assert!(!is_external_test_file(std::path::Path::new(
            "crates/kanzei-tools/src/tracker.rs"
        )));
    }
}
