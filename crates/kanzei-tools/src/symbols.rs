//! R-234 B1:代码符号/结构级视图工具(symbols)。填补 files(行数)与 read(全文)
//! 之间的粒度空白——对指定 Rust 文件/crate 输出符号列表(函数/结构/impl/enum,
//! 带行号与可见性),agent 不必 read 全文即可定位质量热点。
//!
//! 解析策略:轻量行级扫描,不引入 syn 等重依赖(与 R-154 拆分的轻量哲学一致)。
//! 精确识别 `fn name` / `struct Name` / `enum Name` / `impl` / `pub fn` 等定义行,
//! 跳过注释与字符串内的伪命中;不支持跨行宏展开(那是 IDE 的事,这里只给结构地图)。

use std::path::Path;

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolConcurrency, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Deserialize, JsonSchema)]
struct SymbolsInput {
    /// 要扫描的文件或目录(相对 cwd)。目录 = 递归扫 .rs 文件。
    #[serde(default)]
    path: Option<String>,
    /// 只列出匹配此子串的符号名(定位特定函数)。
    #[serde(default)]
    filter: Option<String>,
    /// 只输出公共符号(pub fn/pub struct 等)。默认全量。
    #[serde(default)]
    public_only: bool,
    /// R-234 B2:查「谁调用此符号」——在目标树里列出对该符号名的引用点
    /// (file:line)。填符号名(如 `parse_symbol_line`),与 path 配合使用。
    #[serde(default)]
    callers: Option<String>,
    /// R-265:查「这个符号定义在哪」——输入裸名或限定路径
    /// (如 `try_lock_exclusive` 或 `crate::atomic_file::try_lock_exclusive`),
    /// 全树按符号名精确命中定义点,并给出跨 crate re-export 链。
    /// 与 callers 互斥(同时给出会显式报错)。
    #[serde(default)]
    define: Option<String>,
    /// R-310 B3:按 workspace crate 生成实时分层地图(ident 使用 Cargo 的 `-`→`_`约定)。
    #[serde(default, rename = "crate")]
    crate_name: Option<String>,
    /// R-310 B3:按模块路径过滤分层地图,如 `runner` 或 `runner::drive`。
    #[serde(default)]
    module: Option<String>,
}

/// 一个符号:名称、种类、行号、可见性。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Symbol {
    kind: &'static str,
    name: String,
    line: usize,
    public: bool,
}

/// R-234 B1:符号列表提取器。
pub struct SymbolsTool;

