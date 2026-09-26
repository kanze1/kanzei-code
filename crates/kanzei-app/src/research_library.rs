//! 课题身份独立于开发项目；旧目录原地登记，运行 API 继续使用 storage_root + topic。
use std::path::{Path, PathBuf};

use kanzei_tools::atomic_file::{lock_exclusive, write_atomic};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ResearchEntry {
    pub id: String,
    pub topic: Option<String>,
    pub label: String,
    pub kind: String,
    pub storage_root: String,
    pub linked_projects: Vec<String>,
    pub standalone: bool,
}

#[derive(Default, Serialize, Deserialize)]
struct Library {
    next_id: u64,
    entries: Vec<ResearchEntry>,
}

fn home() -> Result<PathBuf, String> {
    kanzei_harness::kanzei_home().ok_or_else(|| "无法确定研究课题库目录".into())
}

fn read_library(path: &Path) -> Result<Library, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("课题登记表读取失败: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Library::default()),
        Err(e) => Err(e.to_string()),
    }
}

fn edit_library<T>(
    home: &Path,
    edit: impl FnOnce(&mut Library) -> Result<T, String>,
) -> Result<T, String> {
    std::fs::create_dir_all(home).map_err(|e| e.to_string())?;
    let path = home.join("research-library.json");
    let _lock = lock_exclusive(&path).map_err(|e| e.to_string())?;
    let mut library = read_library(&path)?;
    let before = serde_json::to_string(&library).map_err(|e| e.to_string())?;
    // UI2-0926 #13 复核:登记表是 normalized_project_root 输出的持久化副本,随 schema v25 一起
    // 去掉 `\\?\` 前缀;迁移结果与本次编辑一起写回(before 取在迁移之前)。
    library.simplify_paths();
    let result = edit(&mut library)?;
    let after = serde_json::to_string(&library).map_err(|e| e.to_string())?;
    if before != after {
        write_atomic(&path, &after).map_err(|e| e.to_string())?;
    }
    Ok(result)
}

/// 存储根的比较键:无条件剥 `\\?\` 前缀 + 小写(Windows 路径大小写不敏感)。
/// 登记表里同一目录可能有两种写法(升级前的 verbatim 与之后的 simplify),不能按原串比。
fn root_key(root: &str) -> String {
    kanzei_tools::path_form::strip_verbatim(root).to_lowercase()
}

/// 两条登记是否指同一个课题:同一存储根下同一 topic;无 topic 的(未绑定/旧版平铺)再按 kind 区分。
/// 与 [`Library::register`] 的认领口径一致。
fn same_slot(entry: &ResearchEntry, root: &str, topic: &Option<String>, kind: &str) -> bool {
    root_key(&entry.storage_root) == root_key(root)
        && entry.topic == *topic
        && (topic.is_some() || entry.kind == kind)
}

fn id_number(id: &str) -> u64 {
    id.strip_prefix("topic-")
        .and_then(|digits| digits.parse().ok())
        .unwrap_or(u64::MAX)
}

impl Library {
    fn next_id(&mut self) -> String {
        self.next_id += 1;
        format!("topic-{:08}", self.next_id)
    }

    /// UI2-0926 #13 复核:`storage_root` / `linked_projects` 改写成 path_form::simplify 形态;
    /// 迁移前后两种写法各登记了一条的(同一课题)合并成编号最小的那条,关联项目取并集。
    /// 不迁移的话,身份根换成 simplify 形态后 register 一条都认不出,每个课题都会以新编号再登记一遍。
    fn simplify_paths(&mut self) {
        for entry in &mut self.entries {
            entry.storage_root =
                kanzei_tools::path_form::simplify_str(&entry.storage_root).into_owned();
            for project in &mut entry.linked_projects {
                *project = kanzei_tools::path_form::simplify_str(project).into_owned();
            }
            dedup_roots(&mut entry.linked_projects);
        }
        let mut order: Vec<usize> = (0..self.entries.len()).collect();
        order.sort_by_key(|&index| id_number(&self.entries[index].id));
        let mut kept: Vec<ResearchEntry> = Vec::with_capacity(self.entries.len());
        for index in order {
            let entry = self.entries[index].clone();
            match kept
                .iter_mut()
                .find(|kept| same_slot(kept, &entry.storage_root, &entry.topic, &entry.kind))
            {
                Some(survivor) => {
                    survivor.linked_projects.extend(entry.linked_projects);
                    dedup_roots(&mut survivor.linked_projects);
                    survivor.standalone |= entry.standalone;
                }
                None => kept.push(entry),
            }
        }
        if kept.len() != self.entries.len() {
            // 只在真有合并时才改顺序(按编号);没有合并时保持原顺序,不制造无意义的写回。
            self.entries = kept;
        }
    }

    fn register(&mut self, root: &str, topic: Option<String>, label: String, kind: String) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| same_slot(entry, root, &topic, &kind))
        {
            entry.label = label;
            entry.kind = kind;
            return;
        }
        let id = self.next_id();
        self.entries.push(ResearchEntry {
            id,
            topic,
            label,
            kind,
            storage_root: root.into(),
            linked_projects: vec![root.into()],
            standalone: false,
        });
    }
}

/// 按比较键去重(保留先出现的写法),再按原串排序。
fn dedup_roots(roots: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    roots.retain(|root| seen.insert(root_key(root)));
    roots.sort();
}

