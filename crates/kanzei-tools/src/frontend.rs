//! 前端专用定位工具(R-126)。
//!
//! agent 改前端时的两个具体痛处:①编辑锚点撞车——c65c80e 把 `@media (max-width: 700px) {`
//! 整行替换成了别的规则,CSS 结构静默损坏(D-164),浏览器对花括号错配是容错的,没有任何
//! 报错;②改一个 class 时不知道它在哪几处定义,漏改一处就出现"只在某些状态下才错"。
//!
//! 这三个工具都是只读的:定位与检查,不改文件。

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

/// 一个 CSS 规则块的位置。
#[derive(Debug, Clone, PartialEq)]
pub struct RuleSite {
    pub line: usize,
    pub selector: String,
    /// 所在的 @media / @supports 等条件块(顶层为空)。
    pub context: String,
}

/// 扫描 CSS 文本,返回每个含 `needle` 的选择器所在位置。
/// needle 为空则返回全部规则。按行扫描而不是解析 AST:够用、零依赖,
/// 而且行号正是 agent 定位要的东西。
pub fn find_rule_sites(css: &str, needle: &str) -> Vec<RuleSite> {
    let mut sites = Vec::new();
    // 条件块栈:(名字, 它内部内容所在的花括号深度)。记深度而不是"见到 } 就弹一层"
    // 是 D-197 的根因所在——旧写法只在"整行没有新开块"时弹一次,于是两头都错:
    // `@media { .a {\n…\n} }` 里 `.a` 的收尾 } 会把 @media 提前弹掉(块内后续规则
    // 被报成顶层);而单行写完的 `@keyframes x { … }` 永远等不到那一次弹栈,
    // 于是它**之后**的顶层规则全被报成在这个块里。实测本仓库 style.css 576 条规则
    // 里 15 条 context 是错的,两种形态都有。
    let mut stack: Vec<(String, usize)> = Vec::new();
    let mut depth = 0usize;
    for (index, raw) in css.lines().enumerate() {
        let line = strip_line_comment(raw).trim().to_string();
        if line.is_empty() {
            continue;
        }
        // 一行里可能既开又闭(`.a { color: red; }`),按计数净变化算深度。
        let opens = line.matches('{').count();
        let closes = line.matches('}').count();
        let conditional = line.starts_with('@') && opens > 0;
        if conditional {
            stack.push((line.trim_end_matches('{').trim().to_string(), depth + opens));
        } else if opens > 0 {
            // context 取"这个选择器开括号那一刻"的栈,所以记录早于本行深度更新。
            let selector = line.split('{').next().unwrap_or("").trim().to_string();
            if !selector.is_empty() && (needle.is_empty() || selector.contains(needle)) {
                sites.push(RuleSite {
                    line: index + 1,
                    selector,
                    context: stack
                        .iter()
                        .map(|(name, _)| name.as_str())
                        .collect::<Vec<_>>()
                        .join(" > "),
                });
            }
        }
        depth = (depth + opens).saturating_sub(closes);
        // 深度退回到某个条件块之外,它就结束了——单行写完的块在本行当场出栈。
        while stack.last().is_some_and(|(_, inner)| *inner > depth) {
            stack.pop();
        }
    }
    sites
}

fn strip_line_comment(line: &str) -> &str {
    match line.find("/*") {
        Some(index) if !line[..index].contains('"') => &line[..index],
        _ => line,
    }
}