#[async_trait]
impl Tool for SymbolsTool {
    fn name(&self) -> &'static str {
        "symbols"
    }

    fn description(&self) -> String {
        "Code symbol map (R-234): list functions/structs/enums/impls with line numbers \
         and visibility for a Rust file or directory, WITHOUT reading the whole file. \
         Fills the granularity gap between `files` (line counts) and `read` (full text): \
         use it to locate quality hotspots (huge functions, orphan impls, non-pub \
         surface) before deciding what to read. Params: path (file or dir, relative to \
         cwd), filter (substring match on symbol name), public_only, callers (symbol \
         name — list reference points that call it, capped at 50), define (bare name or \
         crate::path — locate its definition anywhere in the tree, resolving cross-crate \
         re-exports; mutually exclusive with callers), crate (workspace crate ident, \
         `-` becomes `_`) and module (module path prefix) for a live crate→module→public \
         symbol map. Map queries rescan the current worktree, so commits never leave a \
         stale persisted index behind.)"
            .into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(SymbolsInput)).unwrap()
    }

    fn concurrency(&self, _input: &serde_json::Value, ctx: &ToolCtx) -> ToolConcurrency {
        ToolConcurrency::shared_worktree(ctx)
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: SymbolsInput = match crate::parse_input(self, input) {
            Ok(value) => value,
            Err(output) => return output,
        };
        let target = input
            .path
            .as_deref()
            .map(|p| ctx.cwd.join(p))
            .unwrap_or_else(|| ctx.cwd.clone());
        if !target.exists() {
            return ToolOutput::failed(
                "SYMBOLS_PATH_NOT_FOUND",
                crate::missing_path_hint(
                    &target,
                    input.path.as_deref().unwrap_or(""),
                    &ctx.project_root,
                ),
            );
        }
        let crate_dirs = crate_ident_to_dir(&ctx.project_root);
        let files = if let Some(crate_name) = input.crate_name.as_deref() {
            let Some((_, crate_dir)) = crate_dirs.iter().find(|(ident, _)| ident == crate_name)
            else {
                let available = crate_dirs
                    .iter()
                    .map(|(ident, _)| ident.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                return ToolOutput::failed(
                    "SYMBOLS_CRATE_NOT_FOUND",
                    format!(
                        "workspace crate `{crate_name}` not found; available crates: {available}"
                    ),
                );
            };
            collect_rs_files(&crate_dir.join("src"))
        } else {
            collect_rs_files(&target)
        };
        if files.is_empty() {
            return ToolOutput::ok(format!("(no .rs files under {})", target.display()));
        }
        if input.crate_name.is_some() || input.module.is_some() {
            return ToolOutput::ok(render_repo_map(
                &files,
                &crate_dirs,
                &ctx.project_root,
                input.module.as_deref(),
                input.filter.as_deref(),
            ));
        }
        // R-265:define 与 callers 互斥——同时给出是参数错误,而非静默取其一。
        if input.define.is_some() && input.callers.is_some() {
            return ToolOutput::error(
                "symbols: `define` 与 `callers` 互斥,一次只能查一个(定义位置 vs 调用点)。",
            );
        }
        // R-265:符号反查——全树按名精确命中定义点,并解释跨 crate re-export 链。
        if let Some(define) = &input.define {
            let report = resolve_define(&files, define, &ctx.project_root);
            return ToolOutput::ok(report);
        } // R-234 B2:调用链查询——列出对指定符号的引用点。
        if let Some(callers) = &input.callers {
            let mut hits: Vec<String> = Vec::new();
            for file in &files {
                let Ok(text) = std::fs::read_to_string(file) else {
                    continue;
                };
                for (idx, raw) in text.lines().enumerate() {
                    // 匹配引用点,排除「定义行本身」:形如 `fn helper` / `pub fn helper`。
                    let line = raw;
                    let is_definition = {
                        let mut rest = line.trim_start();
                        rest = rest.strip_prefix("pub ").unwrap_or(rest);
                        rest = rest.strip_prefix("pub(crate) ").unwrap_or(rest);
                        rest = rest.strip_prefix("async ").unwrap_or(rest);
                        ["fn ", "struct ", "enum ", "trait "]
                            .iter()
                            .any(|p| rest.starts_with(p))
                            && rest
                                .split_whitespace()
                                .nth(1)
                                .map(|name| {
                                    name.split(['<', '(']).next().unwrap_or(name)
                                        == callers.as_str()
                                })
                                .unwrap_or(false)
                    };
                    if line.contains(callers.as_str()) && !is_definition {
                        hits.push(format!("{}:{}: {}", file.display(), idx + 1, line.trim()));
                    }
                }
            }
            // R-265 验收⑤:输出带上限与「已截断」提示,对齐 grep 的 DEFAULT_LIMIT。
            const DEFAULT_LIMIT: usize = 50;
            let total = hits.len();
            let shown = hits.iter().take(DEFAULT_LIMIT).cloned().collect::<Vec<_>>();
            return if total == 0 {
                ToolOutput::ok(format!("(no callers of `{callers}` found)"))
            } else {
                let mut report = format!("callers of `{callers}` ({} hits):\n", total);
                report.push_str(&shown.join("\n"));
                if total > DEFAULT_LIMIT {
                    report.push_str(&format!(
                        "\n... (stopped at limit {DEFAULT_LIMIT}; narrow the pattern or raise limit)"
                    ));
                }
                ToolOutput::ok(report)
            };
        }
        let mut all: Vec<(String, Vec<Symbol>)> = Vec::new();
        for file in files {
            let symbols = scan_symbols(&file);
            if !symbols.is_empty() {
                all.push((file.display().to_string(), symbols));
            }
        }
        if all.is_empty() {
            return ToolOutput::ok("(no symbols found)".to_string());
        }
        let mut lines: Vec<String> = Vec::new();
        let mut shown = 0usize;
        for (path, symbols) in &all {
            // 表头惰性:该文件零命中就不吐 `== path`。
            // 原先无条件先 push 表头再过滤,目录扫描 + filter 时每个「含任意符号」的
            // 文件都留下一行空表头(全 workspace 约 150 行),把真正的命中埋掉——
            // 这正是「已有 filter 为什么不能当符号反查用」的原因。
            let mut block: Vec<String> = Vec::new();
            for sym in symbols {
                if let Some(filter) = &input.filter {
                    if !sym.name.contains(filter.as_str()) {
                        continue;
                    }
                }
                if input.public_only && !sym.public {
                    continue;
                }
                let vis = if sym.public { "pub" } else { "  " };
                block.push(format!("  {vis} {} {}:{}", sym.kind, sym.name, sym.line));
            }
            if !block.is_empty() {
                lines.push(format!("== {path}"));
                shown += block.len();
                lines.append(&mut block);
            }
        }
        // 判空按**符号行数**,不按 lines.len()==1——后者只在单文件扫描时成立,
        // 目录扫描下永远为假,于是空结果被渲染成一堆表头而不是一句"无命中"。
        if shown == 0 {
            return ToolOutput::ok("(no symbols match filter)".to_string());
        }
        ToolOutput::ok(lines.join("\n"))
    }
}

/// 递归收集目录下 .rs 文件;单文件直接返回。跳过 .kanzei 与 target。
fn collect_rs_files(path: &Path) -> Vec<std::path::PathBuf> {
    if path.is_file() {
        return if path.extension().is_some_and(|e| e == "rs") {
            vec![path.to_path_buf()]
        } else {
            Vec::new()
        };
    }
    let mut out = Vec::new();
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                let name = p.file_name().unwrap_or_default().to_string_lossy();
                if name == ".kanzei" || name == "target" || name == "vendor" {
                    continue;
                }
                stack.push(p);
            } else if p.extension().is_some_and(|e| e == "rs") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// R-310 B3:从当前工作树实时生成 crate → module → public symbol 地图。
