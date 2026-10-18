//! Human-selected Windows instance capabilities; models cannot enumerate or select windows.
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kanzei_harness::{ToolCtx, ToolImage, ToolOutput};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use crate::MutexPoisonExt;

static ACTION: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(|| tokio::sync::Mutex::new(()));
static CANDIDATES: LazyLock<Mutex<HashMap<String, Value>>> = LazyLock::new(Mutex::default);
static SELECTED: LazyLock<Mutex<HashMap<String, Selection>>> = LazyLock::new(Mutex::default);

#[derive(Clone)]
struct Selection {
    id: String,
    window: Value,
    snapshot: Option<(String, HashMap<String, Value>)>,
    staged: Vec<std::sync::Arc<tempfile::TempDir>>,
}

pub(super) fn identity() -> String {
    static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    format!(
        "native_{}_{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}

pub(crate) fn selected(process: Option<&str>) -> Option<Value> {
    let selections = SELECTED.lock_or_recover();
    let selection = selections.get(process?)?;
    Some(json!({"id":selection.id,"title":selection.window["title"]}))
}

async fn helper(request: Value) -> Result<Value, String> {
    helper_owned(request, None, &CancellationToken::new()).await
}
async fn helper_owned(
    mut request: Value,
    lease: Option<&crate::native_control::Lease>,
    connection: &CancellationToken,
) -> Result<Value, String> {
    request["input_tag"] = json!(crate::native_control::INPUT_TAG);
    let program = std::env::var_os("ProgramFiles")
        .map(std::path::PathBuf::from)
        .ok_or("Windows ProgramFiles is unavailable")?
        .join("PowerShell/7/pwsh.exe");
    if !program.is_file() {
        return Err("原生窗口工具需要安装 PowerShell 7".into());
    }
    let source = include_str!("../../../scripts/desktop-helper.ps1");
    let directory = program
        .parent()
        .ok_or("PowerShell installation directory missing")?;
    let mut command = tokio::process::Command::new(&program);
    command
        .current_dir(directory)
        .env("PSModulePath", directory.join("Modules"));
    command.args(["-NoProfile", "-NonInteractive", "-Mta", "-Command"])
        .arg("[Console]::InputEncoding=[Text.UTF8Encoding]::new($false); [Console]::OutputEncoding=[Text.UTF8Encoding]::new($false); $envelope=[Console]::In.ReadLine()|ConvertFrom-Json; & ([ScriptBlock]::Create([string]$envelope.script)) -RequestJson $envelope.request_json")
        .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped()).kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    if let Some(lease) = lease {
        crate::native_control::helper_pid(lease, child.id());
    }
    let mut input = serde_json::to_vec(&json!({"script":source,"request_json":serde_json::to_string(&request).map_err(|error|error.to_string())?}))
        .map_err(|error| error.to_string())?;
    input.push(b'\n');
    let cancelled = lease.map(|lease| lease.cancel.clone()).unwrap_or_default();
    let response = helper_exchange(&mut child, &input, &cancelled, connection).await;
    if child
        .try_wait()
        .map_err(|error| error.to_string())?
        .is_some()
    {
        if let Some(lease) = lease {
            crate::native_control::helper_pid(lease, None);
        }
    }
    let (status, stdout, stderr) = response?;
    if stdout.len() > 8 * 1024 * 1024 {
        return Err("Native window response exceeds 8 MiB".into());
    }
    let value: Value = serde_json::from_slice(&stdout).map_err(|error| {
        format!(
            "Native window response invalid: {error}; {}",
            String::from_utf8_lossy(&stderr)
                .chars()
                .take(2048)
                .collect::<String>()
        )
    })?;
    if !status.success() || value["ok"] != true {
        return Err(value["error"]
            .as_str()
            .unwrap_or("Native window helper failed")
            .into());
    }
    Ok(value)
}

async fn helper_exchange(
    child: &mut tokio::process::Child,
    input: &[u8],
    cancelled: &CancellationToken,
    connection: &CancellationToken,
) -> Result<(std::process::ExitStatus, Vec<u8>, Vec<u8>), String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let mut stdin = child.stdin.take().ok_or("Desktop helper stdin missing")?;
    let stdout = child.stdout.take().ok_or("Desktop helper stdout missing")?;
    let stderr = child.stderr.take().ok_or("Desktop helper stderr missing")?;
    let reading = tokio::spawn(async move {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let mut stdout = stdout.take(8 * 1024 * 1024 + 1);
        let mut stderr = stderr.take(4096);
        let (outcome, error) =
            tokio::join!(stdout.read_to_end(&mut out), stderr.read_to_end(&mut err));
        outcome.map_err(|error| error.to_string())?;
        error.map_err(|error| error.to_string())?;
        Ok::<_, String>((out, err))
    });
    // Cancellation owns the entire exchange, including a full stdin pipe while
    // the helper starts. No request bytes may remain queued after stop.
    let waited = tokio::select! {
            biased;
            _ = cancelled.cancelled() => Err("原生动作已取消".into()),
            _ = connection.cancelled() => Err("原生调用方已断开".into()),
            status = tokio::time::timeout_at(deadline, async {
                stdin.write_all(input).await.map_err(|error|error.to_string())?;
                drop(stdin);
                child.wait().await.map_err(|error|error.to_string())
            }) => status.map_err(|_|"原生窗口动作超时；未重试".to_string()).and_then(|value|value),
    };
    if waited.is_err()
        && child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_none()
    {
        child.kill().await.map_err(|error| error.to_string())?;
    }
    // A stop receipt cannot precede OS exit and pipe-reader completion.
    let drained = reading
        .await
        .map_err(|error| error.to_string())
        .and_then(|value| value);
    let (stdout, stderr) = drained?;
    let status = waited?;
    Ok((status, stdout, stderr))
}

