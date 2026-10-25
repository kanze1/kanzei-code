//! Registration binds provenance and creation/reuse receipts; idea completion submits checked references.
use kanzei_harness::{Tool, ToolCtx, ToolOutput};
use kanzei_memory::docstore::{requirement::RequirementSpec, DocStore};

#[derive(Clone)]
pub(crate) struct CaptureSession {
    pub reference: String,
    pub original: String,
    receipts: std::sync::Arc<std::sync::Mutex<std::collections::BTreeSet<String>>>,
}

#[cfg(test)]
#[path = "tracker_capture_tests.rs"]
mod tests;

pub(crate) fn save_source(
    root: &std::path::Path,
    original: &str,
) -> Result<CaptureSession, String> {
    let hash = kanzei_base::content_hash(original.as_bytes());
    let relative = format!(".kanzei/project/capture-sources/{hash}.md");
    let path = root.join(&relative);
    let _lock = kanzei_base::atomic_file::lock_exclusive(&path).map_err(|e| e.to_string())?;
    if path.exists() {
        if std::fs::read_to_string(&path).map_err(|e| e.to_string())? != original {
            return Err("来源内容指纹冲突，原文未覆盖".into());
        }
    } else {
        kanzei_base::atomic_file::write_atomic(&path, original).map_err(|e| e.to_string())?;
    }
    Ok(CaptureSession {
        reference: relative,
        original: original.into(),
        receipts: Default::default(),
    })
}

impl CaptureSession {
    pub(crate) fn ids(&self) -> Result<Vec<String>, String> {
        self.receipts
            .lock()
            .map(|ids| ids.iter().cloned().collect())
            .map_err(|error| format!("读取登记回执失败: {error}"))
    }
    fn record(&self, id: &str) -> Result<(), String> {
        self.receipts
            .lock()
            .map(|mut ids| {
                ids.insert(id.into());
            })
            .map_err(|error| format!("保存登记回执失败: {error}"))
    }
    fn existing(&self, entry: &kanzei_memory::docstore::Entry) -> ToolOutput {
        match self.record(&entry.id) {
            Ok(()) => ToolOutput::ok(format!(
                "existing {} [{}] {}",
                entry.id, entry.status, entry.title
            )),
            Err(error) => ToolOutput::error(error),
        }
    }
}

pub(crate) struct RegistrationTracker {
    pub inner: kanzei_tools::tracker::TrackerTool,
    pub source: CaptureSession,
}

impl RegistrationTracker {
    fn actions(&self) -> &'static [&'static str] {
        &["list", "get", "add", "reuse"]
    }
    fn fields(&self) -> &'static [&'static str] {
        match self.inner.kind.prefix {
            "R" => &[
                "action",
                "id",
                "title",
                "requirement",
                "reason",
                "priority",
                "tag",
                "complexity",
            ],
            "D" => &[
                "action", "id", "title", "fields", "reason", "severity", "priority", "tag",
            ],
            _ => &["action", "id"],
        }
    }
}

