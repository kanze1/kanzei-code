//! Global Skills management. Generation produces a draft without opening a conversation.
use std::path::{Path, PathBuf};

use futures::StreamExt;
use kanzei_harness::{skills as catalog, SkillDef};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkillDraft {
    name: String,
    description: String,
    instructions: String,
    #[serde(default)]
    manual_only: bool,
    #[serde(default = "yes")]
    user_invocable: bool,
}
fn yes() -> bool {
    true
}

fn validate(draft: &SkillDraft) -> Result<(), String> {
    let name = &draft.name;
    if name.is_empty()
        || name.chars().count() > 64
        || name.starts_with('-')
        || name.ends_with('-')
        || name.contains("--")
        || !name
            .chars()
            .all(|c| c == '-' || c.is_alphanumeric() && !c.is_uppercase())
    {
        return Err("名称请使用小写字母、数字和连字符，最多 64 个字符".into());
    }
    if draft.description.trim().is_empty() || draft.description.chars().count() > 1024 {
        return Err("描述不能为空，最多 1024 个字符".into());
    }
    if draft.instructions.trim().is_empty() || draft.instructions.len() > 120_000 {
        return Err("技能正文不能为空，最多 120 KB".into());
    }
    Ok(())
}

fn entry(skill: &SkillDef, home: &Path, disabled: &std::collections::BTreeSet<String>) -> Value {
    let source = if skill.path.starts_with(home.join("builtin-skills")) {
        "builtin"
    } else if skill.path.starts_with(home.join("skills")) {
        "personal"
    } else {
        "external"
    };
    let title = if source == "builtin" {
        catalog::BUILTINS
            .iter()
            .find(|(name, _, _)| *name == skill.name)
            .map(|(_, title, _)| *title)
    } else {
        None
    };
    json!({"name":skill.name, "displayName":title.unwrap_or(&skill.name), "description":skill.description,
        "path":skill.path, "source":source, "editable":source == "personal", "enabled":!disabled.contains(&skill.name),
        "manualOnly":skill.disable_model_invocation, "userInvocable":skill.user_invocable})
}

fn list_at(home: &Path) -> Result<Vec<Value>, String> {
    let disabled = catalog::preferences_at(home)
        .map_err(|e| e.to_string())?
        .disabled;
    Ok(
        catalog::catalog_at(home, catalog::compatibility_home().as_deref())
            .map_err(|e| e.to_string())?
            .iter()
            .map(|skill| entry(skill, home, &disabled))
            .collect(),
    )
}
fn home() -> Result<PathBuf, String> {
    catalog::global_home().map_err(|e| e.to_string())
}

fn skill_lock(home: &Path, name: &str) -> Result<kanzei_base::atomic_file::FileLock, String> {
    let directory = home.join("skill-locks");
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    kanzei_base::atomic_file::lock_exclusive(&directory.join(name)).map_err(|e| e.to_string())
}
fn find_at(home: &Path, name: &str) -> Result<SkillDef, String> {
    catalog::catalog_at(home, catalog::compatibility_home().as_deref())
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|skill| skill.name == name)
        .ok_or_else(|| format!("Skill 不存在：{name}"))
}

#[tauri::command]
pub(crate) fn skills_list() -> Result<Vec<Value>, String> {
    list_at(&home()?)
}

fn read_at(home: &Path, name: &str) -> Result<Value, String> {
    let skill = find_at(home, name)?;
    let content = std::fs::read_to_string(&skill.path).map_err(|e| e.to_string())?;
    if content.len() > 131_072 {
        return Err("Skill 太大，请把参考资料放入 references".into());
    }
    let mut value = entry(
        &skill,
        home,
        &catalog::preferences_at(home)
            .map_err(|e| e.to_string())?
            .disabled,
    );
    value["instructions"] = json!(
        kanzei_harness::markdown::parse_frontmatter(content.trim_start_matches('\u{feff}')).body
    );
    value["revision"] = json!(kanzei_base::content_hash(content.as_bytes()));
    Ok(value)
}
#[tauri::command]
pub(crate) fn skills_read(name: String) -> Result<Value, String> {
    read_at(&home()?, &name)
}