#[tauri::command]
pub(crate) async fn native_desktop_windows() -> Result<Value, String> {
    let value = helper(json!({"action":"list"})).await?;
    let mut candidates = CANDIDATES.lock_or_recover();
    candidates.clear();
    let mut rows = Vec::new();
    for window in value["windows"].as_array().ok_or("Window list missing")? {
        if window["pid"].as_u64() == Some(std::process::id() as u64) {
            continue;
        }
        let id = identity();
        candidates.insert(id.clone(), window.clone());
        rows.push(json!({"id":id,"title":window["title"],"application":std::path::Path::new(window["image"].as_str().unwrap_or("")).file_name().unwrap_or_default().to_string_lossy()}));
    }
    Ok(json!({"windows":rows,"unavailable":value["unavailable"]}))
}

#[tauri::command]
pub(crate) async fn native_desktop_select(
    process_id: String,
    candidate_id: Option<String>,
) -> Result<Option<Value>, String> {
    if process_id.trim().is_empty() {
        return Err("请先选择对话".into());
    }
    release(&process_id, None, None).await?;
    let _action = ACTION.lock().await;
    let Some(id) = candidate_id else {
        SELECTED.lock_or_recover().remove(&process_id);
        return Ok(None);
    };
    let window = CANDIDATES
        .lock_or_recover()
        .get(&id)
        .cloned()
        .ok_or("窗口列表已变化，请重新选择")?;
    // Revalidate the OS instance before publishing a conversation capability.
    helper(json!({"action":"inspect","window":window})).await?;
    SELECTED.lock_or_recover().insert(
        process_id.clone(),
        Selection {
            id: identity(),
            window,
            snapshot: None,
            staged: vec![],
        },
    );
    Ok(selected(Some(&process_id)))
}

