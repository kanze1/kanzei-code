//! v25 路径形态迁移(UI2-0926 #13,docs/design/project_workspace.md §3)。
//!
//! 桌面端的项目身份根原先是 `std::fs::canonicalize` 的产物,Windows 上带 `\\?\` 前缀,于是
//! `processes` 的主键(`d|\\?\C:\…`、`p3|\\?\C:\…`)、`origin_project/project_dir/worktree_path`、
//! `retired_processes`、`file_checkpoints` 的 `tree_root/abs_path/process_id` 与
//! `sessions.project_root` 都存成了 verbatim 形态。身份根改成 `path_form::canonical` 之后,
//! 这些行必须同步改写,否则 `list_processes(origin)` 精确匹配不到,线从界面消失。
//!
//! 规则:只改写 [`kanzei_base::path_form::simplify_str`] 判定可安全去前缀的值(超长路径、保留名
//! 保持原样);主键冲突(同一个位置两种写法各有一行)保留时间戳较新的一行、删掉另一行。

use std::collections::HashSet;

use kanzei_base::path_form::simplify_str;
use rusqlite::{params, Connection, OptionalExtension};

use super::StoreError;

/// 路径文本的 simplify 形态(不改动时原样返回)。
pub(crate) fn simplify_text(value: &str) -> String {
    simplify_str(value).into_owned()
}

/// `<前缀>|<路径>` 形态的进程 id:路径部分可简化时返回新 id,否则 None。
pub(crate) fn simplify_process_id(id: &str) -> Option<String> {
    let (prefix, path) = id.split_once('|')?;
    let simplified = simplify_str(path);
    (simplified != path).then(|| format!("{prefix}|{simplified}"))
}

