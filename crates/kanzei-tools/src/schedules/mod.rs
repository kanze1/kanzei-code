//! Project wakeups. This is independent of the tracker's work selection scheduler.
mod executor;
pub mod hosts;
mod when;
pub use executor::{execute, Outcome};
use kanzei_core::SessionStore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
pub use when::When;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Step {
    Run(String),
    Prompt(String),
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduleDef {
    pub name: String,
    pub enabled: bool,
    pub when: String,
    pub host: String,
    #[serde(default = "local_offset_minutes")]
    pub utc_offset_minutes: i32,
    pub catch_up: String,
    pub agent: String,
    pub model: String,
    pub timeout_secs: u64,
    pub max_steps: u32,
    pub steps: Vec<Step>,
    pub writeback: Vec<String>,
    pub body: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct Diagnostic {
    pub file: String,
    pub line: usize,
    pub message: String,
}

fn local_offset_minutes() -> i32 {
    chrono::Local::now().offset().local_minus_utc() / 60
}
pub fn latest_slot(def: &ScheduleDef, now: i64) -> Result<i64, String> {
    use chrono::TimeZone;
    let tz = chrono::FixedOffset::east_opt(def.utc_offset_minutes * 60).ok_or("无效 UTC 偏移")?;
    Ok(When::parse(&def.when)?
        .latest(tz.timestamp_millis_opt(now).single().ok_or("无效时间")?)
        .timestamp_millis())
}
pub fn next_slot(def: &ScheduleDef, now: i64) -> Result<i64, String> {
    use chrono::TimeZone;
    let tz = chrono::FixedOffset::east_opt(def.utc_offset_minutes * 60).ok_or("无效 UTC 偏移")?;
    Ok(When::parse(&def.when)?
        .next(tz.timestamp_millis_opt(now).single().ok_or("无效时间")?)
        .timestamp_millis())
}
fn scalar(raw: &str) -> String {
    let mut quote = None;
    let mut end = raw.len();
    for (i, c) in raw.char_indices() {
        if matches!(c, '\'' | '"') {
            if quote == Some(c) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(c);
            }
        }
        if c == '#' && quote.is_none() && (i == 0 || raw[..i].ends_with(char::is_whitespace)) {
            end = i;
            break;
        }
    }
    let value = raw[..end].trim();
    if value.len() >= 2
        && (value.starts_with('"') && value.ends_with('"')
            || value.starts_with('\'') && value.ends_with('\''))
    {
        value[1..value.len() - 1].into()
    } else {
        value.into()
    }
}
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 160
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_'))
}
pub fn path(root: &Path, name: &str) -> Result<PathBuf, String> {
    if !valid_name(name) {
        return Err("名称只能包含文字、数字、横线和下划线".into());
    }
    Ok(root.join(".kanzei/schedules").join(format!("{name}.md")))
}
pub fn parse(text: &str, name: &str) -> Result<ScheduleDef, Diagnostic> {
    if text.len() > 2 * 1024 * 1024 {
        return Err(Diagnostic {
            file: format!("{name}.md"),
            line: 1,
            message: "任务定义超过 2 MiB".into(),
        });
    }
    let error = |line, message: String| Diagnostic {
        file: format!("{name}.md"),
        line,
        message,
    };
    if !valid_name(name) {
        return Err(error(1, "任务名无效".into()));
    }
    let lines: Vec<_> = text.lines().collect();
    if lines.first().map(|line| line.trim()) != Some("---") {
        return Err(error(1, "缺少 --- 定义头".into()));
    }
    let end = (1..lines.len())
        .find(|i| lines[*i].trim() == "---")
        .ok_or_else(|| error(1, "定义头未闭合".into()))?;
    let mut def = ScheduleDef {
        utc_offset_minutes: local_offset_minutes(),
        name: name.into(),
        enabled: false,
        when: String::new(),
        host: "app".into(),
        catch_up: "once".into(),
        agent: "readonly".into(),
        model: "primary".into(),
        timeout_secs: 1800,
        max_steps: 32,
        steps: vec![],
        writeback: vec!["notify".into()],
        body: lines[end + 1..].join("\n"),
    };
    let mut section = "";
    let mut seen = std::collections::HashSet::new();
    let mut i = 1;
    while i < end {
        let raw = lines[i];
        let trimmed = raw.trim();
        let number = i + 1;
        i += 1;
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(item) = trimmed.strip_prefix("- ") {
            if section == "writeback" {
                let value = scalar(item);
                def.writeback.push(
                    value
                        .strip_prefix("file: ")
                        .map(|path| format!("file:{}", scalar(path)))
                        .unwrap_or(value),
                );
                continue;
            }
            if section != "steps" {
                return Err(error(number, "列表项必须属于 steps 或 writeback".into()));
            }
            let (kind, value) = item
                .split_once(':')
                .ok_or_else(|| error(number, "步骤格式为 run: 命令 或 prompt: 指令".into()))?;
            let mut value = scalar(value);
            if value == "|" {
                let indent = raw.len() - raw.trim_start().len();
                let start = i;
                while i < end
                    && (lines[i].trim().is_empty()
                        || lines[i].len() - lines[i].trim_start().len() > indent)
                {
                    i += 1;
                }
                let margin = lines[start..i]
                    .iter()
                    .filter(|line| !line.trim().is_empty())
                    .map(|line| line.len() - line.trim_start().len())
                    .min()
                    .unwrap_or(0);
                value = lines[start..i]
                    .iter()
                    .map(|line| line.get(margin..).unwrap_or(""))
                    .collect::<Vec<_>>()
                    .join("\n");
            }
            if value.trim().is_empty() {
                return Err(error(number, "步骤不能为空".into()));
            }
            def.steps.push(match kind.trim() {
                "run" => Step::Run(value),
                "prompt" => Step::Prompt(value),
                _ => return Err(error(number, "只支持 run / prompt 步骤".into())),
            });
            continue;
        }
        let (key, value) = trimmed
            .split_once(':')
            .ok_or_else(|| error(number, "字段格式应为 key: value".into()))?;
        let value = scalar(value);
        section = "";
        if !seen.insert(key) {
            return Err(error(number, format!("字段重复：{key}")));
        }
        match key {
            "name" if value == name => {}
            "name" => return Err(error(number, "name 必须与文件名一致".into())),
            "enabled" => {
                def.enabled = match value.as_str() {
                    "true" => true,
                    "false" => false,
                    _ => return Err(error(number, "enabled 应为 true / false".into())),
                }
            }
            "when" => {
                When::parse(&value).map_err(|message| error(number, message))?;
                def.when = value;
            }
            "host" => {
                if !matches!(value.as_str(), "app" | "system")
                    && !value.strip_prefix("server:").is_some_and(valid_name)
                {
                    return Err(error(
                        number,
                        "host 应为 app / system / server:ENV-id".into(),
                    ));
                }
                def.host = value;
            }
            "catch_up" => {
                if !matches!(value.as_str(), "once" | "skip") {
                    return Err(error(number, "catch_up 应为 once / skip".into()));
                }
                def.catch_up = value;
            }
            "utc_offset_minutes" => {
                def.utc_offset_minutes = value
                    .parse()
                    .ok()
                    .filter(|offset: &i32| (-1439..=1439).contains(offset))
                    .ok_or_else(|| error(number, "UTC 偏移应在 -1439..1439 分钟".into()))?
            }
            "agent" => def.agent = value,
            "model" => def.model = value,
            "max_steps" => {
                def.max_steps = value
                    .parse()
                    .ok()
                    .filter(|n| *n > 0 && *n <= 1000)
                    .ok_or_else(|| error(number, "max_steps 应为 1..1000".into()))?
            }
            "timeout" => {
                let seconds = value
                    .strip_suffix('m')
                    .and_then(|n| n.parse::<u64>().ok())
                    .and_then(|n| n.checked_mul(60))
                    .or_else(|| {
                        value
                            .strip_suffix('h')
                            .and_then(|n| n.parse::<u64>().ok())
                            .and_then(|n| n.checked_mul(3600))
                    })
                    .or_else(|| value.strip_suffix('s').unwrap_or(&value).parse().ok());
                def.timeout_secs = seconds
                    .filter(|n| *n > 0 && *n <= 21600)
                    .ok_or_else(|| error(number, "timeout 应为 1s..6h".into()))?;
            }
            "steps" if value.is_empty() => section = "steps",
            "writeback" if value.is_empty() => {
                section = "writeback";
                def.writeback.clear();
            }
            _ => return Err(error(number, format!("未知或无效字段：{key}"))),
        }
    }
    if def.steps.len() > 64
        || def.steps.iter().any(|step| match step {
            Step::Run(s) | Step::Prompt(s) => s.len() > 64 * 1024,
        })
    {
        return Err(error(end + 1, "任务最多 64 个步骤，每步最多 64 KiB".into()));
    }
    if !valid_name(&def.agent) || def.model.trim().is_empty() || def.model.contains(['\n', '\r']) {
        return Err(error(end + 1, "agent / model 无效".into()));
    }
    if def.when.is_empty() || def.steps.is_empty() {
        return Err(error(end + 1, "缺少 when 或 steps".into()));
    }
    for channel in &def.writeback {
        if !matches!(channel.as_str(), "notify" | "memory_inbox" | "idea")
            && !channel.strip_prefix("file:").is_some_and(|path| {
                !path.trim().is_empty()
                    && !path.contains(['"', '\n', '\r'])
                    && Path::new(path)
                        .components()
                        .all(|part| matches!(part, std::path::Component::Normal(_)))
                    && !Path::new(path).starts_with(".kanzei")
            })
        {
            return Err(error(end + 1, format!("未知回写通道：{channel}")));
        }
    }
    Ok(def)
}
pub fn render(def: &ScheduleDef) -> String {
    let mut text = format!("---\nname: {}\nenabled: {}\nwhen: {}\nhost: {}\ncatch_up: {}\nagent: {}\nmodel: {}\ntimeout: {}s\nmax_steps: {}\nutc_offset_minutes: {}\nsteps:\n",def.name,def.enabled,def.when,def.host,def.catch_up,def.agent,def.model,def.timeout_secs,def.max_steps,def.utc_offset_minutes);
    for step in &def.steps {
        let (kind, value) = match step {
            Step::Run(value) => ("run", value),
            Step::Prompt(value) => ("prompt", value),
        };
        text.push_str(&format!("  - {kind}: |\n"));
        for line in value.lines() {
            text.push_str(&format!("      {line}\n"));
        }
    }
    text.push_str("writeback:\n");
    for channel in &def.writeback {
        text.push_str(&format!("  - \"{channel}\"\n"));
    }
    text.push_str(&format!("---\n{}", def.body));
    text
}
pub fn set_enabled(text: &str, enabled: bool) -> String {
    let mut lines: Vec<_> = text.lines().map(str::to_owned).collect();
    if lines.is_empty() {
        return text.to_string();
    }
    let end = lines
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, line)| line.trim() == "---")
        .map(|(index, _)| index)
        .unwrap_or(lines.len());
    // Block scalar content belongs to the step, even if it says `enabled:`.
    let mut index = 1;
    let mut enabled_index = None;
    while index < end {
        let raw = &lines[index];
        let trimmed = raw.trim();
        if trimmed.starts_with("enabled:") {
            enabled_index = Some(index);
            break;
        }
        let block_indent = trimmed
            .strip_prefix("- ")
            .and_then(|item| item.split_once(':'))
            .filter(|(_, value)| scalar(value) == "|")
            .map(|_| raw.len() - raw.trim_start().len());
        index += 1;
        if let Some(indent) = block_indent {
            while index < end
                && (lines[index].trim().is_empty()
                    || lines[index].len() - lines[index].trim_start().len() > indent)
            {
                index += 1;
            }
        }
    }
    if let Some(index) = enabled_index {
        lines[index] = format!("enabled: {enabled}");
    } else {
        lines.insert(1, format!("enabled: {enabled}"));
    }
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut out = lines.join(newline);
    if text.ends_with('\n') {
        out.push_str(newline);
    }
    out
}
pub fn load(root: &Path) -> (Vec<ScheduleDef>, Vec<Diagnostic>) {
    let (definitions, diagnostics) = load_with_revisions(root);
    (
        definitions
            .into_iter()
            .map(|(definition, _)| definition)
            .collect(),
        diagnostics,
    )
}
/// Pair the parsed definition with the fingerprint of the same bytes.
pub fn load_with_revisions(root: &Path) -> (Vec<(ScheduleDef, String)>, Vec<Diagnostic>) {
    let mut definitions = vec![];
    let mut diagnostics = vec![];
    let Ok(entries) = std::fs::read_dir(root.join(".kanzei/schedules")) else {
        return (definitions, diagnostics);
    };
    for entry in entries.flatten().filter(|entry| {
        entry
            .path()
            .extension()
            .is_some_and(|extension| extension == "md")
    }) {
        let path = entry.path();
        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
        match std::fs::read_to_string(&path)
            .map_err(|error| Diagnostic {
                file: path.display().to_string(),
                line: 1,
                message: error.to_string(),
            })
            .and_then(|text| {
                parse(&text, &stem).map(|def| (def, kanzei_base::content_hash(text.as_bytes())))
            }) {
            Ok(def) => definitions.push(def),
            Err(error) => diagnostics.push(error),
        }
    }
    definitions.sort_by(|a, b| a.0.name.cmp(&b.0.name));
    (definitions, diagnostics)
}
pub fn history_id(root: &Path) -> String {
    format!("{}#schedules", kanzei_core::project_session_id(root))
}
pub fn history(root: &Path, name: Option<&str>, limit: usize) -> Result<Vec<Value>, String> {
    let store =
        SessionStore::open(&kanzei_core::project_state_path(root)).map_err(|e| e.to_string())?;
    Ok(store
        .list_named_events(&history_id(root), name, None, limit)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(
            |event| json!({"type":event.event_type,"sequence":event.sequence,"data":event.payload}),
        )
        .collect())
}
pub fn record(root: &Path, kind: &str, payload: &Value) -> Result<(), String> {
    let store =
        SessionStore::open(&kanzei_core::project_state_path(root)).map_err(|e| e.to_string())?;
    let session = history_id(root);
    store
        .create_session(&session, &root.display().to_string(), Some("定时任务"))
        .map_err(|e| e.to_string())?;
    let mut payload = payload.clone();
    if kind == "schedule.armed" {
        if let Some(name) = payload["name"].as_str().map(str::to_string) {
            if let Ok(text) = std::fs::read_to_string(path(root, &name)?) {
                payload["revision"] = json!(kanzei_base::content_hash(text.as_bytes()));
            }
        }
    }
    store
        .append_event(&session, kind, &payload)
        .map_err(|e| e.to_string())?;
    Ok(())
}
/// This short file lock never crosses an await. Persisting the claim prevents other app/CLI
/// instances from running this slot or overlapping an unfinished run of the same definition.
pub fn claim(
    root: &Path,
    def: &ScheduleDef,
    slot: i64,
    run_id: &str,
    trigger: &str,
) -> Result<bool, String> {
    let path = path(root, &def.name)?;
    let _lock = kanzei_base::atomic_file::try_lock_exclusive(&path, Duration::from_millis(100))
        .map_err(|e| e.to_string())?;
    let Some(_lock) = _lock else { return Ok(false) };
    let current = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    if parse(&current, &def.name).map_err(|e| e.message)? != *def {
        return Ok(false);
    }
    let store =
        SessionStore::open(&kanzei_core::project_state_path(root)).map_err(|e| e.to_string())?;
    if store
        .event_payload_exists(
            &history_id(root),
            Some("schedule.run_started"),
            "$.slot_identity",
            &format!("{}:{slot}", def.name),
        )
        .map_err(|e| e.to_string())?
    {
        return Ok(false);
    }
    let events = history(root, Some(&def.name), 500)?;
    if events
        .iter()
        .any(|event| event["type"] == "schedule.run_started" && event["data"]["slot_ms"] == slot)
    {
        return Ok(false);
    }
    if let Some(last) = events.iter().find(|event| {
        matches!(
            event["type"].as_str(),
            Some("schedule.run_started" | "schedule.run_finished")
        )
    }) {
        if last["type"] == "schedule.run_started" {
            let owner = last["data"]["pid"]
                .as_u64()
                .map(|pid| crate::shell::process_alive(pid as u32))
                .unwrap_or(false);
            let within_lease = last["data"]["started_at_ms"]
                .as_i64()
                .zip(last["data"]["timeout_secs"].as_i64())
                .is_some_and(|(at, timeout)| {
                    chrono::Local::now().timestamp_millis() - at < (timeout + 300) * 1000
                });
            if owner && within_lease {
                return Ok(false);
            }
            record(
                root,
                "schedule.run_finished",
                &json!({"name":def.name,"run_id":last["data"]["run_id"],"slot_ms":last["data"]["slot_ms"],"ok":false,"summary":"上一次运行中断；原槽位不会自动重跑","error":"executor_interrupted","finished_at_ms":chrono::Local::now().timestamp_millis()}),
            )?;
        }
    }
    record(
        root,
        "schedule.run_started",
        &json!({"name":def.name,"slot_ms":slot,"slot_identity":format!("{}:{slot}",def.name),"run_id":run_id,"run_session_id":format!("{}#{run_id}",kanzei_core::project_session_id(root)),"trigger":trigger,"host":def.host,"started_at_ms":chrono::Local::now().timestamp_millis(),"timeout_secs":def.timeout_secs,"pid":std::process::id()}),
    )?;
    Ok(true)
}
/// Newly enabled definitions are armed at observation time, with no historical backfill.
pub fn due(root: &Path, def: &ScheduleDef, now: i64) -> Result<Option<i64>, String> {
    let events = history(root, Some(&def.name), 500)?;
    let store =
        SessionStore::open(&kanzei_core::project_state_path(root)).map_err(|e| e.to_string())?;
    let mut controls = store
        .list_named_events(
            &history_id(root),
            Some(&def.name),
            Some("schedule.armed"),
            1,
        )
        .map_err(|e| e.to_string())?;
    controls.extend(
        store
            .list_named_events(
                &history_id(root),
                Some(&def.name),
                Some("schedule.disarmed"),
                1,
            )
            .map_err(|e| e.to_string())?,
    );
    let latest = controls
        .into_iter()
        .max_by_key(|event| event.sequence)
        .map(|event| json!({"type":event.event_type,"data":event.payload}));
    let latest_control = latest.as_ref();
    if !def.enabled {
        if latest_control.is_some_and(|event| event["type"] == "schedule.armed") {
            record(
                root,
                "schedule.disarmed",
                &json!({"name":def.name,"at_ms":now}),
            )?;
        }
        return Ok(None);
    }
    let armed = match latest_control.filter(|event| event["type"] == "schedule.armed") {
        Some(event)
            if std::fs::read_to_string(path(root, &def.name)?)
                .ok()
                .is_some_and(|text| {
                    event["data"]["revision"] == kanzei_base::content_hash(text.as_bytes())
                }) =>
        {
            event["data"]["at_ms"].as_i64().unwrap_or(now)
        }
        _ => {
            record(
                root,
                "schedule.armed",
                &json!({"name":def.name,"at_ms":now}),
            )?;
            return Ok(None);
        }
    };
    let slot = latest_slot(def, now)?;
    let last = events
        .iter()
        .filter(|event| {
            matches!(
                event["type"].as_str(),
                Some("schedule.run_started" | "schedule.skipped")
            )
        })
        .filter_map(|event| event["data"]["slot_ms"].as_i64())
        .max()
        .unwrap_or(armed);
    if slot <= last || slot <= armed {
        return Ok(None);
    }
    if def.catch_up == "skip" && now - slot > 60000 {
        record(
            root,
            "schedule.skipped",
            &json!({"name":def.name,"slot_ms":slot,"reason":"missed","at_ms":now}),
        )?;
        return Ok(None);
    }
    Ok(Some(slot))
}