#[async_trait::async_trait]
impl Tool for RegistrationTracker {
    fn name(&self) -> &'static str {
        self.inner.name()
    }
    fn description(&self) -> String {
        format!("Read or register {} records; reuse records an existing id without changing it. The app binds original sources. This tool cannot modify existing records.", self.inner.noun)
    }
    fn input_schema(&self) -> serde_json::Value {
        let mut schema = self.inner.input_schema();
        schema["properties"]
            .as_object_mut()
            .expect("tracker schema properties")
            .retain(|key, _| self.fields().contains(&key.as_str()));
        schema["properties"]["action"]["enum"] = serde_json::json!(self.actions());
        schema["additionalProperties"] = serde_json::json!(false);
        let mut required = vec!["title"];
        if self.inner.kind.prefix == "R" {
            required.push("requirement");
        }
        if self.inner.kind.prefix == "D" {
            required.extend(["severity", "priority", "tag"]);
        }
        schema["allOf"] = serde_json::json!([
            {"if":{"properties":{"action":{"enum":["get","reuse"]}},"required":["action"]},"then":{"required":["id"]}},
            {"if":{"properties":{"action":{"const":"add"}},"required":["action"]},"then":{"required":required}}
        ]);
        schema
    }
    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        self.inner.resources(input)
    }

    async fn execute(&self, mut input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let Some(object) = input.as_object() else {
            return ToolOutput::needs_correction("TRACKER_INPUT_INVALID", "登记参数必须是对象");
        };
        if let Some(field) = object
            .keys()
            .find(|key| !self.fields().contains(&key.as_str()))
        {
            return ToolOutput::needs_correction(
                "TRACKER_INPUT_INVALID",
                format!("登记入口不接受字段 {field}"),
            );
        }
        if !input["action"]
            .as_str()
            .is_some_and(|action| self.actions().contains(&action))
        {
            return ToolOutput::needs_correction(
                "TRACKER_INPUT_INVALID",
                "该入口只读取来源或登记新条目，不能修改既有记录",
            );
        }
        if matches!(input["action"].as_str(), Some("list" | "get")) {
            return self.inner.execute(input, ctx).await;
        }
        let entries = match DocStore::open(&ctx.project_root, self.inner.kind).load() {
            Ok(entries) => entries,
            Err(error) => return ToolOutput::error(format!("读取登记记录失败: {error}")),
        };
        if input["action"] == "reuse" {
            let mut candidates = entries.clone();
            match DocStore::open(&ctx.project_root, self.inner.kind).load_archive() {
                Ok(archive) => candidates.extend(archive),
                Err(error) => return ToolOutput::error(format!("读取归档条目失败: {error}")),
            }
            return match input["id"]
                .as_str()
                .and_then(|id| candidates.iter().find(|entry| entry.id == id))
            {
                Some(entry) => self.source.existing(entry),
                None => {
                    ToolOutput::needs_correction("TRACKER_INPUT_INVALID", "复用的条目必须真实存在")
                }
            };
        }
        let previous: std::collections::BTreeSet<_> =
            entries.iter().map(|entry| entry.id.as_str()).collect();
        if self.inner.kind.prefix == "R" {
            if !input["requirement"].is_object() {
                return ToolOutput::needs_correction(
                    "TRACKER_INPUT_INVALID",
                    "requirement 必须是对象",
                );
            }
            input["requirement"]["source"] = serde_json::json!({"reference":self.source.reference});
            let mut spec =
                match serde_json::from_value::<RequirementSpec>(input["requirement"].clone()) {
                    Ok(spec) => spec,
                    Err(error) => {
                        return ToolOutput::needs_correction(
                            "TRACKER_INPUT_INVALID",
                            error.to_string(),
                        )
                    }
                };
            if let Err(error) = spec.normalize(None) {
                return ToolOutput::needs_correction("TRACKER_INPUT_INVALID", error);
            }
            for entry in entries
                .iter()
                .filter(|entry| entry.title == input["title"].as_str().unwrap_or("").trim())
            {
                let old = match RequirementSpec::from_entry(entry) {
                    Ok(old) => old,
                    Err(error) => {
                        return ToolOutput::error(format!("读取既有需求契约失败: {error}"))
                    }
                };
                if old.is_some_and(|old| {
                    old.source.reference == spec.source.reference
                        && old.revision() == spec.revision()
                }) {
                    return self.source.existing(entry);
                }
            }
        } else {
            if input.get("fields").is_none() {
                input["fields"] = serde_json::json!({});
            }
            if !input["fields"].is_object() {
                return ToolOutput::needs_correction("TRACKER_INPUT_INVALID", "fields 必须是对象");
            }
            input["fields"]["来源"] = serde_json::json!(self.source.reference);
            input["fields"]["原始描述"] = serde_json::json!(self.source.original);
            let fields: std::collections::BTreeMap<String, String> =
                match serde_json::from_value(input["fields"].clone()) {
                    Ok(fields) => fields,
                    Err(error) => {
                        return ToolOutput::needs_correction(
                            "TRACKER_INPUT_INVALID",
                            error.to_string(),
                        )
                    }
                };
            let existing = entries.iter().find(|entry| {
                entry.title == input["title"].as_str().unwrap_or("").trim()
                    && entry.severity.as_deref() == input["severity"].as_str()
                    && fields.iter().all(|(key, value)| {
                        entry
                            .fields
                            .iter()
                            .any(|(field, old)| field == key && old == value)
                    })
                    && [("标签", "tag"), ("优先级", "priority")]
                        .iter()
                        .all(|(field, argument)| {
                            input[*argument].as_str().is_some_and(|value| {
                                entry
                                    .fields
                                    .iter()
                                    .any(|(key, old)| key == field && old == value)
                            })
                        })
            });
            if let Some(entry) = existing {
                return self.source.existing(entry);
            }
        }
        let title = input["title"].as_str().unwrap_or("").trim().to_owned();
        let output = self.inner.execute(input, ctx).await;
        let after = match DocStore::open(&ctx.project_root, self.inner.kind).load() {
            Ok(entries) => entries,
            Err(error) => return ToolOutput::error(format!("读取登记结果失败: {error}")),
        };
        for entry in after
            .iter()
            .filter(|entry| !previous.contains(entry.id.as_str()) && entry.title == title)
        {
            if let Err(error) = self.source.record(&entry.id) {
                return ToolOutput::error(error);
            }
        }
        output
    }
}