#[tauri::command]
pub(crate) fn native_desktop_get(process_id: String) -> Option<Value> {
    selected(Some(&process_id))
}
pub(crate) fn revoke(process: &str, window_id: &str) {
    let mut selected = SELECTED.lock_or_recover();
    if selected
        .get(process)
        .is_some_and(|selection| selection.id == window_id)
    {
        selected.remove(process);
    }
}
pub(crate) async fn release(
    process: &str,
    run: Option<&str>,
    window: Option<&str>,
) -> Result<(), String> {
    if let Some(window) = window {
        revoke(process, window);
    }
    if let Some(owner) = crate::native_control::cancel_owner(process, run, window)? {
        let _drained = ACTION.lock().await;
        crate::native_control::confirm_stopped(&owner.id)?;
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    action: String,
    window_id: Option<String>,
    snapshot_id: Option<String>,
    element_id: Option<String>,
    text: Option<String>,
    x: Option<f64>,
    y: Option<f64>,
    key: Option<String>,
    file: Option<String>,
}

pub(crate) fn schema() -> Value {
    json!({"type":"object","additionalProperties":false,"required":["action"],"properties":{
        "action":{"type":"string","enum":["inspect","screenshot","fill","invoke","toggle","select","click","type","key","choose_file"]},
        "window_id":{"type":"string","description":"Copy window_id from the latest observation."},
        "snapshot_id":{"type":"string","description":"Copy snapshot_id exactly from the latest observation. One mutation consumes it; inspect again afterwards."},
        "element_id":{"type":"string","description":"Use an observed control id for UIA actions. Use snapshot.surface.id for screenshot clicks and typing or keys at the observed current focus, including custom-drawn apps."},
        "text":{"type":"string","maxLength":4096,"description":"fill replaces a UIA value; type sends literal Unicode text to the surface's observed focus. Use key for control characters."},
        "x":{"type":"number","minimum":0,"description":"For a surface, X in the returned screenshot pixels; for a control, X relative to that control's native bounds."},
        "y":{"type":"number","minimum":0,"description":"For a surface, Y in the returned screenshot pixels; for a control, Y relative to that control's native bounds."},
        "key":{"type":"string","maxLength":64,"description":"Named key (Enter, Tab, Escape, Backspace, Delete, arrows, Home, End, Space, PageUp, PageDown), letter or digit, optionally prefixed by Control/Alt/Shift modifiers, e.g. Control+A or Shift+Tab."},"file":{"type":"string"}}})
}

pub(crate) async fn execute(
    input: Value,
    ctx: &ToolCtx,
    planned: &str,
    connection: &CancellationToken,
) -> ToolOutput {
    let output = match execute_checked(input, ctx, planned, connection).await {
        Ok(output) => output,
        Err(output) => *output,
    };
    output.with_external_source(
        kanzei_harness::ExternalSourceKind::Desktop,
        format!("window:{planned}"),
    )
}

async fn execute_checked(
    input: Value,
    ctx: &ToolCtx,
    planned: &str,
    connection: &CancellationToken,
) -> Result<ToolOutput, Box<ToolOutput>> {
    let rejected = |message: &str| {
        Box::new(ToolOutput::needs_correction(
            "NATIVE_WINDOW_TARGET_CHANGED",
            message,
        ))
    };
    let parsed: Input = serde_json::from_value(input.clone()).map_err(|error| {
        ToolOutput::needs_correction("INVALID_NATIVE_ACTION", error.to_string())
    })?;
    if ![
        "inspect",
        "screenshot",
        "fill",
        "invoke",
        "toggle",
        "select",
        "click",
        "type",
        "key",
        "choose_file",
    ]
    .contains(&parsed.action.as_str())
    {
        return Err(Box::new(ToolOutput::needs_correction(
            "INVALID_NATIVE_ACTION",
            "Unknown native action",
        )));
    }
    if matches!(parsed.action.as_str(), "fill" | "type") && parsed.text.is_none()
        || parsed.action == "key" && parsed.key.is_none()
        || parsed.action == "click" && (parsed.x.is_none() || parsed.y.is_none())
    {
        return Err(Box::new(ToolOutput::needs_correction(
            "INVALID_NATIVE_ACTION",
            "This action requires its explicit value, key or coordinates",
        )));
    }
    let owner = ctx
        .process_id
        .as_deref()
        .ok_or_else(|| rejected("原生窗口操作需要明确对话"))?;
    let _action = ACTION.lock().await;
    let selection = SELECTED
        .lock_or_recover()
        .get(owner)
        .cloned()
        .ok_or_else(|| rejected("请先在更多菜单选择原生窗口"))?;
    if selection.id != planned
        || parsed
            .window_id
            .as_deref()
            .is_some_and(|id| id != selection.id)
    {
        return Err(rejected("原生窗口选择已变化；旧动作未转移到新窗口"));
    }
    let observation = matches!(parsed.action.as_str(), "inspect" | "screenshot");
    let mut request = json!({"action":parsed.action,"window":selection.window});
    let mut target = json!({"name":selection.window["title"]});
    if !observation {
        if parsed.window_id.as_deref() != Some(selection.id.as_str()) {
            return Err(rejected("动作必须引用已观察窗口"));
        }
        let (snapshot_id, elements) = selection
            .snapshot
            .as_ref()
            .ok_or_else(|| rejected("请先 inspect 目标窗口"))?;
        if parsed.snapshot_id.as_ref() != Some(snapshot_id) {
            return Err(rejected("可访问性快照已变化，请检查后再操作"));
        }
        let reference = parsed
            .element_id
            .as_ref()
            .and_then(|id| elements.get(id))
            .ok_or_else(|| rejected("控件不属于当前快照"))?;
        request["runtime_id"] = reference["runtime_id"].clone();
        request["signature"] = reference["signature"].clone();
        request["target"] = reference["target"].clone();
        if reference["target"] == "surface" {
            request["focus"] = reference["focus"].clone();
        }
        target = reference.clone();
    }
    if let Some(text) = parsed.text {
        if text.chars().count() > 4096 {
            return Err(Box::new(ToolOutput::needs_correction(
                "INVALID_NATIVE_ACTION",
                "Text exceeds 4096 characters",
            )));
        }
        request["text"] = json!(text);
    }
    for (axis, extent, coordinate) in [("x", "width", parsed.x), ("y", "height", parsed.y)] {
        if let Some(mut coordinate) = coordinate {
            if target["target"] == "surface" {
                let captured = target[extent]
                    .as_f64()
                    .filter(|value| *value > 0.0)
                    .ok_or_else(|| ToolOutput::error("Native screenshot dimensions missing"))?;
                let native = target["bounds"][extent]
                    .as_f64()
                    .filter(|value| *value > 0.0)
                    .ok_or_else(|| ToolOutput::error("Native window dimensions missing"))?;
                if coordinate < 0.0 || coordinate >= captured {
                    return Err(Box::new(ToolOutput::needs_correction(
                        "NATIVE_POINTER_OUTSIDE_SCREENSHOT",
                        "Pointer is outside the observed screenshot bounds",
                    )));
                }
                coordinate *= native / captured;
            }
            request[axis] = json!(coordinate);
        }
    }
    if let Some(key) = parsed.key {
        request["key"] = json!(key);
    }
    if parsed.action == "choose_file" {
        use kanzei_harness::permission::{execution_rules, Effect, FileResource, ScopedFile};
        let path = parsed.file.as_deref().ok_or_else(|| {
            ToolOutput::needs_correction("INVALID_NATIVE_ACTION", "file is required")
        })?;
        let file = ScopedFile::resolve(&ctx.cwd, &ctx.project_root, std::path::Path::new(path))
            .map_err(|error| {
                ToolOutput::needs_correction("FILESYSTEM_PATH_REJECTED", error.to_string())
            })?;
        let resource = FileResource::resolve(&ctx.cwd, std::path::Path::new(path))
            .map_err(|error| ToolOutput::error(error.to_string()))?;
        if resource.sensitivity().is_some()
            && !execution_rules().is_some_and(|rules| {
                rules.evaluate_file_with_rule("desktop", &resource).0 == Effect::Allow
            })
        {
            return Err(Box::new(ToolOutput::needs_confirmation(
                "SENSITIVE_FILE_APPROVAL_REQUIRED",
                "原生文件选择需要该文件的精确授权",
            )));
        }
        if !file
            .metadata()
            .map_err(|error| ToolOutput::error(error.to_string()))?
            .is_file()
        {
            return Err(Box::new(ToolOutput::needs_correction(
                "INVALID_NATIVE_FILE",
                "请选择项目范围内的普通文件",
            )));
        }
        use std::io::Read;
        let mut bytes = Vec::new();
        file.open()
            .map_err(|error| ToolOutput::error(error.to_string()))?
            .take(8 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| ToolOutput::error(error.to_string()))?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err(Box::new(ToolOutput::needs_correction(
                "NATIVE_FILE_TOO_LARGE",
                "Native file choice is limited to 8 MiB",
            )));
        }
        let staged = std::sync::Arc::new(
            tempfile::tempdir().map_err(|error| ToolOutput::error(error.to_string()))?,
        );
        let name = file
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| ToolOutput::error("File name is not valid UTF-8"))?;
        let copy = ScopedFile::resolve(staged.path(), staged.path(), std::path::Path::new(name))
            .map_err(|error| ToolOutput::error(error.to_string()))?;
        copy.write_atomic(&bytes, || Ok(()))
            .map_err(|error| ToolOutput::error(error.to_string()))?;
        request["file"] = json!(copy.path());
        SELECTED
            .lock_or_recover()
            .get_mut(owner)
            .ok_or_else(|| rejected("Window disconnected"))?
            .staged
            .push(staged);
    }
    if !observation {
        if let Some(current) = SELECTED.lock_or_recover().get_mut(owner) {
            current.snapshot = None;
        }
    }
    if connection.is_cancelled() {
        return Err(Box::new(ToolOutput::failed(
            "NATIVE_ACTION_CANCELLED",
            "原生调用方已取消",
        )));
    }
    let lease = crate::native_control::begin(
        ctx,
        &selection.id,
        selection.window["title"].as_str().unwrap_or(""),
    )
    .await
    .map_err(|error| ToolOutput::failed("NATIVE_CONTROL_UNAVAILABLE", error))?;
    let point = request.get("x").map(|x| json!({"x":x,"y":request["y"]}));
    let action_id = crate::native_control::start_action(&lease, &parsed.action, &target, point)
        .map_err(|error| ToolOutput::failed("NATIVE_ACTION_CANCELLED", error))?;
    let result = helper_owned(request, Some(&lease), connection).await;
    let interrupted = lease.cancel.is_cancelled() || connection.is_cancelled();
    let mut value = match result {
        Ok(value) if !interrupted => value,
        outcome => {
            crate::native_control::finish_action(
                &lease,
                action_id,
                if interrupted { "cancelled" } else { "failed" },
            );
            if interrupted {
                crate::native_control::cancel(&lease.id)
                    .map_err(|error| ToolOutput::failed("NATIVE_STOP_UNCONFIRMED", error))?;
            }
            return Err(Box::new(ToolOutput::failed(
                if interrupted {
                    "NATIVE_ACTION_CANCELLED"
                } else {
                    "NATIVE_ACTION_UNVERIFIED"
                },
                outcome
                    .err()
                    .unwrap_or_else(|| "动作期间已停止；结果需要重新观察".into()),
            )));
        }
    };
    if observation {
        crate::native_control::observed_bounds(
            &lease,
            action_id,
            value["snapshot"]["bounds"].clone(),
        );
    }
    crate::native_control::finish_action(
        &lease,
        action_id,
        if observation {
            "observed"
        } else {
            "dispatched"
        },
    );
    if !observation {
        if let Some(current) = SELECTED.lock_or_recover().get_mut(owner) {
            current.snapshot = None;
        }
        value["window_id"] = json!(selection.id);
        return Ok(ToolOutput::ok(value.to_string()).with_external_source(
            kanzei_harness::ExternalSourceKind::Desktop,
            format!("window:{}", selection.id),
        ));
    }
    let mut refs = HashMap::new();
    let surface = value["snapshot"]["surface"]
        .as_object_mut()
        .ok_or_else(|| ToolOutput::error("Native screenshot surface missing"))?;
    if !surface.get("runtime_id").is_some_and(Value::is_string)
        || !surface.get("signature").is_some_and(Value::is_string)
    {
        return Err(Box::new(ToolOutput::error(
            "Native screenshot surface identity missing",
        )));
    }
    let mut reference = Value::Object(surface.clone());
    reference["target"] = json!("surface");
    reference["name"] = json!("Window screenshot surface");
    refs.insert("surface".into(), reference);
    for field in ["runtime_id", "signature", "focus"] {
        surface.remove(field);
    }
    surface.insert("id".into(), json!("surface"));
    surface.insert("coordinate_space".into(), json!("screenshot_pixels"));
    for (index, element) in value["snapshot"]["elements"]
        .as_array_mut()
        .ok_or_else(|| ToolOutput::error("Native snapshot missing"))?
        .iter_mut()
        .enumerate()
    {
        let id = format!("element_{index}");
        if !element["runtime_id"].is_string() || !element["signature"].is_string() {
            return Err(Box::new(ToolOutput::error(
                "Native element identity missing",
            )));
        }
        refs.insert(
            id.clone(),
            json!({"target":"element","runtime_id":element["runtime_id"],"signature":element["signature"],"name":element["name"],"bounds":element["bounds"]}),
        );
        element.as_object_mut().unwrap().remove("runtime_id");
        element.as_object_mut().unwrap().remove("signature");
        element["id"] = json!(id);
    }
    let snapshot_id = identity();
    let image = value
        .as_object_mut()
        .unwrap()
        .remove("image")
        .ok_or_else(|| ToolOutput::error("Native capture missing"))?;
    value.as_object_mut().unwrap().remove("window");
    value["window_id"] = json!(selection.id);
    value["snapshot_id"] = json!(snapshot_id);
    value["scope"]=json!("Only the human-selected window and same-process owned dialogs. A dispatched or pending action is not proof of task success; verify the resulting UI. No OS sandbox is claimed.");
    if let Some(current) = SELECTED.lock_or_recover().get_mut(owner) {
        if current.id != selection.id {
            return Err(rejected("窗口选择已变化"));
        }
        current.snapshot = Some((snapshot_id, refs));
    }
    let mut out = ToolOutput::ok(value.to_string()).with_external_source(
        kanzei_harness::ExternalSourceKind::Desktop,
        format!("window:{}", selection.id),
    );
    out.images.push(ToolImage {
        media_type: "image/png".into(),
        data: image["data"]
            .as_str()
            .ok_or_else(|| ToolOutput::error("Native PNG missing"))?
            .into(),
    });
    Ok(out)
}