/// 不写缓存文件；每次查询重新扫描 Cargo workspace 和 `.rs` 文件，故提交后的增量
/// 变化会立即进入下一次查询，避免维护一份会过期的静态索引。
fn render_repo_map(
    files: &[std::path::PathBuf],
    crate_dirs: &[(String, std::path::PathBuf)],
    project_root: &std::path::Path,
    module_filter: Option<&str>,
    symbol_filter: Option<&str>,
) -> String {
    use std::collections::BTreeMap;

    let mut modules: BTreeMap<(String, String, std::path::PathBuf), Vec<Symbol>> = BTreeMap::new();
    for file in files {
        let Some((crate_name, crate_dir)) = crate_dirs
            .iter()
            .find(|(_, dir)| file.starts_with(dir.join("src")))
        else {
            continue;
        };
        let module = module_path(file, crate_dir);
        if let Some(wanted) = module_filter {
            let matches = module == wanted || module.starts_with(&format!("{wanted}::"));
            if !matches {
                continue;
            }
        }
        let public_symbols = scan_symbols(file)
            .into_iter()
            .filter(|symbol| symbol.public)
            .filter(|symbol| symbol_filter.is_none_or(|filter| symbol.name.contains(filter)))
            .collect::<Vec<_>>();
        if !public_symbols.is_empty() {
            modules.insert((crate_name.clone(), module, file.clone()), public_symbols);
        }
    }
    if modules.is_empty() {
        return "(no public symbols match crate/module filter)".into();
    }

    let crate_count = modules
        .keys()
        .map(|(crate_name, _, _)| crate_name)
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let module_count = modules.len();
    let symbol_count = modules.values().map(Vec::len).sum::<usize>();
    let mut report = format!(
        "repo map (crates: {crate_count}, modules: {module_count}, public_symbols: {symbol_count})\n"
    );
    let mut previous_crate = None;
    for ((crate_name, module, file), symbols) in modules {
        if previous_crate.as_deref() != Some(crate_name.as_str()) {
            report.push_str(&format!("== crate `{crate_name}`\n"));
            previous_crate = Some(crate_name.clone());
        }
        let relative = file.strip_prefix(project_root).unwrap_or(&file);
        report.push_str(&format!("  module `{module}` ({})\n", relative.display()));
        for symbol in symbols {
            report.push_str(&format!(
                "    pub {} {}:{}\n",
                symbol.kind, symbol.name, symbol.line
            ));
        }
    }
    report
}

fn module_path(file: &std::path::Path, crate_dir: &std::path::Path) -> String {
    let src = crate_dir.join("src");
    let Ok(relative) = file.strip_prefix(src) else {
        return "crate".into();
    };
    let mut parts = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let Some(last) = parts.last_mut() else {
        return "crate".into();
    };
    if last == "lib.rs" || last == "main.rs" {
        return "crate".into();
    }
    if last == "mod.rs" {
        parts.pop();
    } else if let Some(stem) = last.strip_suffix(".rs") {
        *last = stem.to_string();
    }
    if parts.is_empty() {
        "crate".into()
    } else {
        parts.join("::")
    }
}

/// R-265:符号反查。`define` 输入裸名或限定路径(crate::mod::sym),全树按
/// **符号名精确命中**定义点(路径只参与输出解释,不参与命中判定——事故成因正是
/// 按路径字面解析扑空),再解释跨 crate re-export 链。
///
/// 返回逐行报告:命中定义位置 + 再导出链。未命中给出明确「未找到」。
fn resolve_define(
    files: &[std::path::PathBuf],
    define: &str,
    project_root: &std::path::Path,
) -> String {
    // 裸名或限定路径,一律取末段作为符号名。
    let symbol = define.split("::").last().unwrap_or(define).trim();
    let mut hits: Vec<(std::path::PathBuf, usize, String, bool)> = Vec::new();
    for file in files {
        for sym in scan_symbols(file) {
            if sym.name == symbol {
                hits.push((file.clone(), sym.line, sym.kind.to_string(), sym.public));
            }
        }
    }
    let reexports = collect_reexports(files);
    let mut out = String::new();
    // 未直接命中:可能查询的是 as 改名后的**新名**(定义名不叫这个)。
    // 回落:找 exported == symbol 且带 source_name 的 re-export,用原名重查定义。
    if hits.is_empty() {
        let mut fallback_name: Option<String> = None;
        let mut fallback_chain: Vec<String> = Vec::new();
        for re in &reexports {
            if re.exported == symbol {
                fallback_chain.push(format!(
                    "  {}:{}  {}",
                    re.file.display(),
                    re.line,
                    re.source_line.trim()
                ));
                if let Some(orig) = &re.source_name {
                    fallback_name = Some(orig.clone());
                }
            }
        }
        if let Some(orig) = &fallback_name {
            for file in files {
                for sym in scan_symbols(file) {
                    if sym.name == orig.as_str() {
                        hits.push((file.clone(), sym.line, sym.kind.to_string(), sym.public));
                    }
                }
            }
            if !hits.is_empty() {
                out.push_str(&format!(
                    "`{symbol}` is a re-export alias of `{orig}` (as-renamed):\n"
                ));
                out.push_str(&format!("re-export chain:\n{}", fallback_chain.join("\n")));
                out.push('\n');
            }
        }
        if hits.is_empty() {
            out.push_str(&format!(
                "(no definition of `{symbol}` found in {} files; it may be a re-export alias — check `pub use` chains)\n",
                files.len()
            ));
            if !fallback_chain.is_empty() {
                out.push_str(&format!(
                    "re-export chain for `{symbol}`:\n{}",
                    fallback_chain.join("\n")
                ));
            }
            return out;
        }
    }
    // R-265:限定路径解释——`crate::atomic_file::try_lock_exclusive` 等带 crate
    // 前缀时,附该 crate 的源码目录提示(不参与命中判定,只回答「这个 crate 在哪」)。
    if define.contains("::") {
        let prefix = define.split("::").next().unwrap_or("").trim();
        if !prefix.is_empty() && prefix != "crate" && prefix != "self" && prefix != "super" {
            let crate_map = crate_ident_to_dir(project_root);
            for (ident, dir) in &crate_map {
                if ident == prefix {
                    out.push_str(&format!(
                        "crate `{prefix}` 源码目录: {}\n",
                        dir.strip_prefix(project_root).unwrap_or(dir).display()
                    ));
                    break;
                }
            }
        }
    }
    out.push_str(&format!("definition of `{symbol}` ({} hit):\n", hits.len()));
    for (file, line, kind, public) in &hits {
        let vis = if *public { "pub" } else { "  " };
        let rel = file.strip_prefix(project_root).unwrap_or(file);
        out.push_str(&format!(
            "  {vis} {kind} {symbol}  {}:{line}\n",
            rel.display()
        ));
    }
    // 再导出链:两型都算——①符号直连(exported == symbol:as 新名/花括号列表项);
    // ②模块整体(exported == 宿主模块名,该模块内符号经此链可见)。
    let mut chain: Vec<String> = Vec::new();
    for re in &reexports {
        if re.exported == symbol {
            chain.push(format!(
                "  {}:{}  {}",
                re.file.display(),
                re.line,
                re.source_line.trim()
            ));
            continue;
        }
        // 模块整体型:命中定义文件的宿主模块名(stem)与导出名一致。
        for (file, _, _, _) in &hits {
            let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned());
            if stem.as_deref() == Some(re.exported.as_str()) {
                chain.push(format!(
                    "  {}:{}  {}",
                    re.file.display(),
                    re.line,
                    re.source_line.trim()
                ));
                break;
            }
        }
    }
    chain.sort();
    chain.dedup();
    if !chain.is_empty() {
        out.push_str(&format!(
            "re-export chain (how `{symbol}` reaches other crates):\n{}",
            chain.join("\n")
        ));
    } else {
        out.push_str("(no `pub use` re-export of this symbol found in tree)\n");
    }
    out
}