/// Reject links escaping the application-owned directory before a write or move.
fn managed_path(home: &Path, path: &Path) -> Result<PathBuf, String> {
    let base = home.join("skills");
    std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let canonical_base = std::fs::canonicalize(&base).map_err(|e| e.to_string())?;
    let parent = path.parent().ok_or("技能路径无效")?;
    if parent.exists() {
        let actual = std::fs::canonicalize(parent).map_err(|e| e.to_string())?;
        if !actual.starts_with(&canonical_base) {
            return Err("只能修改自己的全局 Skills".into());
        }
    } else if !parent.starts_with(&base) {
        return Err("只能修改自己的全局 Skills".into());
    }
    if path.exists() {
        let actual = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
        if !actual.starts_with(canonical_base) {
            return Err("技能文件指向全局目录之外".into());
        }
    }
    Ok(path.to_owned())
}

fn render(draft: &SkillDraft, previous: &str) -> String {
    // Keep unrelated frontmatter (e.g. allowed-tools and metadata) on an edit.
    let normalized = previous
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n");
    let mut yaml = normalized
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n"))
        .and_then(|(yaml, _)| serde_yaml_ng::from_str::<serde_yaml_ng::Mapping>(yaml).ok())
        .unwrap_or_default();
    yaml.insert("name".into(), draft.name.clone().into());
    yaml.insert("description".into(), draft.description.clone().into());
    yaml.insert("disable-model-invocation".into(), draft.manual_only.into());
    yaml.insert("user-invocable".into(), draft.user_invocable.into());
    format!(
        "---\n{}---\n\n{}\n",
        serde_yaml_ng::to_string(&yaml).unwrap(),
        draft.instructions.trim()
    )
}

fn save_at(home: &Path, draft: SkillDraft, expected_hash: Option<&str>) -> Result<Value, String> {
    validate(&draft)?;
    let _lock = skill_lock(home, &draft.name)?;
    let existing = find_at(home, &draft.name).ok();
    let path = if let Some(skill) = &existing {
        if !skill.path.starts_with(home.join("skills")) {
            return Err("该名称已被占用，请换一个名称创建自定义 Skill".into());
        }
        if expected_hash.is_none() {
            return Err("同名 Skill 已存在，请打开后编辑".into());
        }
        managed_path(home, &skill.path)?
    } else {
        managed_path(
            home,
            &home.join("skills").join(&draft.name).join("SKILL.md"),
        )?
    };
    let previous = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.to_string()),
    };
    let content = render(&draft, &previous);
    if content.len() > 131_072 {
        return Err("Skill 太大，请把参考资料放入 references".into());
    }
    kanzei_base::atomic_file::write_atomic_cas(
        &path,
        &content,
        expected_hash.unwrap_or(&kanzei_base::content_hash(b"")),
        |text| kanzei_base::content_hash(text.as_bytes()),
    )?;
    read_at(home, &draft.name)
}
#[tauri::command]
pub(crate) fn skills_save(
    draft: SkillDraft,
    expected_hash: Option<String>,
) -> Result<Value, String> {
    save_at(&home()?, draft, expected_hash.as_deref())
}

#[tauri::command]
pub(crate) fn skills_set_enabled(name: String, enabled: bool) -> Result<Vec<Value>, String> {
    let home = home()?;
    find_at(&home, &name)?;
    catalog::set_enabled_at(&home, &name, enabled).map_err(|e| e.to_string())?;
    list_at(&home)
}

