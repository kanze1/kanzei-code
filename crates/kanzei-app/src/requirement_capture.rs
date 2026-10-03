//! Capture provenance is written by the app, never reconstructed by the model.
use kanzei_harness::{Tool, ToolCtx, ToolOutput};
use kanzei_tools::docstore::{requirement::RequirementSpec, DocStore, REQUIREMENTS};

pub(crate) fn save_source(root: &std::path::Path, original: &str) -> Result<String, String> {
    let hash = original.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    });
    let relative = format!(".kanzei/project/requirement-sources/{hash:016x}.md");
    let path = root.join(&relative);
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let _lock = kanzei_tools::atomic_file::lock_exclusive(&path).map_err(|e| e.to_string())?;
    if path.exists() {
        if std::fs::read_to_string(&path).map_err(|e| e.to_string())? != original {
            return Err("来源内容指纹冲突，原文未覆盖".into());
        }
    } else {
        kanzei_tools::atomic_file::write_atomic(&path, original).map_err(|e| e.to_string())?;
    }
    Ok(relative)
}

pub(crate) struct CaptureRequirementTool {
    pub inner: kanzei_tools::tracker::TrackerTool,
    pub source_reference: String,
}

#[async_trait::async_trait]
impl Tool for CaptureRequirementTool {
    fn name(&self) -> &'static str {
        self.inner.name()
    }
    fn description(&self) -> String {
        self.inner.description()
    }
    fn input_schema(&self) -> serde_json::Value {
        let mut schema = self.inner.input_schema();
        if let Some(properties) = schema["properties"].as_object_mut() {
            properties.retain(|key, _| {
                [
                    "action",
                    "id",
                    "title",
                    "requirement",
                    "reason",
                    "priority",
                    "tag",
                    "complexity",
                ]
                .contains(&key.as_str())
            });
        }
        schema["properties"]["action"]["enum"] = serde_json::json!(["list", "get", "add"]);
        schema["allOf"] = serde_json::json!([
            {"if":{"properties":{"action":{"const":"get"}}},"then":{"required":["id"]}},
            {"if":{"properties":{"action":{"const":"add"}}},"then":{"required":["title","requirement"]}}
        ]);
        schema
    }
    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        self.inner.resources(input)
    }
    async fn execute(&self, mut input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        if !matches!(input["action"].as_str(), Some("list" | "get" | "add")) {
            return ToolOutput::needs_correction(
                "TRACKER_INPUT_INVALID",
                "登记只允许 list/get/add；既有条目通过正式编辑入口修改",
            );
        }
        if input["action"] == "add" {
            // Older models may still emit legacy fields. Preserve their meaning but
            // never manufacture a missing statement from the title or original text.
            if input.get("requirement").is_none() {
                let fields = &input["fields"];
                let acceptance = fields["验收"]
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| vec![serde_json::json!({"text": s})])
                    .unwrap_or_default();
                input["requirement"] = serde_json::json!({
                    "statement": fields["内容"].as_str().unwrap_or(""), "acceptance": acceptance,
                });
                if let Some(fields) = input.get_mut("fields").and_then(|f| f.as_object_mut()) {
                    for key in ["内容", "验收", "来源", "原始描述", "发现记录"] {
                        fields.remove(key);
                    }
                }
            }
            if !input["requirement"].is_object() {
                return ToolOutput::needs_correction(
                    "TRACKER_INPUT_INVALID",
                    "requirement 必须是对象",
                );
            }
            input["requirement"]["source"] =
                serde_json::json!({"reference": self.source_reference});
            if let Ok(mut spec) =
                serde_json::from_value::<RequirementSpec>(input["requirement"].clone())
            {
                if spec.normalize(None).is_ok() {
                    let store = DocStore::open(&ctx.project_root, &REQUIREMENTS);
                    if let Ok(entries) = store.load() {
                        if let Some(existing) = entries.iter().find(|entry| {
                            entry.title == input["title"].as_str().unwrap_or("")
                                && RequirementSpec::from_entry(entry)
                                    .ok()
                                    .flatten()
                                    .is_some_and(|old| {
                                        old.source.reference == spec.source.reference
                                            && old.revision() == spec.revision()
                                    })
                        }) {
                            return ToolOutput::ok(format!(
                                "existing {} [{}] {}",
                                existing.id, existing.status, existing.title
                            ));
                        }
                    }
                }
            }
        }
        self.inner.execute(input, ctx).await
    }
}