/// CSS 结构完整性:花括号配对。浏览器对错配是静默容错的,一个被吃掉的
/// `@media ... {` 会让整段规则无条件生效而没有任何报错(D-164 就这样上的线)。
pub fn css_structure_issues(css: &str) -> Vec<String> {
    let without_comments = strip_block_comments(css);
    let mut depth = 0usize;
    let mut stray = 0usize;
    let mut stray_lines: Vec<usize> = Vec::new();
    for (index, line) in without_comments.lines().enumerate() {
        for ch in line.chars() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    if depth == 0 {
                        stray += 1;
                        if stray_lines.len() < 5 {
                            stray_lines.push(index + 1);
                        }
                    } else {
                        depth -= 1;
                    }
                }
                _ => {}
            }
        }
    }
    let mut issues = Vec::new();
    if stray > 0 {
        issues.push(format!(
            "{stray} 个多余的 `}}`(行 {});很可能某条规则或 @media 的开括号被覆盖删除了",
            stray_lines
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if depth > 0 {
        issues.push(format!("{depth} 个未闭合的 `{{`"));
    }
    issues
}

fn strip_block_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut in_comment = false;
    while i < bytes.len() {
        if !in_comment && bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
            in_comment = true;
            i += 2;
            continue;
        }
        if in_comment && bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
            in_comment = false;
            i += 2;
            continue;
        }
        if !in_comment {
            out.push(bytes[i] as char);
        } else if bytes[i] == b'\n' {
            // 注释里的换行要留住,否则后面的行号全错。
            out.push('\n');
        }
        i += 1;
    }
    out
}

#[derive(Deserialize, JsonSchema)]
struct FrontendLocateInput {
    /// 要定位的 class / id / 选择器片段,例如 `.doc-edit` 或 `bg-entry`。
    selector: String,
    /// CSS 文件路径(相对项目根)。默认 crates/kanzei-app/ui/style.css。
    #[serde(default)]
    css_path: Option<String>,
}

/// 列出某个选择器片段在 CSS 里的全部定义点(行号 + 所在 @media 上下文)。
pub struct FrontendLocateTool;

#[async_trait]
impl Tool for FrontendLocateTool {
    fn name(&self) -> &'static str {
        "frontend_locate"
    }
    fn description(&self) -> String {
        "定位一个 CSS 选择器片段的全部定义点:返回每处的行号、完整选择器与所在 @media 条件块。\
         改样式前先跑一次——同一个 class 常在多处定义(基础规则 + 响应式覆盖),漏改一处就会\
         出现「只在某些宽度下才错」。只读,不改文件。"
            .into()
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(FrontendLocateInput)).unwrap()
    }
    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: FrontendLocateInput = match serde_json::from_value(input) {
            Ok(value) => value,
            Err(e) => return ToolOutput::error(format!("invalid input: {e}")),
        };
        let rel = input
            .css_path
            .unwrap_or_else(|| "crates/kanzei-app/ui/style.css".to_string());
        let path = ctx.project_root.join(&rel);
        let css = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) => return ToolOutput::error(format!("读取 {} 失败: {e}", path.display())),
        };
        let needle = input.selector.trim();
        let sites = find_rule_sites(&css, needle);
        if sites.is_empty() {
            return ToolOutput::ok(format!(
                "{rel} 里没有匹配 `{needle}` 的规则。检查拼写,或该样式可能写在行内 style / 别的文件。"
            ));
        }
        let lines: Vec<String> = sites
            .iter()
            .map(|site| {
                if site.context.is_empty() {
                    format!("{}:{}  {}", rel, site.line, site.selector)
                } else {
                    format!(
                        "{}:{}  {}   [{}]",
                        rel, site.line, site.selector, site.context
                    )
                }
            })
            .collect();
        ToolOutput::ok(format!("{} 处定义:\n{}", sites.len(), lines.join("\n")))
    }
}

#[derive(Deserialize, JsonSchema)]
struct FrontendCheckInput {
    /// CSS 文件路径(相对项目根)。默认 crates/kanzei-app/ui/style.css。
    #[serde(default)]
    css_path: Option<String>,
}

/// CSS 结构完整性检查(花括号配对 / 孤儿规则)。
pub struct FrontendCheckTool;