/// R-265:crate ident → 源码目录映射。读 workspace members 的 `[package].name`,
/// `-` → `_`(crate ident 约定)。供限定路径(`crate::module::sym`)解释时
/// 定位「这个 crate 的源码在哪」,输出目录提示。
fn crate_ident_to_dir(project_root: &std::path::Path) -> Vec<(String, std::path::PathBuf)> {
    let mut out = Vec::new();
    let Ok(workspace_toml) = std::fs::read_to_string(project_root.join("Cargo.toml")) else {
        return out;
    };
    // 提取 [workspace] members = [...] 里的 crate 相对目录(形如 crates/kanzei-base)。
    let mut members: Vec<String> = Vec::new();
    let mut in_workspace = false;
    let mut in_members = false;
    for line in workspace_toml.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_workspace = t.starts_with("[workspace]");
            in_members = false;
            continue;
        }
        if !in_workspace {
            continue;
        }
        if let Some(rest) = t
            .strip_prefix("members")
            .and_then(|rest| rest.trim_start().strip_prefix('='))
        {
            // 合法 TOML 既可能把 members 写成单行数组，也可能写成多行数组。
            // 单行数组不能只依赖下面的「每行一个引号项」分支，否则 crate 查询会
            // 静默得到空 workspace。
            let mut remaining = rest;
            while let Some(start) = remaining.find('"') {
                let after_start = &remaining[start + 1..];
                let Some(end) = after_start.find('"') else {
                    break;
                };
                let member = &after_start[..end];
                if !member.is_empty() {
                    members.push(member.to_string());
                }
                remaining = &after_start[end + 1..];
            }
            in_members = !rest.contains(']');
            continue;
        }
        if !in_members {
            continue;
        }
        if t.starts_with('"') {
            let name = t.trim_end_matches(',').trim_matches('"');
            if !name.is_empty() {
                members.push(name.to_string());
            }
        }
        if t == "]" {
            in_members = false;
        }
    }
    for member in members {
        let manifest = project_root.join(&member).join("Cargo.toml");
        let Ok(text) = std::fs::read_to_string(&manifest) else {
            continue;
        };
        for line in text.lines() {
            let t = line.trim();
            if let Some(rest) = t.strip_prefix("name = ") {
                let name = rest.trim().trim_matches('"').to_string();
                let ident = name.replace('-', "_");
                out.push((ident, project_root.join(&member)));
                break;
            }
        }
    }
    out
}

/// 一条 `pub use` 再导出记录(三种形态统一:模块整体 / as 改名 / 花括号列表)。
struct ReExport {
    file: std::path::PathBuf,
    line: usize,
    /// 导出后的可见名:模块整体 = 模块名,as 改名 = 新名,花括号 = 列表项。
    exported: String,
    /// 源路径末段:as 改名的原名(如 `kill_process`),供 hits 为空时回落重查。
    source_name: Option<String>,
    /// 该 pub use 的原始行(供报告展示)。
    source_line: String,
}

