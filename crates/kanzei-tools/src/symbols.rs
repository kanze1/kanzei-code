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
         cwd), filter (substring match on symbol name), public_only."
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
            return ToolOutput::error(format!("path not found: {}", target.display()));
        }
        let files = collect_rs_files(&target);
        if files.is_empty() {
            return ToolOutput::ok(format!("(no .rs files under {})", target.display()));
        }
        // R-234 B2:调用链查询——列出对指定符号的引用点。
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
            return if hits.is_empty() {
                ToolOutput::ok(format!("(no callers of `{callers}` found)"))
            } else {
                ToolOutput::ok(format!(
                    "callers of `{callers}` ({} hits):\n{}",
                    hits.len(),
                    hits.join("\n")
                ))
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
                let line_comment = rest.find("//").map_or(usize::MAX, |i| i);
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
        .trim_start_matches("pub(super) ");
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
        let names: Vec<(&str, &str)> = symbols
            .iter()
            .map(|s| (s.kind, s.name.as_str()))
            .collect();
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
}