#[async_trait]
impl Tool for FrontendCheckTool {
    fn name(&self) -> &'static str {
        "frontend_check"
    }
    fn description(&self) -> String {
        "检查 CSS 结构完整性:花括号是否配对、有没有多余的 `}`。浏览器对花括号错配是静默\
         容错的——被吃掉一个 `@media ... {` 会让整段响应式规则无条件生效且不报任何错。\
         改完 CSS 必须跑一次。只读,不改文件。"
            .into()
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(FrontendCheckInput)).unwrap()
    }
    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: FrontendCheckInput = match serde_json::from_value(input) {
            Ok(value) => value,
            Err(e) => return ToolOutput::error(format!("invalid input: {e}")),
        };
        let rel = input
            .css_path
            .unwrap_or_else(|| "crates/kanzei-app/ui/style.css".to_string());
        let path = ctx.project_root.join(&rel);
        let css = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) => return ToolOutput::error(format!("读取 {} 失败: {e}", path.display())),
        };
        let issues = css_structure_issues(&css);
        if issues.is_empty() {
            ToolOutput::ok(format!("{rel}: 结构完整,花括号配对正常。"))
        } else {
            // 结构坏了是要立刻修的,用 error 让它在轨迹里显眼。
            ToolOutput::error(format!("{rel} 结构损坏:\n- {}", issues.join("\n- ")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 定位给出全部定义点并标出所在媒体查询() {
        let css = "\
.doc-edit { color: red; }
/* 注释里的 .doc-edit 不算 */
@media (max-width: 700px) {
  .doc-edit { color: blue; }
}
.other { color: green; }
";
        let sites = find_rule_sites(css, ".doc-edit");
        assert_eq!(sites.len(), 2, "{sites:?}");
        assert_eq!(sites[0].line, 1);
        assert!(sites[0].context.is_empty(), "顶层规则不该带上下文");
        assert_eq!(sites[1].line, 4);
        assert!(
            sites[1].context.contains("max-width: 700px"),
            "响应式覆盖必须标出所在 @media,否则改了基础规则还以为改完了: {:?}",
            sites[1],
        );
    }

    /// D-197:条件块的进出必须按花括号深度算,不能"见到收尾行就弹一层"。
    /// 两种形态在本仓库 style.css 上都真实发生过。
    #[test]
    fn 条件块上下文不被多行规则提前关闭也不泄漏到块外() {
        // ① 块内多行规则的收尾 } 不得把 @media 提前弹掉。
        let css = "\
@media (max-width: 700px) {
  .a {
    color: blue;
  }
  .b { color: red; }
}
.c { color: green; }
";
        let sites = find_rule_sites(css, "");
        let by_selector = |name: &str| {
            sites
                .iter()
                .find(|s| s.selector == name)
                .unwrap_or_else(|| panic!("没找到 {name}: {sites:?}"))
                .context
                .clone()
        };
        assert!(by_selector(".a").contains("max-width: 700px"));
        assert!(
            by_selector(".b").contains("max-width: 700px"),
            "多行规则的收尾 }} 把 @media 提前弹掉了,块内后续规则被报成顶层"
        );
        assert!(by_selector(".c").is_empty(), "块外规则不该带上下文");

        // ② 单行写完的条件块必须当场出栈,不能糊到它之后的顶层规则上。
        //    这一种更糟:它把顶层规则报成在一个根本不包含它的块里。
        let css = "\
@keyframes fadein { from { opacity: 0; } to { opacity: 1; } }
.msg { color: red; }
";
        let sites = find_rule_sites(css, ".msg");
        assert_eq!(sites.len(), 1, "{sites:?}");
        assert!(
            sites[0].context.is_empty(),
            "单行 @keyframes 泄漏到了它之后的顶层规则上: {:?}",
            sites[0]
        );
    }

    #[test]
    fn 结构检查抓得住被吃掉的开括号() {
        // D-164 的真实形态:@media 那一行被别的规则替换掉,留下孤儿规则与多余的 }
        let broken = "\
.a { color: red; }
  .b { color: blue; }
}
";
        let issues = css_structure_issues(broken);
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert!(issues[0].contains("多余"), "{}", issues[0]);
        assert!(
            issues[0].contains("行 3"),
            "要给出行号才好定位: {}",
            issues[0]
        );

        assert!(css_structure_issues(".a { color: red; }\n").is_empty());
        assert!(
            css_structure_issues("@media (min-width: 1px) {\n .a { color: red; }\n")
                .iter()
                .any(|i| i.contains("未闭合")),
        );
        // 注释里的花括号不参与配对,且不能打乱行号。
        let with_comment = "/* { 这里有个假括号\n 还有一行 } */\n.a { color: red; }\n";
        assert!(css_structure_issues(with_comment).is_empty());
    }
}