/// 全树收集 `pub use` 再导出记录。行级扫描 + 跨行花括号列表合并;
/// 不引入 syn(与 R-154 轻量哲学一致)。
fn collect_reexports(files: &[std::path::PathBuf]) -> Vec<ReExport> {
    let mut out = Vec::new();
    for file in files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let mut lines = text.lines().enumerate();
        // 跨行状态:正在积累 `pub use x::{ a, b, ... }` 列表。
        let mut pending: Option<(usize, String, Vec<String>)> = None;
        for (idx, raw) in lines.by_ref() {
            let line = raw.trim();
            // 跨行列表继续:追加列表项直到 `};`。
            if let Some((start, prefix, mut items)) = pending.take() {
                for part in line.split(',') {
                    let part = part.trim().trim_end_matches(';');
                    if !part.is_empty() {
                        items.push(part.to_string());
                    }
                }
                if line.contains('}') {
                    for item in items {
                        out.push(ReExport {
                            file: file.clone(),
                            line: start + 1,
                            exported: item.clone(),
                            source_name: None,
                            source_line: format!(
                                "pub use {}::{{ ... }} (跨行列表,首行 {})",
                                prefix,
                                start + 1
                            ),
                        });
                    }
                } else {
                    pending = Some((start, prefix, items));
                }
                continue;
            }
            if !line.starts_with("pub use ") {
                continue;
            }
            let rest = &line["pub use ".len()..];
            // 花括号列表跨行:`pub use path::{ a, b,` 还没闭合 → 挂起。
            if rest.contains("::{") && !rest.contains('}') {
                let prefix = rest.split("::{").next().unwrap_or("").trim().to_string();
                let tail = rest.split("::{").nth(1).unwrap_or("").to_string();
                let mut items: Vec<String> = Vec::new();
                for part in tail.split(',') {
                    let part = part.trim().trim_end_matches(';');
                    if !part.is_empty() {
                        items.push(part.to_string());
                    }
                }
                pending = Some((idx, prefix, items));
                continue;
            }
            // as 改名:`pub use path::orig as new;`
            if let Some((orig, new)) = rest.split_once(" as ") {
                let orig_name = orig
                    .split("::")
                    .last()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
                out.push(ReExport {
                    file: file.clone(),
                    line: idx + 1,
                    exported: new.trim().trim_end_matches(';').to_string(),
                    source_name: orig_name,
                    source_line: line.to_string(),
                });
                continue;
            }
            // 单行花括号列表:`pub use path::{ a, b };`
            if let Some((_prefix, list)) = rest.split_once("::{") {
                let list = list.trim_end_matches('}').trim_end_matches(';');
                for item in list.split(',') {
                    let item = item.trim();
                    if !item.is_empty() {
                        out.push(ReExport {
                            file: file.clone(),
                            line: idx + 1,
                            exported: item.to_string(),
                            source_name: None,
                            source_line: line.to_string(),
                        });
                    }
                }
                continue;
            }
            // 模块整体:`pub use path::module;`
            let exported = rest
                .split("::")
                .last()
                .unwrap_or(rest)
                .trim_end_matches(';')
                .trim()
                .to_string();
            if !exported.is_empty() {
                out.push(ReExport {
                    file: file.clone(),
                    line: idx + 1,
                    exported,
                    source_name: None,
                    source_line: line.to_string(),
                });
            }
        }
    }
    out
}

/// 行级扫描符号。状态机跳过字符串/注释内的伪命中。
fn scan_symbols(file: &Path) -> Vec<Symbol> {
    let Ok(text) = std::fs::read_to_string(file) else {
        return Vec::new();
    };
    let mut symbols = Vec::new();
    let mut in_block_comment = false;
    for (idx, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        // 块注释状态:/* ... */ 跨行。
        let mut rest = line;
        loop {
            if in_block_comment {
                if let Some(end) = rest.find("*/") {
                    in_block_comment = false;
                    rest = &rest[end + 2..];
                    continue;
                }
                break;
            }
            if let Some(start) = rest.find("/*") {
                // 行注释优先:/* 出现在 // 之后则整行是注释。
                let line_comment = rest.find("//").unwrap_or(usize::MAX);
                if start < line_comment {
                    in_block_comment = true;
                    rest = &rest[start + 2..];
                    continue;
                }
                break;
            }
            break;
        }
        if in_block_comment {
            continue;
        }
        // 去掉行注释与字符串字面量(粗略:遇到 // 或 " 截断;字符串内引号场景稀少,可接受)。
        let code = strip_line_tail(rest);
        let code = code.trim();
        if code.is_empty() {
            continue;
        }
        if let Some(sym) = parse_symbol_line(code, idx + 1) {
            symbols.push(sym);
        }
    }
    symbols
}

/// 去掉行注释 `// ...`(不处理字符串里的 //——行级工具可接受的近似)。
fn strip_line_tail(line: &str) -> &str {
    match line.find("//") {
        Some(i) => &line[..i],
        None => line,
    }
}