fn list_library(home: &Path, projects: &[String]) -> Result<serde_json::Value, String> {
    edit_library(home, |library| {
        let mut roots = projects.to_vec();
        roots.extend(
            library
                .entries
                .iter()
                .map(|entry| entry.storage_root.clone()),
        );
        roots.sort();
        roots.dedup();
        let mut diagnostics = Vec::new();
        for root in roots {
            if !Path::new(&root).is_dir() {
                continue;
            }
            let root = crate::normalized_project_root(Path::new(&root));
            let root_text = root.to_string_lossy().into_owned();
            if projects
                .iter()
                .any(|project| crate::normalized_project_root(Path::new(project)) == root)
            {
                library.register(
                    &root_text,
                    None,
                    root.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    "unbound".into(),
                );
            }
            let research = root.join(".kanzei/research");
            let Ok(dirs) = std::fs::read_dir(&research) else {
                continue;
            };
            for dir in dirs
                .filter_map(Result::ok)
                .filter(|dir| dir.path().is_dir())
            {
                let name = dir.file_name().to_string_lossy().into_owned();
                if kanzei_tools::docstore::DocStore::validate_topic(&name).is_err() {
                    continue;
                }
                let metadata = crate::research_topics::topic_path(&root, &name).and_then(|path| {
                    crate::research_topics::describe_topic(
                        &path,
                        path.join("sources.md").is_file() || path.join("findings.md").is_file(),
                    )
                });
                match metadata {
                    Ok(metadata) => {
                        library.register(&root_text, Some(name), metadata.title, metadata.kind)
                    }
                    Err(error) => diagnostics.push(error),
                }
            }
            if ["sources.md", "findings.md", "report.md"]
                .iter()
                .any(|name| research.join(name).is_file())
            {
                library.register(&root_text, None, "旧版平铺".into(), "legacy".into());
            }
        }
        let entries = library
            .entries
            .iter()
            .map(|entry| {
                let mut value = serde_json::to_value(entry).expect("serializable research entry");
                value["available"] = serde_json::json!(match entry.topic.as_deref() {
                    Some(topic) =>
                        crate::research_topics::topic_path(Path::new(&entry.storage_root), topic)
                            .is_ok(),
                    None => Path::new(&entry.storage_root).is_dir(),
                });
                value["legacy"] = serde_json::json!(entry.kind == "legacy");
                value
            })
            .collect::<Vec<_>>();
        Ok(serde_json::json!({ "entries": entries, "diagnostics": diagnostics }))
    })
}

#[tauri::command]
pub(crate) fn research_library_list() -> Result<serde_json::Value, String> {
    list_library(&home()?, &crate::prefs::load_prefs().projects)
}

fn create_entry(home: &Path, topic: &str, title: &str) -> Result<ResearchEntry, String> {
    kanzei_tools::docstore::DocStore::validate_topic(topic).map_err(|e| e.to_string())?;
    if title.trim().is_empty() || title.trim().chars().count() > 200 {
        return Err("课题名称应为 1 到 200 个字符".into());
    }
    edit_library(home, |library| {
        let (id, root) = loop {
            let id = library.next_id();
            let root = home.join("research-workspaces").join(&id);
            if !root.exists() {
                break (id, root);
            }
        };
        std::fs::create_dir_all(root.parent().unwrap()).map_err(|e| e.to_string())?;
        // create_dir 防止失败重试覆盖已存在但未登记的目录。
        std::fs::create_dir(&root)
            .map_err(|e| format!("创建课题工作目录失败 {}: {e}", root.display()))?;
        std::fs::create_dir(root.join(".kanzei")).map_err(|e| e.to_string())?;
        crate::research_topics::create_topic(&root, topic, title)?;
        let entry = ResearchEntry {
            id,
            topic: Some(topic.into()),
            label: title.trim().into(),
            kind: "research".into(),
            storage_root: crate::normalized_project_root(&root)
                .to_string_lossy()
                .into_owned(),
            linked_projects: vec![],
            standalone: true,
        };
        library.entries.push(entry.clone());
        Ok(entry)
    })
}

#[tauri::command]
pub(crate) fn research_library_create(
    topic: String,
    title: String,
) -> Result<ResearchEntry, String> {
    create_entry(&home()?, &topic, &title)
}

fn link_projects(home: &Path, id: &str, projects: Vec<String>) -> Result<ResearchEntry, String> {
    let mut normalized = Vec::new();
    for project in projects {
        if !Path::new(&project).is_dir() {
            return Err(format!("关联项目目录不存在: {project}"));
        }
        let root = crate::normalized_project_root(Path::new(&project));
        if !root.is_dir() {
            return Err(format!("关联项目目录不存在: {project}"));
        }
        normalized.push(root.to_string_lossy().into_owned());
    }
    dedup_roots(&mut normalized);
    edit_library(home, |library| {
        let entry = library
            .entries
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or("研究课题不存在")?;
        entry.linked_projects = normalized;
        Ok(entry.clone())
    })
}

#[tauri::command]
pub(crate) fn research_library_link_projects(
    id: String,
    projects: Vec<String>,
) -> Result<ResearchEntry, String> {
    link_projects(&home()?, &id, projects)
}

#[cfg(test)]
mod tests;
