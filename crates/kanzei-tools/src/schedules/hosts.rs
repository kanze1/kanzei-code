//! Explicit host registration; called only when a user saves/toggles a hosted definition.
use super::*;
use sha2::{Digest, Sha256};
fn task_name(root: &Path, name: &str) -> String {
    format!(
        "Kanzei-{}-{name}",
        &format!(
            "{:x}",
            Sha256::digest(kanzei_core::project_session_id(root))
        )[..12]
    )
}
fn sh(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
#[cfg(windows)]
fn ps(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}
async fn checked(program: &str, args: &[String]) -> Result<String, String> {
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    crate::hide_console_async(&mut command);
    let output = tokio::time::timeout(Duration::from_secs(30), command.output())
        .await
        .map_err(|_| format!("{program} 超时"))?
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "{program} 失败：{}",
            String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(2000)
                .collect::<String>()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
#[cfg(windows)]
pub async fn system(root: &Path, def: &ScheduleDef, enabled: bool) -> Result<(), String> {
    use base64::Engine;
    let task = ps(&task_name(root, &def.name));
    let script = if !enabled {
        format!("if (Get-ScheduledTask -TaskName {task} -ErrorAction SilentlyContinue) {{ Unregister-ScheduledTask -TaskName {task} -Confirm:$false -ErrorAction Stop }}")
    } else {
        let beside = std::env::current_exe()
            .ok()
            .map(|exe| exe.with_file_name("kz.exe"))
            .filter(|exe| exe.is_file());
        let cli = beside
            .or_else(|| crate::shell::find_executable("kz.exe", &crate::shell::fresh_path()))
            .ok_or("找不到 kz.exe")?;
        let host_log = root
            .join(".kanzei/artifacts/schedules/runs")
            .join(format!("{}-host.log", def.name));
        let invoke=format!("$ErrorActionPreference='Stop'; [IO.Directory]::CreateDirectory({}) | Out-Null; try {{ & {} schedule run {} --project-root {} --trigger system *> {}; exit $LASTEXITCODE }} catch {{ [IO.File]::WriteAllText({},($_ | Out-String)); exit 1 }}",ps(&kanzei_base::path_form::simplify(host_log.parent().unwrap()).display().to_string()),ps(&cli.display().to_string()),ps(&def.name),ps(&kanzei_base::path_form::simplify(root).display().to_string()),ps(&kanzei_base::path_form::simplify(&host_log).display().to_string()),ps(&kanzei_base::path_form::simplify(&host_log).display().to_string()));
        let powershell =
            crate::shell::find_executable("powershell.exe", &crate::shell::fresh_path())
                .ok_or("找不到 Windows PowerShell")?;
        let encoded = base64::engine::general_purpose::STANDARD.encode(
            invoke
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        );
        // PowerShell -WindowStyle Hidden can exit before its script starts when
        // the host supplies no console. WSH starts the same command hidden.
        let launcher = host_log.with_extension("vbs");
        let command = format!(
            "\"{}\" -NoProfile -NonInteractive -EncodedCommand {encoded}",
            powershell.display()
        );
        let vbs = format!(
            "Set shell = CreateObject(\"WScript.Shell\")\r\ncode = shell.Run(\"{}\", 0, True)\r\nWScript.Quit code\r\n",
            command.replace('"', "\"\"")
        );
        kanzei_base::atomic_file::write_atomic(&launcher, &vbs).map_err(|e| e.to_string())?;
        let wscript = crate::shell::find_executable("wscript.exe", &crate::shell::fresh_path())
            .ok_or("找不到 Windows Script Host，无法隐藏启动定时任务")?;
        if def.utc_offset_minutes != local_offset_minutes() {
            return Err("system 任务的 UTC 偏移必须与本机一致".into());
        }
        let when = When::parse(&def.when)?;
        let trigger=match when {
            When::Minutes(n)=>format!("New-ScheduledTaskTrigger -Once -At ([DateTimeOffset]::FromUnixTimeMilliseconds({}).LocalDateTime) -RepetitionInterval (New-TimeSpan -Minutes {n})",when.next_ms(chrono::Local::now().timestamp_millis())),
            When::Hours(n)=>format!("New-ScheduledTaskTrigger -Once -At ([DateTimeOffset]::FromUnixTimeMilliseconds({}).LocalDateTime) -RepetitionInterval (New-TimeSpan -Hours {n})",when.next_ms(chrono::Local::now().timestamp_millis())),
            When::Daily(h,m)=>format!("New-ScheduledTaskTrigger -Daily -At '{h:02}:{m:02}'"),
            When::Weekdays(h,m)=>format!("New-ScheduledTaskTrigger -Weekly -DaysOfWeek Monday,Tuesday,Wednesday,Thursday,Friday -At '{h:02}:{m:02}'"),
            When::Weekly(day,h,m)=>format!("New-ScheduledTaskTrigger -Weekly -DaysOfWeek {} -At '{h:02}:{m:02}'",["Monday","Tuesday","Wednesday","Thursday","Friday","Saturday","Sunday"][day as usize]),
        };
        let catchup = if def.catch_up == "once" {
            "-StartWhenAvailable"
        } else {
            ""
        };
        format!("$ErrorActionPreference='Stop'; $action=New-ScheduledTaskAction -Execute {} -WorkingDirectory {} -Argument {}; $trigger={trigger}; $settings=New-ScheduledTaskSettingsSet {catchup} -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -WakeToRun -MultipleInstances IgnoreNew -ExecutionTimeLimit (New-TimeSpan -Seconds {}); $principal=New-ScheduledTaskPrincipal -UserId ([System.Security.Principal.WindowsIdentity]::GetCurrent().Name) -LogonType Interactive -RunLevel Limited; Register-ScheduledTask -TaskName {task} -Action $action -Trigger $trigger -Settings $settings -Principal $principal -Force | Out-Null",ps(&wscript.display().to_string()),ps(&kanzei_base::path_form::simplify(root).display().to_string()),ps(&format!("//B //Nologo \"{}\"",kanzei_base::path_form::simplify(&launcher).display())),def.timeout_secs+120)
    };
    let encoded = base64::engine::general_purpose::STANDARD.encode(
        script
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    checked(
        "powershell.exe",
        &[
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-EncodedCommand".into(),
            encoded,
        ],
    )
    .await
    .map(|_| ())
}
#[cfg(not(windows))]
pub async fn system(_root: &Path, _def: &ScheduleDef, _enabled: bool) -> Result<(), String> {
    Err("system 档目前需要 Windows 任务计划程序；其他系统可用 server 档".into())
}
fn environment(
    root: &Path,
    def: &ScheduleDef,
) -> Result<crate::research_environment::ResearchEnvironment, String> {
    let id = def.host.strip_prefix("server:").ok_or("不是 server 任务")?;
    let environment = crate::research_environment::load_environment(root, id)?;
    if environment.kind != "ssh"
        || environment.status != "active"
        || environment.policy != "relaxed"
        || !environment.workdir.starts_with('/')
        || environment.host.starts_with('-')
        || environment.host.chars().any(char::is_whitespace)
    {
        return Err("server 档需要 active 的 SSH 环境、relaxed 执行策略和绝对 workdir".into());
    }
    Ok(environment)
}
pub fn cron_expr(when: &When) -> String {
    match *when {
        When::Minutes(n) => format!("*/{n} * * * *"),
        When::Hours(n) => format!("0 */{n} * * *"),
        When::Daily(h, m) => format!("{m} {h} * * *"),
        When::Weekdays(h, m) => format!("{m} {h} * * 1-5"),
        When::Weekly(day, h, m) => format!("{m} {h} * * {}", (day + 1) % 7),
    }
}
fn ssh_args(host: &str, command: String) -> Vec<String> {
    vec![
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        "ConnectTimeout=10".into(),
        host.into(),
        command,
    ]
}
pub async fn server(root: &Path, def: &ScheduleDef, enabled: bool) -> Result<(), String> {
    let env = environment(root, def)?;
    let marker = format!("# {}", task_name(root, &def.name));
    if enabled {
        checked(
            "ssh",
            &ssh_args(
                &env.host,
                format!(
                    "mkdir -p {} {} && command -v kz && kz schedule list --project-root {} > /dev/null",
                    sh(&format!("{}/.kanzei/schedules", env.workdir)),
                    sh(&format!("{}/.kanzei/artifacts/schedules/runs", env.workdir)),sh(&env.workdir)
                ),
            ),
        )
        .await?;
        let source = path(root, &def.name)?;
        checked(
            "scp",
            &[
                "-B".into(),
                "-o".into(),
                "ConnectTimeout=10".into(),
                source.display().to_string(),
                format!(
                    "{}:{}/.kanzei/schedules/{}.md",
                    env.host, env.workdir, def.name
                ),
            ],
        )
        .await?;
    }
    let cron = if enabled {
        format!(
            "{} cd {} && kz schedule run {} --project-root {} --trigger server >> {} 2>&1 {marker}",
            "* * * * *",
            sh(&env.workdir),
            sh(&def.name),
            sh(&env.workdir),
            sh(&format!(
                "{}/.kanzei/artifacts/schedules/runs/{}.log",
                env.workdir, def.name
            ))
        )
    } else {
        String::new()
    };
    let command=format!("set -eu; tmp=$(mktemp); (crontab -l 2>/dev/null || true) | grep -F -v {} > \"$tmp\" || true; {} crontab \"$tmp\"; rm -f \"$tmp\"",sh(&marker),if enabled{format!("printf '%s\\n' {} >> \"$tmp\";",sh(&cron))}else{String::new()});
    checked("ssh", &ssh_args(&env.host, command))
        .await
        .map(|_| ())
}
/// A remote immediate run is detached; history is subsequently pulled like a cron run.
pub async fn run_server_now(root: &Path, def: &ScheduleDef) -> Result<(), String> {
    let env = environment(root, def)?;
    checked("ssh",&ssh_args(&env.host,format!("cd {} && nohup kz schedule run {} --project-root {} --trigger server-manual > {} 2>&1 < /dev/null &",sh(&env.workdir),sh(&def.name),sh(&env.workdir),sh(&format!("{}/.kanzei/artifacts/schedules/runs/{}-manual.log",env.workdir,def.name))))).await.map(|_|())
}
pub async fn pull(root: &Path, def: &ScheduleDef) -> Result<usize, String> {
    let env = environment(root, def)?;
    let source = format!(
        "{}/.kanzei/artifacts/schedules/runs/{}",
        env.workdir, def.name
    );
    let names=checked("ssh",&ssh_args(&env.host,format!("find {} -maxdepth 1 -type f -name '*.json' -printf '%T@ %f\\n' 2>/dev/null | sort -rn | head -100 | cut -d ' ' -f2-",sh(&source)))).await?;
    let mut imported = 0;
    for name in names.lines().filter(|name| {
        name.ends_with(".json")
            && name
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
    }) {
        let run_id = name.trim_end_matches(".json");
        let store = SessionStore::open(&kanzei_core::project_state_path(root))
            .map_err(|e| e.to_string())?;
        if store
            .event_payload_exists(
                &history_id(root),
                Some("schedule.run_finished"),
                "$.run_id",
                run_id,
            )
            .map_err(|e| e.to_string())?
        {
            continue;
        }
        let output = checked(
            "ssh",
            &ssh_args(
                &env.host,
                format!("head -c 2097153 {}", sh(&format!("{source}/{name}"))),
            ),
        )
        .await?;
        if output.len() > 2 * 1024 * 1024 {
            return Err("远端结果超过导入大小上限".into());
        }
        let mut outcome: Outcome = serde_json::from_str(&output).map_err(|e| e.to_string())?;
        if outcome.name != def.name || outcome.run_id != run_id {
            return Err("远端结果身份与登记任务不符".into());
        }
        let lock_path = path(root, &def.name)?;
        // Claim importing before await; pending receipts are visible and never silently replayed.
        {
            let _lock =
                kanzei_base::atomic_file::lock_exclusive(&lock_path).map_err(|e| e.to_string())?;
            if store
                .event_payload_exists(
                    &history_id(root),
                    Some("schedule.run_finished"),
                    "$.run_id",
                    run_id,
                )
                .map_err(|e| e.to_string())?
            {
                continue;
            }
            if let Some(receipt) = store
                .latest_matching_event(
                    &history_id(root),
                    "schedule.import_started",
                    "$.run_id",
                    run_id,
                )
                .map_err(|e| e.to_string())?
            {
                let at = receipt.payload["at_ms"]
                    .as_i64()
                    .unwrap_or(receipt.created_at);
                if chrono::Local::now().timestamp_millis() - at < 120000 {
                    continue;
                }
                record(
                    root,
                    "schedule.run_finished",
                    &json!({"name":def.name,"run_id":run_id,"ok":false,"error":"import_interrupted","summary":"导入曾中断，部分通道可能已回写；需要按运行 ID 核查，不自动重放","finished_at_ms":chrono::Local::now().timestamp_millis()}),
                )?;
                continue;
            }
            record(
                root,
                "schedule.import_started",
                &json!({"name":def.name,"run_id":run_id,"host":def.host,"at_ms":chrono::Local::now().timestamp_millis()}),
            )?;
        }
        let mut local = def.clone();
        local
            .writeback
            .retain(|channel| !channel.starts_with("file:"));
        outcome
            .writeback
            .extend(match tokio::time::timeout(Duration::from_secs(60),super::executor::writeback(root,&local,&outcome,false)).await {
            Ok(receipts)=>receipts,Err(_)=>vec![json!({"channel":"import","ok":false,"detail":"导入回写超时；按运行 ID 检查部分回写，不自动重放"})]
        });
        record(root, "schedule.run_finished", &json!(outcome))?;
        imported += 1;
    }
    Ok(imported)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cron_and_quoting_preserve_names() {
        assert_eq!(
            cron_expr(&When::parse("工作日 09:30").unwrap()),
            "30 9 * * 1-5"
        );
        assert_eq!(
            cron_expr(&When::parse("每周日 12:00").unwrap()),
            "0 12 * * 0"
        );
        assert_eq!(sh("a'b"), "'a'\\''b'");
        assert!(ssh_args("user@host", "true".into()).contains(&"BatchMode=yes".into()));
    }
}
