//! 权限规则持久化(R-205 从 config.rs 拆出)。
//!
//! config.rs 原混装四域;本文件承接「权限规则持久化」一域:append_allow_rule 的
//! 文本级追加、规则摘要(rule_digest)、通配判定、资源保留。config.rs 经 re-export
//! 保持 `config::xxx` 调用点零变更。

use std::path::{Path, PathBuf};

use crate::config::update_config_document;
use crate::permission::Rule;

/// `*` 通配资源判定:全仓统一按 trim 后比较,避免两处判定不一致(D-139)。
pub(crate) fn is_wildcard_resource(resource: &str) -> bool {
    resource.trim() == "*"
}

/// 告警里点名规则用的摘要:取前两条的命令文本,各截 40 个**字符**(不是字节——
/// 命令里有中文,按字节切会在多字节中间断开而 panic),其余折成"等 N 条"。
pub(crate) fn rule_digest(rules: &[&Rule]) -> String {
    fn 命令(resource: &str) -> String {
        let text = serde_json::from_str::<serde_json::Value>(resource)
            .ok()
            .and_then(|json| json.get("command")?.as_str().map(str::to_string))
            .unwrap_or_else(|| resource.to_string());
        let mut 头: String = text.chars().take(40).collect();
        if text.chars().count() > 40 {
            头.push('…');
        }
        头
    }
    let 前两条: Vec<String> = rules.iter().take(2).map(|r| 命令(&r.resource)).collect();
    let mut out = 前两条
        .iter()
        .map(|c| format!("`{c}`"))
        .collect::<Vec<_>>()
        .join("、");
    if rules.len() > 前两条.len() {
        out.push_str(&format!(" 等，共 {} 条", rules.len()));
    }
    out
}

/// "总是允许"的持久化:向项目配置追加 allow 规则(后来的规则 last-match-wins)。
/// 文本级追加(D-083):toml_edit 保留注释、排版与未知字段,不做整文件 round-trip。
///
/// 写进去的是**完全相同的那一条**(见 [`generalize_resource`]:同一命令 + 同一目录 / 同一路径),不是「这一类
/// 操作」。界面据此把按钮叫「允许并记住」、提示写明只对这一条生效(UX-148)——别再把它说成「之后不再询问」。
pub fn append_allow_rule(
    project_root: &Path,
    action: &str,
    resource: &str,
) -> anyhow::Result<PathBuf> {
    let path = project_root.join(".kanzei").join("kanzei.toml");
    update_config_document(&path, |doc| {
        let permissions = doc.entry("permissions").or_insert(toml_edit::table());
        let inline_permissions = permissions.is_inline_table();
        if let Some(table) = permissions.as_table_mut() {
            table.set_implicit(true);
        }
        let Some(permissions) = permissions.as_table_like_mut() else {
            anyhow::bail!("{}: `permissions` 不是表,无法追加规则", path.display());
        };
        let empty_rules = if inline_permissions {
            toml_edit::value(toml_edit::Array::new())
        } else {
            toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new())
        };
        let rules = permissions.entry("rules").or_insert(empty_rules);
        let mut rule = toml_edit::Table::new();
        rule.insert("action", toml_edit::value(action));
        rule.insert("resource", toml_edit::value(resource));
        rule.insert("effect", toml_edit::value("allow"));
        if let Some(rules) = rules.as_array_of_tables_mut() {
            rules.push(rule);
        } else if let Some(rules) = rules.as_array_mut() {
            rules.push(rule.into_inline_table());
        } else {
            anyhow::bail!(
                "{}: `permissions.rules` 不是规则数组,无法追加规则",
                path.display()
            );
        }
        Ok(())
    })?;
    Ok(path)
}

/// "总是允许"时保留具体资源，避免把一个命令的授权扩大为首词通配。
/// bash 的 shell/解释器语义无法靠首词推断安全边界；调用方仍可对
/// 用户明确配置的整体 `*` 使用 yolo 语义。
/// 因此「记住」的口径就是**逐字相同**的资源:换个参数、换个工作目录都会重新询问。
pub fn generalize_resource(action: &str, resource: &str) -> String {
    let _ = action;
    resource.to_string()
}