/// 从单行代码解析一个符号定义。
fn parse_symbol_line(code: &str, line: usize) -> Option<Symbol> {
    let code = code.trim();
    let public = code.starts_with("pub ");
    let body = if public {
        code.trim_start_matches("pub ")
    } else {
        code
    };
    let body = body
        .trim_start_matches("pub(crate) ")
        .trim_start_matches("pub(super) ")
        // R-265:async fn 也识别——`pub async fn kill_process` 剥掉 async 前缀,
        // 否则 as 改名回落的原名找不到定义(验收② real: kill_process 是 async fn)。
        .trim_start_matches("async ");
    // 顺序:impl(可带泛型) > fn > struct > enum > trait > type > const/static。
    if let Some(rest) = body.strip_prefix("impl") {
        let after = rest.trim_start();
        if after.is_empty() {
            return None; // 裸 impl 块起始,不列(块内 fn 单独列)。
        }
        // 提取 impl 主体名:impl Foo for Bar → "Foo for Bar";
        // impl<T> Vec<T> → 跳过泛型参数取 Vec。
        let head = after.split(['{', ';']).next().unwrap_or(after).trim();
        let head = if head.starts_with('<') {
            // 跳过 <...> 泛型段。
            head.split_once('>')
                .map(|(_, after_gen)| after_gen.trim_start())
                .unwrap_or(head)
        } else {
            head
        };
        let head = head.trim_start_matches("dyn ");
        if head.is_empty() {
            return None;
        }
        let name = head
            .split_whitespace()
            .filter(|t| *t != "for")
            .map(|t| t.split('<').next().unwrap_or(t)) // 去泛型参数 Vec<T> → Vec
            .collect::<Vec<_>>()
            .join(" ");
        return Some(Symbol {
            kind: "impl",
            name,
            line,
            public,
        });
    }
    for kw in ["fn", "struct", "enum", "trait", "type", "mod"] {
        if let Some(rest) = body.strip_prefix(kw) {
            // 真·词边界:剥掉关键字后必须**紧跟空白**。
            //
            // 原先只检查 trim 之后的首字符是不是字母——那是词边界的反面:
            // `typed_writer.lock()` 剥掉 `type` 剩 `d_writer.lock()`,首字符 `d`
            // 是字母于是照过,产出假符号 `type d_writer.lock`。实测假阳性:
            // crates/kanzei/src/main.rs:522/:573、crates/kanzei-app/src/fast_model.rs:209
            // (`models.iter()` → `mod els.iter`)。列表模式下这只是噪声,一旦拿
            // symbols 当「符号定义在哪」用,它就是直接给出错误答案。
            if !rest.starts_with(char::is_whitespace) {
                continue;
            }
            let after = rest.trim_start();
            if after.is_empty()
                || !after
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphabetic() || c == '_')
            {
                continue;
            }
            let name = after
                .split(['<', '(', ';', '{', '='])
                .next()
                .unwrap_or(after)
                .trim()
                .to_string();
            if name.is_empty() {
                continue;
            }
            return Some(Symbol {
                kind: kw,
                name,
                line,
                public,
            });
        }
    }
    // const/static NAME: 带类型标注的。
    for kw in ["const", "static"] {
        if let Some(rest) = body.strip_prefix(kw) {
            // 同上的词边界。这里原先连空白检查都没有:`constant_foo: X` 会被剥成
            // 符号 `ant_foo`,任何以 const/static 开头的标识符都中招。
            if !rest.starts_with(char::is_whitespace) {
                continue;
            }
            let after = rest.trim_start();
            let name = after.split(':').next().unwrap_or(after).trim().to_string();
            if !name.is_empty()
                && name
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphabetic() || c == '_')
            {
                return Some(Symbol {
                    kind: kw,
                    name,
                    line,
                    public,
                });
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(content: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-symbols-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("sample.rs");
        std::fs::write(&file, content).unwrap();
        file
    }

    #[tokio::test]
    async fn 不存在路径返回同目录最近邻候选() {
        let dir = std::env::temp_dir().join(format!(
            "kz-symbols-path-hint-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("coordinator.rs"), "fn coordinator() {}\n").unwrap();
        let ctx = kanzei_harness::ToolCtx::new(dir.clone(), dir.clone());
        let out = SymbolsTool
            .execute(serde_json::json!({"path": "coordinatr.rs"}), &ctx)
            .await;
        assert!(out.is_error, "{}", out.content);
        assert_eq!(out.code, Some("SYMBOLS_PATH_NOT_FOUND"));
        assert!(out.content.contains("coordinator.rs"), "{}", out.content);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 符号扫描_识别函数结构impl与可见性() {
        let file = fixture(
            "pub fn public_fn() {}\n\
             fn private_fn() {}\n\
             pub struct Config {}\n\
             enum Mode { A, B }\n\
             impl Config {\n\
                 pub fn method(&self) {}\n\
                 fn hidden(&self) {}\n\
             }\n\
             trait Runnable { fn run(&self); }\n\
             // fn commented_out() {}\n\
             let _s = \"fn not_a_fn()\";\n",
        );
        let symbols = scan_symbols(&file);
        let names: Vec<(&str, &str, bool)> = symbols
            .iter()
            .map(|s| (s.kind, s.name.as_str(), s.public))
            .collect();
        assert!(names.contains(&("fn", "public_fn", true)), "{names:?}");
        assert!(names.contains(&("fn", "private_fn", false)), "{names:?}");
        assert!(names.contains(&("struct", "Config", true)), "{names:?}");
        assert!(names.contains(&("enum", "Mode", false)), "{names:?}");
        assert!(names.contains(&("impl", "Config", false)), "{names:?}");
        assert!(names.contains(&("fn", "method", true)), "{names:?}");
        assert!(names.contains(&("fn", "hidden", false)), "{names:?}");
        assert!(names.contains(&("trait", "Runnable", false)), "{names:?}");
        // 注释与字符串内的伪 fn 不得命中。
        assert!(
            !names.contains(&("fn", "commented_out", false)),
            "{names:?}"
        );
        assert!(!names.contains(&("fn", "not_a_fn", false)), "{names:?}");
        std::fs::remove_file(&file).ok();
    }

    /// 关键字必须按词边界匹配:以关键字为**前缀**的标识符不是符号声明。
    ///
    /// 这三行都是从仓里真实抄来的假阳性来源:
    /// - `typed_writer.lock()`  crates/kanzei/src/main.rs:522(剥 `type` → `d_writer.lock`)
    /// - `models.iter()`        crates/kanzei-app/src/fast_model.rs:209(剥 `mod` → `els.iter`)
    /// - `constant_foo`         const/static 分支原先连空白都不检查
    ///
    /// 列表模式下这些只是噪声;拿 symbols 回答「符号定义在哪」时,它们就是错答案。
    #[test]
    fn 符号扫描_关键字前缀的标识符不得误判为声明() {
        let file = fixture(
            "typed_writer.lock().unwrap().record_error(error);\n\
             models.iter().any(|m| m.id == want);\n\
             constant_foo: i32 = 3;\n\
             structural.rebuild();\n\
             // 真声明必须仍然命中:\n\
             type Alias = u32;\n\
             mod inner;\n\
             const REAL: i32 = 1;\n\
             struct Real {}\n",
        );
        let symbols = scan_symbols(&file);
        let names: Vec<(&str, &str)> = symbols.iter().map(|s| (s.kind, s.name.as_str())).collect();
        for bogus in ["d_writer.lock", "els.iter", "ant_foo", "ural.rebuild"] {
            assert!(
                !names.iter().any(|(_, n)| *n == bogus),
                "关键字前缀被当成声明剥出了假符号 {bogus}: {names:?}"
            );
        }
        assert!(names.contains(&("type", "Alias")), "{names:?}");
        assert!(names.contains(&("mod", "inner")), "{names:?}");
        assert!(names.contains(&("const", "REAL")), "{names:?}");
        assert!(names.contains(&("struct", "Real")), "{names:?}");
        std::fs::remove_file(&file).ok();
    }

    #[test]
    fn 符号扫描_处理泛型与pubcrate() {
        let file = fixture(
            "pub(crate) fn helper<T>(x: T) -> T { x }\n\
             impl<T> Vec<T> {\n  fn push_inner(&mut self) {}\n}\n\
             pub const MAX: usize = 100;\n",
        );
        let symbols = scan_symbols(&file);
        let names: Vec<(&str, &str)> = symbols.iter().map(|s| (s.kind, s.name.as_str())).collect();
        assert!(names.contains(&("fn", "helper",)), "{names:?}");
        assert!(names.contains(&("impl", "Vec",)), "{names:?}");
        assert!(names.contains(&("fn", "push_inner",)), "{names:?}");
        assert!(names.contains(&("const", "MAX",)), "{names:?}");
        std::fs::remove_file(&file).ok();
    }

    #[test]
    fn 符号扫描_块注释跨行不误伤() {
        let file = fixture(
            "/*\n  fn inside_block() {}\n*/\n\
             fn real() {}\n",
        );
        let symbols = scan_symbols(&file);
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"real"), "{names:?}");
        assert!(!names.contains(&"inside_block"), "{names:?}");
        std::fs::remove_file(&file).ok();
    }

    /// R-234 B2:调用链查询——callers 列出引用点,排除定义行本身。
    #[tokio::test]
    async fn 调用链查询_列出引用点排除定义行() {
        let dir = std::env::temp_dir().join(format!(
            "kz-symbols-callers-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("lib.rs"),
            "pub fn helper() {}\n\
             fn caller_a() { helper(); }\n\
             fn caller_b() { helper(); helper(); }\n",
        )
        .unwrap();
        let ctx = kanzei_harness::ToolCtx::new(dir.clone(), dir.clone());
        let tool = SymbolsTool;
        let out = tool
            .execute(serde_json::json!({"path": ".", "callers": "helper"}), &ctx)
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("caller_a"), "{}", out.content);
        assert!(out.content.contains("caller_b"), "{}", out.content);
        // 定义行 `pub fn helper()` 不应被算作调用点(那是定义不是调用)。
        assert!(!out.content.contains("pub fn helper"), "{}", out.content);
        assert!(out.content.contains("2 hits"), "{}", out.content);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// R-265 验收④:define 与 callers 同时给出时显式报错,而非静默取其一。
    #[tokio::test]
    async fn define与callers互斥_显式报错() {
        let dir = std::env::temp_dir().join(format!(
            "kz-symbols-excl-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("lib.rs"), "pub fn helper() {}\n").unwrap();
        let ctx = kanzei_harness::ToolCtx::new(dir.clone(), dir.clone());
        let tool = SymbolsTool;
        let out = tool
            .execute(
                serde_json::json!({"path": ".", "define": "helper", "callers": "helper"}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "互斥必须显式报错, {}", out.content);
        assert!(out.content.contains("互斥"), "{}", out.content);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// R-265 验收①/②/③:resolve_define 按名命中定义,三型 re-export 都进链
    /// (模块整体 / as 改名 / 跨行花括号列表)。
    #[test]
    fn define_命中定义点并收集三型再导出链() {
        let dir = std::env::temp_dir().join(format!(
            "kz-symbols-define-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("lib.rs"),
            "pub use kanzei_base::atomic_file;\n\
             pub use background::kill_process as kill_background_processes_for_process;\n\
             pub use scheduling::{\n\
                 append_progress, backlog_status,\n\
                 workable_titles,\n\
             };\n",
        )
        .unwrap();
        let files = vec![dir.join("lib.rs")];
        // ①模块整体:define=atomic_file 命中 re-export 链(模块名即导出名)。
        let report = resolve_define(&files, "atomic_file", &dir);
        assert!(report.contains("atomic_file"), "{}", report);
        // ②as 改名:define=kill_background_processes_for_process 命中链。
        let report2 = resolve_define(&files, "kill_background_processes_for_process", &dir);
        assert!(
            report2.contains("kill_background_processes_for_process"),
            "{}",
            report2
        );
        // ③跨行花括号:define=workable_titles 命中链(跨行列表合并不丢项)。
        let report3 = resolve_define(&files, "workable_titles", &dir);
        assert!(report3.contains("workable_titles"), "{}", report3);
        let report4 = resolve_define(&files, "backlog_status", &dir);
        assert!(report4.contains("backlog_status"), "{}", report4);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// R-265 验收⑤:callers 输出带上限与「已截断」提示(对齐 grep DEFAULT_LIMIT)。
    #[tokio::test]
    async fn callers_超过上限给出截断提示() {
        let dir = std::env::temp_dir().join(format!(
            "kz-symbols-limit-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut src = String::from("pub fn helper() {}\n");
        for i in 0..80 {
            src.push_str(&format!("fn caller_{i}() {{ helper(); }}\n"));
        }
        std::fs::write(dir.join("lib.rs"), src).unwrap();
        let ctx = kanzei_harness::ToolCtx::new(dir.clone(), dir.clone());
        let tool = SymbolsTool;
        let out = tool
            .execute(serde_json::json!({"path": ".", "callers": "helper"}), &ctx)
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("80 hits"), "{}", out.content);
        assert!(
            out.content.contains("stopped at limit 50"),
            "{}",
            out.content
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// R-265:crate ident → 源码目录映射(`-`→`_`)。
    /// R-310 B3:分层查询只输出 public symbol,并且第二次查询反映当前文件内容，
    /// 证明它是机器实时生成而不是提交前一次性写死的索引。
    #[tokio::test]
    async fn repo_map_按crate与module查询并随当前文件增量更新() {
        let dir = std::env::temp_dir().join(format!(
            "kz-symbols-repo-map-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let crate_dir = dir.join("crates/demo-crate");
        std::fs::create_dir_all(crate_dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/demo-crate\"]\n",
        )
        .unwrap();
        std::fs::write(
            crate_dir.join("Cargo.toml"),
            "[package]\nname = \"demo-crate\"\n",
        )
        .unwrap();
        let source = crate_dir.join("src/lib.rs");
        std::fs::write(&source, "pub fn first() {}\nfn hidden() {}\n").unwrap();
        let ctx = kanzei_harness::ToolCtx::new(dir.clone(), dir.clone());
        let tool = SymbolsTool;
        let first = tool
            .execute(
                serde_json::json!({
                    "crate": "demo_crate",
                    "module": "crate"
                }),
                &ctx,
            )
            .await;
        assert!(!first.is_error, "{}", first.content);
        assert!(
            first.content.contains("crate `demo_crate`"),
            "{}",
            first.content
        );
        assert!(
            first.content.contains("module `crate`"),
            "{}",
            first.content
        );
        assert!(first.content.contains("pub fn first"), "{}", first.content);
        assert!(!first.content.contains("hidden"), "{}", first.content);

        std::fs::write(
            &source,
            "pub fn first() {}\npub fn second() {}\nfn hidden() {}\n",
        )
        .unwrap();
        let second = tool
            .execute(
                serde_json::json!({
                    "crate": "demo_crate",
                    "module": "crate"
                }),
                &ctx,
            )
            .await;
        assert!(!second.is_error, "{}", second.content);
        assert!(
            second.content.contains("pub fn second"),
            "{}",
            second.content
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn crate_ident_to_dir_映射_下划线ident到目录() {
        let dir = std::env::temp_dir().join(format!(
            "kz-symbols-crates-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("crates/kanzei-base")).unwrap();
        std::fs::create_dir_all(dir.join("crates/kanzei-tools")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nmembers = [\n    \"crates/kanzei-base\",\n    \"crates/kanzei-tools\",\n]\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("crates/kanzei-base/Cargo.toml"),
            "[package]\nname = \"kanzei-base\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("crates/kanzei-tools/Cargo.toml"),
            "[package]\nname = \"kanzei-tools\"\n",
        )
        .unwrap();
        let map = crate_ident_to_dir(&dir);
        let base = map
            .iter()
            .find(|(ident, _)| ident == "kanzei_base")
            .map(|(_, d)| d.clone());
        assert!(
            base.is_some() && base.unwrap().ends_with("crates/kanzei-base"),
            "{map:?}"
        );
        let tools = map
            .iter()
            .find(|(ident, _)| ident == "kanzei_tools")
            .map(|(_, d)| d.clone());
        assert!(
            tools.is_some() && tools.unwrap().ends_with("crates/kanzei-tools"),
            "{map:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// R-265:限定路径 define 输出 crate 源码目录提示。
    #[test]
    fn define_限定路径_输出crate目录提示() {
        let dir = std::env::temp_dir().join(format!(
            "kz-symbols-cratepath-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("crates/kanzei-tools/src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nmembers = [\n    \"crates/kanzei-tools\",\n]\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("crates/kanzei-tools/Cargo.toml"),
            "[package]\nname = \"kanzei-tools\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("crates/kanzei-tools/src/lib.rs"),
            "pub fn helper() {}\n",
        )
        .unwrap();
        let files = collect_rs_files(&dir.join("crates"));
        let report = resolve_define(&files, "kanzei_tools::helper", &dir);
        assert!(report.contains("kanzei_tools"), "{}", report);
        assert!(report.contains("crates/kanzei-tools"), "{}", report);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// R-265 验收①/②:对**真实仓库**穿透跨 crate re-export——try_lock_exclusive
    /// 定义在 kanzei-base,经 kanzei-tools lib.rs 再导出;as 改名回落原名。
    #[tokio::test]
    async fn define_真实仓库穿透跨crate再导出() {
        // CARGO_MANIFEST_DIR = crates/kanzei-tools;上两级 = 仓库根。
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("repo root");
        let crates = repo.join("crates");
        if !crates
            .join("kanzei-base")
            .join("src")
            .join("atomic_file.rs")
            .exists()
        {
            return; // 非本仓库环境(如打包后),跳过真实文件断言。
        }
        let files = collect_rs_files(&crates);
        // ①验收①:define=try_lock_exclusive 命中 kanzei-base/src/atomic_file.rs。
        let report = resolve_define(&files, "try_lock_exclusive", repo);
        assert!(
            report.contains("atomic_file.rs") && report.contains("try_lock_exclusive"),
            "{}",
            report
        );
        // 再导出链:kanzei-tools/src/lib.rs 的 `pub use kanzei_base::atomic_file;`
        // 让 try_lock_exclusive 在 kanzei-tools crate 可见——模块整体型链。
        assert!(
            report.contains("re-export chain") && report.contains("kanzei-tools"),
            "{}",
            report
        );
        // ②验收②:as 改名——define=kill_background_processes_for_process(新名,
        // 定义名 kill_process 不叫这个)必须回落原名命中 background/lifecycle.rs 定义。
        let report2 = resolve_define(&files, "kill_background_processes_for_process", repo);
        assert!(
            report2.contains("lifecycle.rs")
                && report2.contains("kill_background_processes_for_process"),
            "{}",
            report2
        );
        // ③验收③:跨行花括号再导出——define=workable_titles(tracker.rs:25-29
        // 的 `pub use scheduling::{ ... }` 列表项)必须命中 scheduling.rs 定义。
        let report3 = resolve_define(&files, "workable_titles", repo);
        assert!(
            report3.contains("scheduling.rs") && report3.contains("workable_titles"),
            "{}",
            report3
        );
    }
}