#[cfg(test)]
mod tests {
    use super::*;
    const TEXT:&str="---\nname: check\nenabled: true\nwhen: 每 15 分钟\nsteps:\n  - run: 'echo \"quoted # literal\"' # comment\n  - prompt: |\n      inspect the output\n      preserve this line\nwriteback:\n  - notify\n  - memory_inbox\n  - file: reports/answer.md\n  - idea\n---\nHuman explanation\n";
    fn project() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "kz-schedule-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(path.join(".kanzei/schedules")).unwrap();
        std::fs::write(path.join(".kanzei/schedules/check.md"), TEXT).unwrap();
        path
    }
    #[test]
    fn definition_roundtrip_comments_blocks_and_line_diagnostics() {
        let def = parse(TEXT, "check").unwrap();
        assert_eq!(def.steps.len(), 2);
        assert_eq!(def.writeback.len(), 4);
        assert_eq!(def, parse(&TEXT.replace('\n', "\r\n"), "check").unwrap());
        assert_eq!(def, parse(&render(&def), "check").unwrap());
        assert_eq!(
            parse(&TEXT.replace("每 15 分钟", "每 7 分钟"), "check")
                .unwrap_err()
                .line,
            4
        );
        assert!(parse(&TEXT.replace("when:", "unknown:"), "check").is_err());
        assert_eq!(
            set_enabled(TEXT, false),
            TEXT.replace("enabled: true", "enabled: false")
        );
        assert!(path(Path::new("project"), "../escape").is_err());
    }
    #[test]
    fn newly_enabled_never_backfills_skip_and_once_are_distinct() {
        let root = project();
        let mut def = parse(TEXT, "check").unwrap();
        let now = When::parse(&def.when)
            .unwrap()
            .latest_ms(chrono::Local::now().timestamp_millis())
            + 120000;
        assert!(due(&root, &def, now).unwrap().is_none());
        assert_eq!(
            due(&root, &def, now + 900000).unwrap(),
            Some(now - 120000 + 900000)
        );
        def.catch_up = "skip".into();
        assert!(due(&root, &def, now + 900000).unwrap().is_none());
        assert!(history(&root, Some("check"), 10)
            .unwrap()
            .iter()
            .any(|event| event["type"] == "schedule.skipped"));
        let text = TEXT.replace("enabled: true", "enabled: false");
        std::fs::write(path(&root, "check").unwrap(), &text).unwrap();
        def.enabled = false;
        due(&root, &def, now + 1000000).unwrap();
        std::fs::write(path(&root, "check").unwrap(), TEXT).unwrap();
        def.enabled = true;
        assert!(due(&root, &def, now + 2000000).unwrap().is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn claim_is_persisted_once_and_live_owner_prevents_overlapping_runs() {
        let root = project();
        let def = parse(TEXT, "check").unwrap();
        assert!(claim(&root, &def, 1, "one", "manual").unwrap());
        assert!(!claim(&root, &def, 1, "duplicate", "manual").unwrap());
        assert!(!claim(&root, &def, 2, "overlap", "manual").unwrap());
        record(
            &root,
            "schedule.run_finished",
            &json!({"name":"check","run_id":"one","ok":true}),
        )
        .unwrap();
        assert!(claim(&root, &def, 2, "two", "manual").unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn slot_receipts_survive_history_window_and_remote_time_uses_definition_offset() {
        use chrono::TimeZone;
        let root = project();
        let mut def = parse(TEXT, "check").unwrap();
        assert!(claim(&root, &def, 1, "one", "manual").unwrap());
        record(
            &root,
            "schedule.run_finished",
            &json!({"name":"check","run_id":"one","ok":true}),
        )
        .unwrap();
        let store = SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
        for i in 0..510 {
            store
                .append_event(
                    &history_id(&root),
                    "schedule.note",
                    &json!({"name":"check","i":i}),
                )
                .unwrap();
        }
        assert!(!claim(&root, &def, 1, "duplicate-after-window", "manual").unwrap());
        def.when = "每天 09:00".into();
        def.utc_offset_minutes = 480;
        let utc = chrono::Utc.with_ymd_and_hms(2026, 10, 2, 1, 1, 0).unwrap();
        assert_eq!(
            latest_slot(&def, utc.timestamp_millis()).unwrap(),
            utc.timestamp_millis() - 60000
        );
        assert!(parse(
            &TEXT.replace("when:", "utc_offset_minutes: -2147483648\nwhen:"),
            "check"
        )
        .is_err());
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn real_command_run_isolated_history_file_writeback_and_duplicate_skipped() {
        let root = project();
        let mut def = parse(TEXT, "check").unwrap();
        def.steps = vec![Step::Run("echo SCHEDULE_ACTUAL_OUTPUT".into())];
        def.writeback = vec!["file:reports/result.md".into()];
        std::fs::write(path(&root, "check").unwrap(), render(&def)).unwrap();
        let outcome = execute(root.clone(), def.clone(), 1, "manual".into())
            .await
            .unwrap()
            .unwrap();
        assert!(outcome.ok, "{:?}", outcome.error);
        assert!(outcome.text.contains("SCHEDULE_ACTUAL_OUTPUT"));
        assert!(std::fs::read_to_string(root.join("reports/result.md"))
            .unwrap()
            .contains("SCHEDULE_ACTUAL_OUTPUT"));
        let store = SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
        assert!(store
            .list_events(&kanzei_core::project_session_id(&root), 0)
            .unwrap()
            .is_empty());
        assert!(store
            .latest_event(&outcome.run_session_id, "conversation.updated")
            .unwrap()
            .is_some());
        assert!(execute(root.clone(), def, 1, "manual".into())
            .await
            .unwrap()
            .is_none());
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn three_failed_real_commands_disable_definition() {
        let root = project();
        let mut def = parse(TEXT, "check").unwrap();
        def.steps = vec![Step::Run("exit 7".into())];
        def.writeback = vec![];
        std::fs::write(path(&root, "check").unwrap(), render(&def)).unwrap();
        for slot in 1..=3 {
            let outcome = execute(root.clone(), def.clone(), slot, "manual".into())
                .await
                .unwrap()
                .unwrap();
            assert!(!outcome.ok);
            assert!(outcome.error.unwrap().contains("7"));
        }
        assert!(
            !parse(
                &std::fs::read_to_string(path(&root, "check").unwrap()).unwrap(),
                "check"
            )
            .unwrap()
            .enabled
        );
        assert!(history(&root, Some("check"), 20)
            .unwrap()
            .iter()
            .any(|event| event["type"] == "schedule.auto_disabled"));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn stale_dispatch_does_not_run_a_replaced_definition() {
        let root = project();
        let def = parse(TEXT, "check").unwrap();
        std::fs::write(path(&root, "check").unwrap(), set_enabled(TEXT, false)).unwrap();
        assert!(!claim(&root, &def, 42, "old-dispatch", "timer").unwrap());
        assert!(history(&root, Some("check"), 10).unwrap().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn rearming_starts_a_new_failure_streak() {
        let root = project();
        let mut def = parse(TEXT, "check").unwrap();
        def.steps = vec![Step::Run("exit 7".into())];
        def.writeback = vec![];
        std::fs::write(path(&root, "check").unwrap(), render(&def)).unwrap();
        for slot in 1..=2 {
            execute(root.clone(), def.clone(), slot, "manual".into())
                .await
                .unwrap()
                .unwrap();
        }
        record(
            &root,
            "schedule.armed",
            &json!({"name":"check","at_ms":chrono::Local::now().timestamp_millis()}),
        )
        .unwrap();
        execute(root.clone(), def.clone(), 3, "manual".into())
            .await
            .unwrap()
            .unwrap();
        assert!(
            parse(
                &std::fs::read_to_string(path(&root, "check").unwrap()).unwrap(),
                "check"
            )
            .unwrap()
            .enabled
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn toggle_only_updates_the_parsed_frontmatter_enabled_field() {
        let text = "---\nname: check\n  enabled: true\nwhen: 每 15 分钟\nsteps:\n  - run: echo ready\n---\nenabled: example\n";
        assert!(parse(text, "check").unwrap().enabled);
        let disabled = set_enabled(text, false);
        assert!(!parse(&disabled, "check").unwrap().enabled);
        assert!(disabled.ends_with("enabled: example\n"));
        let missing = text.replace("  enabled: true\n", "");
        let enabled = set_enabled(&missing, true);
        assert!(parse(&enabled, "check").unwrap().enabled);
        assert!(enabled.ends_with("enabled: example\n"));
        let block = "---\nwhen: 每 15 分钟\nsteps:\n  - prompt: |\n      enabled: example\n  enabled: true\n---\n";
        let disabled = set_enabled(block, false);
        assert!(!parse(&disabled, "check").unwrap().enabled);
        assert_eq!(
            format!("{:?}", parse(&disabled, "check").unwrap().steps),
            format!("{:?}", parse(block, "check").unwrap().steps)
        );
    }
}