#[derive(Default)]
pub(crate) struct DesktopTool {
    plans: Mutex<HashMap<String, Option<String>>>,
}
impl DesktopTool {
    fn plan_key(input: &Value, ctx: &ToolCtx) -> String {
        kanzei_core::store::stable_json_hash(&json!([
            ctx.project_root,
            ctx.process_id,
            ctx.run_id,
            input
        ]))
    }
}
#[async_trait::async_trait]
impl kanzei_harness::Tool for DesktopTool {
    fn name(&self) -> &'static str {
        "desktop"
    }
    fn description(&self) -> String {
        "Observe and operate a human-selected Windows window. inspect returns both a screenshot surface and UIA controls. Copy its exact window_id and snapshot_id for one action. For custom-drawn apps or sparse UIA trees, click snapshot.surface.id at screenshot pixel coordinates, inspect the focused result, then type literal Unicode text or send key (Enter, Control+A, Shift+Tab, etc.) to the same surface. Surface input preserves the observed focus; title animation does not replace window identity. For accessible controls, fill replaces their value; invoke clicks the live control center; toggle/select use UIA; control click coordinates are relative to native element bounds. Every mutation consumes the snapshot: inspect again to verify and obtain the next reference. Human window selection is required in this conversation's More menu; you cannot enumerate or select windows. choose_file uses a scoped project file (max 8 MiB); sensitive files need exact consent. Password controls and agent/approval windows are blocked. Dispatch is not task completion: never blindly repeat a pending action.".into()
    }
    fn input_schema(&self) -> Value {
        schema()
    }
    fn resources(&self, _: &Value) -> Vec<String> {
        vec!["window:unselected".into()]
    }
    fn permission_resources(
        &self,
        input: &Value,
        ctx: &ToolCtx,
    ) -> Vec<kanzei_harness::permission::Resource> {
        use kanzei_harness::permission::Resource;
        let mut plans = self.plans.lock_or_recover();
        let planned = plans
            .entry(Self::plan_key(input, ctx))
            .or_insert_with(|| crate::desktop_bridge::native_target(ctx));
        let mut resources = vec![Resource::Literal(format!(
            "window:{}",
            planned.as_deref().unwrap_or("unselected")
        ))];
        if input["action"] == "choose_file" {
            if let Some(file) = input["file"].as_str() {
                resources.push(Resource::File(file.into()));
            }
        }
        resources
    }
    fn concurrency(&self, _: &Value, _: &ToolCtx) -> kanzei_harness::ToolConcurrency {
        kanzei_harness::ToolConcurrency::Exclusive
    }
    async fn execute(&self, input: Value, ctx: &ToolCtx) -> ToolOutput {
        let planned = self
            .plans
            .lock_or_recover()
            .remove(&Self::plan_key(&input, ctx))
            .flatten();
        crate::desktop_bridge::native_action(input, ctx, planned).await
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancellation_interrupts_a_blocked_request_write_and_reaps_the_child() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let script = format!(
            "$signal=[Net.Sockets.TcpClient]::new('127.0.0.1',{});$signal.GetStream().WriteByte(1);[void][Threading.ManualResetEvent]::new($false).WaitOne()",
            listener.local_addr().unwrap().port()
        );
        let program = std::path::PathBuf::from(std::env::var_os("ProgramFiles").unwrap())
            .join("PowerShell/7/pwsh.exe");
        let mut child = tokio::process::Command::new(program)
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .creation_flags(0x0800_0000)
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let cancelled = CancellationToken::new();
        let connection = CancellationToken::new();
        // The child never reads stdin. This exceeds the OS pipe capacity, so
        // the exchange stays inside write_all until cancellation owns cleanup.
        let request = vec![b'x'; 1024 * 1024];
        let outcome = tokio::time::timeout(Duration::from_secs(10), async {
            let (response, ()) = tokio::join!(
                biased;
                helper_exchange(&mut child, &request, &cancelled, &connection),
                async {
                    let (mut signal, _) = listener.accept().await.unwrap();
                    signal.read_exact(&mut [0u8; 1]).await.unwrap();
                    cancelled.cancel();
                }
            );
            response
        })
        .await
        .unwrap();
        assert_eq!(outcome.unwrap_err(), "原生动作已取消");
        assert!(
            child.try_wait().unwrap().is_some(),
            "cancel acknowledgement must follow OS process exit"
        );
    }
}
