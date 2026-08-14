//! 权限规则持久化(R-205 从 config.rs 拆出)。
//!
//! config.rs 原混装四域;本文件承接「权限规则持久化」一域:append_allow_rule 的
//! 文本级追加、规则摘要(rule_digest)、通配判定、资源保留。config.rs 经 re-export
//! 保持 `config::xxx` 调用点零变更。

use std::path::{Path, PathBuf};

use crate::config::KanzeiConfig;
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
pub fn append_allow_rule(
    project_root: &Path,
    action: &str,
    resource: &str,
) -> anyhow::Result<PathBuf> {
    let path = project_root.join(".kanzei").join("kanzei.toml");
    let text = if path.is_file() {
        std::fs::read_to_string(&path)?
    } else {
        String::new()
    };
    // 语义预检:类型错误在这里明确报出,而不是把规则追进一个坏文件。
    toml::from_str::<KanzeiConfig>(&text)
        .map_err(|e| anyhow::anyhow!("invalid {}: {e}", path.display()))?;
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid {}: {e}", path.display()))?;
    let permissions = doc.entry("permissions").or_insert(toml_edit::table());
    let Some(permissions) = permissions.as_table_mut() else {
        anyhow::bail!("{}: `permissions` 不是表,无法追加规则", path.display());
    };
    permissions.set_implicit(true);
    let rules = permissions
        .entry("rules")
        .or_insert(toml_edit::Item::ArrayOfTables(
            toml_edit::ArrayOfTables::new(),
        ));
    let Some(rules) = rules.as_array_of_tables_mut() else {
        anyhow::bail!(
            "{}: `permissions.rules` 不是数组表,无法追加规则",
            path.display()
        );
    };
    let mut rule = toml_edit::Table::new();
    rule.insert("action", toml_edit::value(action));
    rule.insert("resource", toml_edit::value(resource));
    rule.insert("effect", toml_edit::value("allow"));
    rules.push(rule);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, doc.to_string())?;
    Ok(path)
}

/// "总是允许"时保留具体资源，避免把一个命令的授权扩大为首词通配。
/// bash 的 shell/解释器语义无法靠首词推断安全边界；调用方仍可对
/// 用户明确配置的整体 `*` 使用 yolo 语义。
pub fn generalize_resource(action: &str, resource: &str) -> String {
    let _ = action;
    resource.to_string()
}