/// The selected idea may only be read or explicitly completed with real references.
pub(crate) struct IdeaSplitTracker {
    pub inner: kanzei_tools::tracker::TrackerTool,
    pub idea: kanzei_memory::docstore::Entry,
    pub source: CaptureSession,
}

#[async_trait::async_trait]
impl Tool for IdeaSplitTracker {
    fn name(&self) -> &'static str {
        self.inner.name()
    }
    fn description(&self) -> String {
        "Read the selected idea. When its decomposition is complete, update status=split with the real requirement/defect refs; include all records registered during this task. Original idea text cannot be changed.".into()
    }
    fn input_schema(&self) -> serde_json::Value {
        let mut schema = self.inner.input_schema();
        schema["properties"]
            .as_object_mut()
            .expect("tracker properties")
            .retain(|key, _| ["action", "id", "status", "refs"].contains(&key.as_str()));
        schema["additionalProperties"] = serde_json::json!(false);
        schema["required"] = serde_json::json!(["action", "id"]);
        schema["properties"]["action"]["enum"] = serde_json::json!(["get", "update"]);
        schema["properties"]["id"] = serde_json::json!({"const":self.idea.id});
        schema["properties"]["status"] = serde_json::json!({"enum":["split"]});
        schema["allOf"] = serde_json::json!([{"if":{"properties":{"action":{"const":"update"}},"required":["action"]},"then":{"required":["status","refs"],"properties":{"refs":{"minItems":1,"uniqueItems":true}}}}]);
        schema
    }
    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        self.inner.resources(input)
    }
    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let Some(object) = input.as_object() else {
            return ToolOutput::needs_correction("TRACKER_INPUT_INVALID", "想法参数必须是对象");
        };
        if object
            .keys()
            .any(|key| !["action", "id", "status", "refs"].contains(&key.as_str()))
            || input["id"] != self.idea.id
        {
            return ToolOutput::needs_correction(
                "TRACKER_INPUT_INVALID",
                "拆解只能读取或完成所选想法，不能修改正文或其它想法",
            );
        }
        if input["action"] == "get" {
            return self.inner.execute(input, ctx).await;
        }
        if input["action"] != "update" || input["status"] != "split" {
            return ToolOutput::needs_correction(
                "TRACKER_INPUT_INVALID",
                "完成拆解使用 update、status=split 和真实产出 refs",
            );
        }
        let refs = match serde_json::from_value::<Vec<String>>(input["refs"].clone()) {
            Ok(refs) if !refs.is_empty() => refs,
            _ => {
                return ToolOutput::needs_correction(
                    "TRACKER_INPUT_INVALID",
                    "refs 必须包含真实需求或缺陷编号",
                )
            }
        };
        if refs.iter().collect::<std::collections::BTreeSet<_>>().len() != refs.len() {
            return ToolOutput::needs_correction("TRACKER_INPUT_INVALID", "refs 不能重复");
        }
        let registered = match self.source.ids() {
            Ok(ids) => ids,
            Err(error) => return ToolOutput::error(error),
        };
        if !registered.iter().all(|id| refs.contains(id)) {
            return ToolOutput::needs_correction(
                "TRACKER_INPUT_INVALID",
                format!("完成引用遗漏本次已登记条目: {}", registered.join(" ")),
            );
        }
        let current = match DocStore::open(&ctx.project_root, self.inner.kind).load() {
            Ok(entries) => entries,
            Err(error) => return ToolOutput::error(format!("读取原想法失败: {error}")),
        };
        if current.iter().find(|entry| entry.id == self.idea.id) != Some(&self.idea) {
            return ToolOutput::needs_correction(
                "TRACKER_INPUT_INVALID",
                "原想法已改变，本次拆解不能覆盖新内容",
            );
        }
        self.inner.execute(input, ctx).await
    }
}