/// 同一个位置可能出现在库里的三种写法:调用方给的、simplify 形态、verbatim 形态。
/// 用于读时兜底(迁移之后理论上只剩 simplify 形态)。
pub(crate) fn path_forms(value: &str) -> [String; 3] {
    let simplified = simplify_text(value);
    let verbatim = match simplified.strip_prefix(r"\\") {
        Some(rest) if !simplified.starts_with(r"\\?\") => format!(r"\\?\UNC\{rest}"),
        _ if simplified.starts_with(r"\\?\") => simplified.clone(),
        _ => format!(r"\\?\{simplified}"),
    };
    [value.to_string(), simplified, verbatim]
}

fn changed(value: &str) -> Option<String> {
    let simplified = simplify_str(value);
    (simplified != value).then(|| simplified.into_owned())
}

pub(crate) fn simplify_stored_paths(connection: &Connection) -> Result<(), StoreError> {
    let processes = simplify_processes(connection)?;
    let retired = simplify_retired(connection)?;
    let checkpoints = simplify_checkpoints(connection)?;
    let sessions = simplify_sessions(connection)?;
    if processes + retired + checkpoints + sessions > 0 {
        tracing::info!(
            processes,
            retired,
            checkpoints,
            sessions,
            "v25 迁移:存量路径与进程 id 去掉 \\\\?\\ 前缀"
        );
    }
    Ok(())
}

type ProcessRow = (String, String, String, Option<String>, i64);

fn simplify_processes(connection: &Connection) -> Result<usize, StoreError> {
    let rows: Vec<ProcessRow> = {
        let mut statement = connection.prepare(
            "SELECT process_id, origin_project, project_dir, worktree_path, updated_at FROM processes",
        )?;
        let mapped = statement.query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })?;
        mapped.collect::<Result<Vec<_>, _>>()?
    };
    let mut deleted: HashSet<String> = HashSet::new();
    let mut touched = 0;
    for (id, origin, dir, worktree, updated_at) in rows {
        if deleted.contains(&id) {
            continue;
        }
        let new_id = simplify_process_id(&id);
        let new_origin = changed(&origin);
        let new_dir = changed(&dir);
        let new_worktree = worktree.as_deref().and_then(changed);
        if new_id.is_none() && new_origin.is_none() && new_dir.is_none() && new_worktree.is_none() {
            continue;
        }
        let target_id = new_id.clone().unwrap_or_else(|| id.clone());
        if new_id.is_some() {
            let existing: Option<i64> = connection
                .query_row(
                    "SELECT updated_at FROM processes WHERE process_id = ?1",
                    params![target_id],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(existing_updated) = existing {
                if existing_updated >= updated_at {
                    // 简化形态那一行更新:它是权威,旧写法这一行作废。
                    connection
                        .execute("DELETE FROM processes WHERE process_id = ?1", params![id])?;
                    deleted.insert(id);
                    touched += 1;
                    continue;
                }
                connection.execute(
                    "DELETE FROM processes WHERE process_id = ?1",
                    params![target_id],
                )?;
                deleted.insert(target_id.clone());
            }
        }
        connection.execute(
            "UPDATE processes SET process_id = ?1, origin_project = ?2, project_dir = ?3,
                    worktree_path = ?4
              WHERE process_id = ?5",
            params![
                target_id,
                new_origin.unwrap_or(origin),
                new_dir.unwrap_or(dir),
                new_worktree.or(worktree),
                id
            ],
        )?;
        touched += 1;
    }
    Ok(touched)
}

fn simplify_retired(connection: &Connection) -> Result<usize, StoreError> {
    let rows: Vec<(String, String, i64)> = {
        let mut statement = connection
            .prepare("SELECT process_id, origin_project, retired_at FROM retired_processes")?;
        let mapped = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        mapped.collect::<Result<Vec<_>, _>>()?
    };
    let mut deleted: HashSet<String> = HashSet::new();
    let mut touched = 0;
    for (id, origin, retired_at) in rows {
        if deleted.contains(&id) {
            continue;
        }
        let new_id = simplify_process_id(&id);
        let new_origin = changed(&origin);
        if new_id.is_none() && new_origin.is_none() {
            continue;
        }
        let target_id = new_id.clone().unwrap_or_else(|| id.clone());
        if new_id.is_some() {
            let existing: Option<i64> = connection
                .query_row(
                    "SELECT retired_at FROM retired_processes WHERE process_id = ?1",
                    params![target_id],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(existing_at) = existing {
                if existing_at >= retired_at {
                    connection.execute(
                        "DELETE FROM retired_processes WHERE process_id = ?1",
                        params![id],
                    )?;
                    deleted.insert(id);
                    touched += 1;
                    continue;
                }
                connection.execute(
                    "DELETE FROM retired_processes WHERE process_id = ?1",
                    params![target_id],
                )?;
                deleted.insert(target_id.clone());
            }
        }
        connection.execute(
            "UPDATE retired_processes SET process_id = ?1, origin_project = ?2 WHERE process_id = ?3",
            params![target_id, new_origin.unwrap_or(origin), id],
        )?;
        touched += 1;
    }
    Ok(touched)
}

fn simplify_checkpoints(connection: &Connection) -> Result<usize, StoreError> {
    // 主键是 (run_id, path_key),path_key 本来就是剥过前缀的比较键,不受影响;只改展示/审计列。
    let rows: Vec<(i64, String, String, Option<String>)> = {
        let mut statement = connection.prepare(
            r"SELECT rowid, tree_root, abs_path, process_id FROM file_checkpoints
               WHERE tree_root LIKE '\\?\%' OR abs_path LIKE '\\?\%'
                  OR process_id LIKE '%|\\?\%'",
        )?;
        let mapped = statement.query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?;
        mapped.collect::<Result<Vec<_>, _>>()?
    };
    let mut touched = 0;
    for (rowid, tree_root, abs_path, process_id) in rows {
        let process_id = process_id.map(|id| simplify_process_id(&id).unwrap_or(id));
        connection.execute(
            "UPDATE file_checkpoints SET tree_root = ?1, abs_path = ?2, process_id = ?3 WHERE rowid = ?4",
            params![
                simplify_text(&tree_root),
                simplify_text(&abs_path),
                process_id,
                rowid
            ],
        )?;
        touched += 1;
    }
    Ok(touched)
}

fn simplify_sessions(connection: &Connection) -> Result<usize, StoreError> {
    // session_id 由 session_identity(剥前缀后哈希)导出,与这一列的写法无关,只改展示列。
    let rows: Vec<(String, String)> = {
        let mut statement = connection.prepare(
            r"SELECT session_id, project_root FROM sessions WHERE project_root LIKE '\\?\%'",
        )?;
        let mapped = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        mapped.collect::<Result<Vec<_>, _>>()?
    };
    let mut touched = 0;
    for (session_id, root) in rows {
        let Some(simplified) = changed(&root) else {
            continue;
        };
        connection.execute(
            "UPDATE sessions SET project_root = ?1 WHERE session_id = ?2",
            params![simplified, session_id],
        )?;
        touched += 1;
    }
    Ok(touched)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{now_ms, SessionStore, StoredProcess};

    fn process(
        id: &str,
        origin: &str,
        dir: &str,
        worktree: Option<&str>,
        at: i64,
    ) -> StoredProcess {
        StoredProcess {
            process_id: id.into(),
            origin_project: origin.into(),
            project_dir: dir.into(),
            worktree_path: worktree.map(str::to_string),
            model: Some(format!("model-{at}")),
            profile: None,
            research_topic: None,
            reasoning: None,
            manual_models: Vec::new(),
            phase_pipeline: false,
            subagents_enabled: true,
            tracker_writes_enabled: false,
            updated_at: at,
        }
    }

    fn temp_db(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("kz-v25-{tag}-{}-{}", std::process::id(), now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("state.db")
    }

    fn force_raw_row(store: &SessionStore, p: &StoredProcess) {
        // upsert 走的是现行代码;这里直接写,模拟 v24 留下的原始行(含 updated_at)。
        store
            .connection
            .execute(
                "INSERT INTO processes (process_id, origin_project, project_dir, worktree_path,
                        model, manual_models, phase_pipeline, subagents_enabled,
                        tracker_writes_enabled, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, '[]', 0, 1, 0, ?6)",
                params![
                    p.process_id,
                    p.origin_project,
                    p.project_dir,
                    p.worktree_path,
                    p.model,
                    p.updated_at
                ],
            )
            .unwrap();
    }

    #[test]
    fn v25_迁移去掉前缀且冲突保留较新行() {
        let path = temp_db("migrate");
        {
            let store = SessionStore::open(&path).unwrap();
            let verbatim = r"\\?\C:\x";
            // 默认进程与一条并行线:都是 verbatim 形态。
            force_raw_row(
                &store,
                &process(r"d|\\?\C:\x", verbatim, verbatim, None, 10),
            );
            force_raw_row(
                &store,
                &process(r"p1|\\?\C:\x", verbatim, verbatim, Some(r"\\?\C:\x\wt"), 20),
            );
            // 同一位置的冲突:p2 两种写法各一行,简化形态那行更新 → 保留它。
            force_raw_row(
                &store,
                &process(r"p2|\\?\C:\x", verbatim, verbatim, None, 30),
            );
            force_raw_row(&store, &process(r"p2|C:\x", r"C:\x", r"C:\x", None, 40));
            // p3:verbatim 那行更新 → 保留 verbatim 行(改写成简化形态),删旧的简化行。
            force_raw_row(&store, &process(r"p3|C:\x", r"C:\x", r"C:\x", None, 50));
            force_raw_row(
                &store,
                &process(r"p3|\\?\C:\x", verbatim, verbatim, None, 60),
            );
            // 超长路径:不能剥,保持原样。
            let long = format!(r"\\?\C:\{}", "l".repeat(300));
            force_raw_row(
                &store,
                &process(&format!("p9|{long}"), &long, &long, None, 70),
            );
            store
                .connection
                .execute(
                    "INSERT INTO retired_processes(process_id, origin_project, retired_at)
                     VALUES (?1, ?2, 5)",
                    params![r"p7|\\?\C:\x", verbatim],
                )
                .unwrap();
            store
                .connection
                .execute(
                    "INSERT INTO file_checkpoints(run_id, path_key, abs_path, rel_path, tree_root,
                         process_id, pre_exists, pre_bytes, captured_at, updated_at)
                     VALUES ('run1', 'c:/x/a.rs', ?1, 'a.rs', ?2, ?3, 1, 3, 1, 1)",
                    params![r"\\?\C:\x\a.rs", verbatim, r"p1|\\?\C:\x"],
                )
                .unwrap();
            store.create_session("ses_v25", verbatim, None).unwrap();
            store
                .connection
                .execute(
                    "UPDATE schema_meta SET value = '24' WHERE key = 'schema_version'",
                    [],
                )
                .unwrap();
        }
        let store = SessionStore::open(&path).unwrap();
        let listed = store.list_processes(r"C:\x").unwrap();
        let ids: Vec<&str> = listed.iter().map(|p| p.process_id.as_str()).collect();
        assert_eq!(ids, vec![r"d|C:\x", r"p1|C:\x", r"p2|C:\x", r"p3|C:\x"]);
        let p1 = listed.iter().find(|p| p.process_id == r"p1|C:\x").unwrap();
        assert_eq!(p1.origin_project, r"C:\x");
        assert_eq!(p1.project_dir, r"C:\x");
        assert_eq!(p1.worktree_path.as_deref(), Some(r"C:\x\wt"));
        let p2 = listed.iter().find(|p| p.process_id == r"p2|C:\x").unwrap();
        assert_eq!(p2.model.as_deref(), Some("model-40"), "冲突保留较新行");
        let p3 = listed.iter().find(|p| p.process_id == r"p3|C:\x").unwrap();
        assert_eq!(
            p3.model.as_deref(),
            Some("model-60"),
            "verbatim 行更新时保留它"
        );
        // 库里真的只剩一种写法(不是只在读时归一)。
        let raw_ids: Vec<String> = {
            let mut statement = store
                .connection
                .prepare("SELECT process_id FROM processes ORDER BY process_id")
                .unwrap();
            let rows = statement.query_map([], |row| row.get(0)).unwrap();
            rows.map(Result::unwrap).collect()
        };
        assert!(
            raw_ids
                .iter()
                .filter(|id| !id.starts_with("p9|"))
                .all(|id| !id.contains(r"\\?\")),
            "{raw_ids:?}"
        );
        assert!(
            raw_ids.iter().any(|id| id.starts_with(r"p9|\\?\C:\")),
            "超长路径必须保留 verbatim:{raw_ids:?}"
        );
        assert_eq!(
            store.list_retired_process_ids(r"C:\x").unwrap(),
            vec![r"p7|C:\x".to_string()]
        );
        let (tree_root, abs_path, owner): (String, String, String) = store
            .connection
            .query_row(
                "SELECT tree_root, abs_path, process_id FROM file_checkpoints WHERE run_id='run1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            (tree_root.as_str(), abs_path.as_str(), owner.as_str()),
            (r"C:\x", r"C:\x\a.rs", r"p1|C:\x")
        );
        let root: String = store
            .connection
            .query_row(
                "SELECT project_root FROM sessions WHERE session_id='ses_v25'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(root, r"C:\x");
        drop(store);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn 读时兜底_两种写法都列得出且归一() {
        let store = SessionStore::open_in_memory().unwrap();
        // 迁移后又冒出来的 verbatim 行(比如旧形态的写入):按简化根也列得出来,且 id 已归一。
        force_raw_row(
            &store,
            &process(r"p4|\\?\C:\y", r"\\?\C:\y", r"\\?\C:\y", None, 1),
        );
        let listed = store.list_processes(r"C:\y").unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].process_id, r"p4|C:\y");
        assert_eq!(listed[0].origin_project, r"C:\y");
        // 传 verbatim 根也一样。
        assert_eq!(store.list_processes(r"\\?\C:\y").unwrap().len(), 1);
    }

    #[test]
    fn 三种写法() {
        assert_eq!(
            path_forms(r"C:\x"),
            [r"C:\x".to_string(), r"C:\x".into(), r"\\?\C:\x".into()]
        );
        assert_eq!(
            path_forms(r"\\?\C:\x"),
            [r"\\?\C:\x".to_string(), r"C:\x".into(), r"\\?\C:\x".into()]
        );
        assert_eq!(path_forms(r"\\s\sh")[2], r"\\?\UNC\s\sh");
        assert_eq!(
            simplify_process_id(r"d|\\?\C:\x").as_deref(),
            Some(r"d|C:\x")
        );
        assert_eq!(simplify_process_id(r"d|C:\x"), None);
        assert_eq!(simplify_process_id("no-bar"), None);
    }
}