fn delete_at(home: &Path, name: &str, expected_hash: &str) -> Result<(), String> {
    let skill = find_at(home, name)?;
    if !skill.path.starts_with(home.join("skills")) {
        return Err("内置和外部 Skills 可以停用，不能从这里删除".into());
    }
    let _lock = skill_lock(home, name)?;
    let path = managed_path(home, &skill.path)?;
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    if kanzei_base::content_hash(text.as_bytes()) != expected_hash {
        return Err("Skill 已被修改，请刷新后再删除".into());
    }
    let target = if path.file_name().is_some_and(|name| name == "SKILL.md") {
        path.parent().unwrap()
    } else {
        &path
    };
    let trash = home.join("skill-trash");
    std::fs::create_dir_all(&trash).map_err(|e| e.to_string())?;
    std::fs::rename(
        target,
        trash.join(format!(
            "{}-{name}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
#[tauri::command]
pub(crate) fn skills_delete(name: String, expected_hash: String) -> Result<Vec<Value>, String> {
    let home = home()?;
    delete_at(&home, &name, &expected_hash)?;
    list_at(&home)
}

fn copy_resources(
    source: &Path,
    destination: &Path,
    count: &mut usize,
    bytes: &mut u64,
) -> Result<(), String> {
    for file in std::fs::read_dir(source).map_err(|e| e.to_string())? {
        let file = file.map_err(|e| e.to_string())?;
        let meta = std::fs::symlink_metadata(file.path()).map_err(|e| e.to_string())?;
        if meta.file_type().is_symlink() {
            return Err("导入目录含有链接，请使用实际文件".into());
        }
        *count += 1;
        *bytes += meta.len();
        if *count > 1024 || *bytes > 16 * 1024 * 1024 {
            return Err("技能目录过大，请保持在 1024 个文件和 16 MB 以内".into());
        }
        let target = destination.join(file.file_name());
        if meta.is_dir() {
            std::fs::create_dir(&target).map_err(|e| e.to_string())?;
            copy_resources(&file.path(), &target, count, bytes)?;
        } else if meta.is_file() {
            std::fs::copy(file.path(), target).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
fn import_at(home: &Path, directory: &Path) -> Result<Value, String> {
    let directory = std::fs::canonicalize(directory).map_err(|e| e.to_string())?;
    let text = std::fs::read_to_string(directory.join("SKILL.md"))
        .map_err(|_| "请选择包含 SKILL.md 的技能文件夹")?;
    if text.len() > 131_072 {
        return Err("SKILL.md 超过 128 KB".into());
    }
    let parent = directory.parent().ok_or("技能目录无效")?;
    // Use the same parser as the runtime, including YAML blocks and invocation flags.
    let name = directory
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or("技能名称无效")?;
    let skill = kanzei_harness::markdown::skills_in_directory(parent)
        .into_iter()
        .find(|skill| skill.path == directory.join("SKILL.md"))
        .ok_or("SKILL.md 无效，请检查名称、描述和 YAML")?;
    if skill.name != name {
        return Err("技能名称必须与文件夹名称一致".into());
    }
    if list_at(home)?.iter().any(|skill| skill["name"] == name) {
        return Err("同名 Skill 已存在，导入不会覆盖它".into());
    }
    let _lock = skill_lock(home, name)?;
    let destination = home.join("skills").join(name);
    managed_path(home, &destination.join("SKILL.md"))?;
    let staging = home.join(format!(
        ".skill-import-{}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        std::process::id()
    ));
    std::fs::create_dir(&staging).map_err(|e| e.to_string())?;
    let result = copy_resources(&directory, &staging, &mut 0, &mut 0)
        .and_then(|_| std::fs::rename(&staging, &destination).map_err(|e| e.to_string()));
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result?;
    read_at(home, name)
}
#[tauri::command]
pub(crate) fn skills_import(directory: String) -> Result<Value, String> {
    import_at(&home()?, Path::new(&directory))
}

fn duplicate_at(home: &Path, name: &str, new_name: &str) -> Result<Value, String> {
    let source = find_at(home, name)?;
    let details = read_at(home, name)?;
    let draft = SkillDraft {
        name: new_name.into(),
        description: source.description.clone(),
        instructions: details["instructions"].as_str().unwrap_or_default().into(),
        manual_only: source.disable_model_invocation,
        user_invocable: source.user_invocable,
    };
    validate(&draft)?;
    let _lock = skill_lock(home, new_name)?;
    if find_at(home, new_name).is_ok() {
        return Err("同名 Skill 已存在，请换一个名称".into());
    }
    let path = managed_path(home, &home.join("skills").join(new_name).join("SKILL.md"))?;
    let destination = path.parent().unwrap();
    let staging = home.join(format!(
        ".skill-copy-{}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        std::process::id()
    ));
    std::fs::create_dir(&staging).map_err(|e| e.to_string())?;
    let previous = std::fs::read_to_string(&source.path).map_err(|e| e.to_string())?;
    let result = (|| {
        if source
            .path
            .file_name()
            .is_some_and(|name| name == "SKILL.md")
        {
            copy_resources(source.path.parent().unwrap(), &staging, &mut 0, &mut 0)?;
        }
        kanzei_base::atomic_file::write_atomic(
            &staging.join("SKILL.md"),
            &render(&draft, &previous),
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(&staging, destination).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result?;
    read_at(home, new_name)
}
#[tauri::command]
pub(crate) fn skills_duplicate(name: String, new_name: String) -> Result<Value, String> {
    duplicate_at(&home()?, &name, &new_name)
}

fn parse_generated(text: &str) -> Result<SkillDraft, String> {
    let text = text.trim();
    let text = text
        .strip_prefix("```json")
        .or_else(|| text.strip_prefix("```"))
        .and_then(|text| text.strip_suffix("```"))
        .unwrap_or(text)
        .trim();
    let draft: SkillDraft =
        serde_json::from_str(text).map_err(|_| "模型返回的技能草稿格式无效，请重新生成")?;
    validate(&draft)?;
    Ok(draft)
}

#[tauri::command]
pub(crate) async fn skills_generate(description: String) -> Result<SkillDraft, String> {
    if description.trim().is_empty() || description.len() > 16_000 {
        return Err("请简要描述技能用途，最多 16 KB".into());
    }
    let home = home()?;
    let config = kanzei_harness::KanzeiConfig::load_at_root(&home).map_err(|e| e.to_string())?;
    let proxy = match config.proxy.as_deref() {
        Some("off") => kanzei_llm::ProxyConfig::Disabled,
        Some("env") | None => kanzei_llm::ProxyConfig::Env,
        Some(value) => kanzei_llm::ProxyConfig::Explicit(value.to_owned()),
    };
    let model = config.resolve_model("primary").map_err(|e| e.to_string())?;
    let route = kanzei_core::build_route(&model, &proxy)
        .await
        .map_err(|e| e.to_string())?;
    let client = kanzei_llm::LlmClient::new(&proxy).map_err(|e| e.to_string())?;
    let guidance = catalog::BUILTINS[0].2;
    let request = kanzei_llm::LlmRequest {
        model: model.model.clone(),
        system: vec![format!("你是 Skills 草稿生成助手。只返回一个 JSON 对象，字段 name、description、instructions；name 使用小写英文、数字和连字符，最多 64 个字符，description 最多 1024 个字符。instructions 为简洁的 Markdown 正文。根据用户语言编写描述和正文。生成可复用、触发条件明确的技能，不执行任务，不写文件，不虚构工具或能力，不引用当前机器不存在的绝对路径。\n{guidance}")],
        messages: vec![kanzei_llm::Message::user_text(&description)], tools: Vec::new(), hosted_tools: Vec::new(),
        max_tokens: 4096, temperature: None, reasoning: kanzei_llm::ReasoningEffort::Off,
        service_tier: config.service_tier_for(&model),
    };
    tokio::time::timeout(std::time::Duration::from_secs(120), async {
        let mut stream = client
            .stream(&route, &request)
            .await
            .map_err(|e| e.to_string())?;
        let mut output = String::new();
        let mut finished = false;
        while let Some(event) = stream.next().await {
            match event.map_err(|e| e.to_string())? {
                kanzei_llm::LlmEvent::TextDelta { text, .. } => output.push_str(&text),
                kanzei_llm::LlmEvent::StepFinish {
                    reason: kanzei_llm::FinishReason::MaxTokens,
                    ..
                } => return Err("技能草稿被截断，请缩短描述后重新生成".into()),
                kanzei_llm::LlmEvent::StepFinish { .. } => finished = true,
                _ => {}
            }
        }
        if !finished {
            return Err("生成连接中断，请重新生成".into());
        }
        parse_generated(&output)
    })
    .await
    .map_err(|_| "技能生成超时，请重试")?
}

pub(crate) fn append_explicit_instructions(
    system: &mut String,
    prompt: &str,
) -> anyhow::Result<()> {
    let home = catalog::global_home()?;
    let disabled = catalog::preferences_at(&home)?.disabled;
    for skill in catalog::global_catalog()? {
        let token = format!("${}", skill.name);
        let invoked = prompt.match_indices(&token).any(|(start, _)| {
            let previous = prompt[..start].chars().next_back();
            previous.is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
                && prompt[start + token.len()..]
                    .chars()
                    .next()
                    .is_none_or(|c| !c.is_alphanumeric() && c != '-' && c != '_')
        });
        if !invoked {
            continue;
        }
        anyhow::ensure!(
            !disabled.contains(&skill.name),
            "Skill 已停用：{}，请在 Skills 中启用",
            skill.name
        );
        anyhow::ensure!(skill.user_invocable, "Skill 不允许手动调用：{}", skill.name);
        anyhow::ensure!(
            std::fs::metadata(&skill.path)?.len() <= 131_072,
            "Skill 太大：{}",
            skill.name
        );
        system.push_str(&format!("\n用户显式调用了全局 Skill ${}。遵循用户当前指令；相对路径以 {} 的目录为基准，按需读取引用资料。\n{}\n",
            skill.name, skill.path.display(), std::fs::read_to_string(&skill.path)?));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temp() -> PathBuf {
        std::env::temp_dir().join(format!(
            "kz-skill-manager-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
    fn draft() -> SkillDraft {
        SkillDraft {
            name: "sample-check".into(),
            description: "Verify a sample".into(),
            instructions: "Read references/example.md.".into(),
            manual_only: false,
            user_invocable: true,
        }
    }
    #[test]
    fn global_save_edit_conflict_delete_and_import_resources() {
        let root = temp();
        let home = root.join("home");
        let created = save_at(&home, draft(), None).unwrap();
        assert!(save_at(&home, draft(), None).is_err());
        let revision = created["revision"].as_str().unwrap();
        let mut edited = draft();
        edited.instructions = "Updated instructions".into();
        let saved = save_at(&home, edited, Some(revision)).unwrap();
        assert!(save_at(&home, draft(), Some(revision)).is_err());
        assert!(delete_at(&home, "sample-check", revision).is_err());
        delete_at(&home, "sample-check", saved["revision"].as_str().unwrap()).unwrap();
        assert!(find_at(&home, "sample-check").is_err());
        let source = root.join("import-check");
        std::fs::create_dir_all(source.join("references")).unwrap();
        std::fs::write(source.join("SKILL.md"), "---\nname: import-check\ndescription: >\n  Imported skill\ndisable-model-invocation: true\n---\nRead references/source.md").unwrap();
        std::fs::write(source.join("references/source.md"), "source evidence").unwrap();
        let imported = import_at(&home, &source).unwrap();
        assert!(imported["manualOnly"].as_bool().unwrap());
        assert_eq!(
            std::fs::read_to_string(home.join("skills/import-check/references/source.md")).unwrap(),
            "source evidence"
        );
        assert!(import_at(&home, &source).is_err());
        duplicate_at(&home, "import-check", "import-copy").unwrap();
        assert_eq!(
            std::fs::read_to_string(home.join("skills/import-copy/references/source.md")).unwrap(),
            "source evidence"
        );
        let copied = read_at(&home, "import-copy").unwrap();
        let edited = SkillDraft {
            name: "import-copy".into(),
            description: "Updated imported skill".into(),
            instructions: "Read references/source.md".into(),
            manual_only: true,
            user_invocable: true,
        };
        save_at(&home, edited, copied["revision"].as_str()).unwrap();
        assert!(
            std::fs::read_to_string(home.join("skills/import-copy/SKILL.md"))
                .unwrap()
                .contains("name: import-copy")
        );
        let mut invalid = draft();
        invalid.name = "../escape".into();
        assert!(save_at(&home, invalid, None).is_err());
        assert!(delete_at(&home, "pdf", "").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn enabled_state_is_global_and_explicit_calls_obey_it() {
        let root = temp();
        crate::settings::with_kanzei_home(&root, || {
            save_at(&root, draft(), None).unwrap();
            let mut system = String::new();
            append_explicit_instructions(&mut system, "use $sample-check").unwrap();
            assert!(system.contains("Read references/example.md"));
            let mut chinese = String::new();
            append_explicit_instructions(&mut chinese, "请使用$sample-check").unwrap();
            assert!(chinese.contains("Read references/example.md"));
            let mut other = String::new();
            append_explicit_instructions(&mut other, "$sample-check-other").unwrap();
            assert!(other.is_empty());
            skills_set_enabled("sample-check".into(), false).unwrap();
            assert!(append_explicit_instructions(&mut String::new(), "$sample-check").is_err());
            assert!(
                !kanzei_harness::markdown::discover_skills(&root.join("project-one"))
                    .iter()
                    .any(|s| s.name == "sample-check")
            );
            skills_set_enabled("sample-check".into(), true).unwrap();
            let one = kanzei_harness::markdown::discover_skills(&root.join("project-one"));
            let two = kanzei_harness::markdown::discover_skills(&root.join("project-two"));
            assert_eq!(
                one.iter().map(|s| &s.name).collect::<Vec<_>>(),
                two.iter().map(|s| &s.name).collect::<Vec<_>>()
            );
        });
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn generated_draft_requires_valid_metadata_and_complete_json() {
        assert!(parse_generated("{\"name\":\"daily-review\",\"description\":\"Review daily progress\",\"instructions\":\"Read the supplied report.\"}").is_ok());
        assert!(parse_generated(
            "{\"name\":\"bad/name\",\"description\":\"x\",\"instructions\":\"x\"}"
        )
        .is_err());
        assert!(parse_generated("{\"name\":\"daily-review\"").is_err());
    }
}
