//! Prepare an isolated Python environment on this machine or a registered SSH host.
use super::*;
use tokio::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ComputeSpec {
    /// Omit for unregistered local compute; otherwise use an existing ENV-... ID.
    pub environment_id: Option<String>,
    pub python: String,
    pub gpu_required: bool,
    #[serde(default)]
    pub min_vram_mb: u64,
    /// Optional nonempty requirements file relative to the topic; omit for standard-library-only code.
    pub requirements: Option<String>,
    #[serde(default)]
    /// Existing files relative to the topic, e.g. experiment.py, without the .kanzei/research/<topic>/ prefix.
    pub upload_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreparedCompute {
    pub spec: ComputeSpec,
    pub kind: String,
    pub host: Option<String>,
    pub workdir: String,
    pub python: String,
    pub snapshot: Value,
    pub prepared_at: i64,
}

fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

async fn output(mut cmd: Command) -> Result<String, String> {
    cmd.kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);
    let out = tokio::time::timeout(std::time::Duration::from_secs(600), cmd.output())
        .await
        .map_err(|_| "环境命令超过 10 分钟，请检查连接或依赖源")?
        .map_err(|e| e.to_string())?;
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if !out.status.success() {
        return Err(format!(
            "环境命令失败: {}",
            text.chars().take(5000).collect::<String>()
        ));
    }
    Ok(text)
}

async fn run(
    kind: &str,
    host: Option<&str>,
    dir: &str,
    program: &str,
    args: &[&str],
) -> Result<String, String> {
    let mut command = if kind == "ssh" {
        let mut cmd = Command::new("ssh");
        cmd.args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=15",
            "--",
            host.ok_or("缺少 SSH host")?,
        ]);
        cmd.arg(format!(
            "cd {} && {} {}",
            quote(dir),
            quote(program),
            args.iter().map(|s| quote(s)).collect::<Vec<_>>().join(" ")
        ));
        cmd
    } else {
        let mut cmd = Command::new(program);
        cmd.args(args).current_dir(dir);
        cmd
    };
    command.env("PIP_DISABLE_PIP_VERSION_CHECK", "1");
    output(command).await
}

const PROBE: &str = r#"import json,sys,platform,importlib.metadata
r={'python':sys.version,'executable':sys.executable,'platform':platform.platform(),'gpu_available':False,'gpus':[]}
try:
 import torch
 r.update(torch=torch.__version__,cuda=torch.version.cuda,gpu_available=torch.cuda.is_available())
 if r['gpu_available']:
  for i in range(torch.cuda.device_count()):
   p=torch.cuda.get_device_properties(i)
   free,total=torch.cuda.mem_get_info(i)
   r['gpus'].append({'name':p.name,'total_mb':total//1048576,'free_mb':free//1048576})
  x=torch.arange(256,device='cuda',dtype=torch.float32)
  r['gpu_smoke']=float((x*x).sum().cpu())
except ImportError: pass
r['packages']={d.metadata['Name']:d.version for d in importlib.metadata.distributions() if d.metadata['Name']}
print('KANZEI_COMPUTE='+json.dumps(r))
"#;

pub async fn prepare(
    root: &Path,
    topic: &str,
    revision: u64,
    spec: ComputeSpec,
) -> Result<Workflow, String> {
    let state = load(root, topic)?.ok_or("尚未启动")?;
    if state.revision != revision
        || !state.runnable()
        || !matches!(state.stage, Stage::Prepare | Stage::PlanFull)
    {
        return Err("阶段或版本已变化，请回读；环境准备仅在实验准备和完整实验方案阶段可用".into());
    }
    if spec.python.trim().is_empty() || spec.python.starts_with('-') {
        return Err("需要有效的 Python 可执行路径".into());
    }
    let dir = topic_dir(root, topic)?;
    for file in spec.upload_files.iter().chain(spec.requirements.iter()) {
        artifact(root, topic, file)?;
    }
    let environment = spec
        .environment_id
        .as_ref()
        .map(|id| crate::research_environment::load_environment(root, id))
        .transpose()?;
    if environment.as_ref().is_some_and(|e| e.status != "active") {
        return Err("环境不是 active".into());
    }
    let kind = environment
        .as_ref()
        .map(|e| e.kind.clone())
        .unwrap_or_else(|| "local".into());
    let host = environment
        .as_ref()
        .filter(|e| e.kind == "ssh")
        .map(|e| e.host.clone());
    let workdir = if kind == "ssh" {
        format!(
            "{}/{}",
            environment.as_ref().unwrap().workdir.trim_end_matches('/'),
            topic
        )
    } else {
        dir.to_string_lossy()
            .trim_start_matches("\\\\?\\")
            .to_string()
    };
    if let Some(host) = &host {
        let mut mkdir = Command::new("ssh");
        mkdir.args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=15",
            "--",
            host,
            &format!("mkdir -p {}", quote(&workdir)),
        ]);
        output(mkdir).await?;
        for file in spec.upload_files.iter().chain(spec.requirements.iter()) {
            let relative = file.replace('\\', "/");
            if let Some(parent) = Path::new(&relative)
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
            {
                run(
                    &kind,
                    Some(host),
                    &workdir,
                    "mkdir",
                    &["-p", &parent.to_string_lossy()],
                )
                .await?;
            }
            let mut scp = Command::new("scp");
            scp.args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=15", "--"])
                .arg(dir.join(file))
                .arg(format!("{host}:{workdir}/{relative}"));
            output(scp).await?;
        }
    }
    // Reuse large scientific wheels, but install requested dependencies only in the topic venv.
    let venv = ".auto-venv";
    run(
        &kind,
        host.as_deref(),
        &workdir,
        &spec.python,
        &["-m", "venv", "--system-site-packages", venv],
    )
    .await?;
    let python = format!(
        "{workdir}/{venv}/{}",
        if kind == "local" && cfg!(windows) {
            "Scripts/python.exe"
        } else {
            "bin/python"
        }
    );
    if let Some(req) = &spec.requirements {
        run(
            &kind,
            host.as_deref(),
            &workdir,
            &python,
            &[
                "-m",
                "pip",
                "install",
                "--disable-pip-version-check",
                "-r",
                req,
            ],
        )
        .await?;
    }
    let probe = run(&kind, host.as_deref(), &workdir, &python, &["-c", PROBE]).await?;
    let line = probe
        .lines()
        .find_map(|l| l.strip_prefix("KANZEI_COMPUTE="))
        .ok_or("环境探测没有返回事实")?;
    let snapshot: Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
    kanzei_base::atomic_file::write_atomic(&dir.join("compute-probe.json"), line)
        .map_err(|e| e.to_string())?;
    if spec.gpu_required
        && (snapshot["gpu_available"] != true
            || !snapshot["gpus"].as_array().is_some_and(|g| {
                g.iter()
                    .any(|g| g["free_mb"].as_u64().unwrap_or(0) >= spec.min_vram_mb)
            }))
    {
        return Err("GPU/CUDA 或可用显存不满足方案；探测事实已写 compute-probe.json，请选择其他环境或降低需求后重试".into());
    }
    let prepared = PreparedCompute {
        spec,
        kind,
        host,
        workdir,
        python,
        snapshot,
        prepared_at: now_ms(),
    };
    update(root, topic, revision, "prepare_compute", "agent", |state| {
        if !state.runnable() {
            return Err("研究已暂停".into());
        }
        state.compute = Some(prepared);
        Ok(())
    })
}
