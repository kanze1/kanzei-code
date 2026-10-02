pub(crate) async fn schedule_cli(args: &[String]) -> anyhow::Result<()> {
    let action = args.first().map(String::as_str).unwrap_or("list");
    let mut name = None;
    let mut root_flag = None;
    let mut trigger = "manual".to_string();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--project-root" => {
                i += 1;
                root_flag = Some(std::path::PathBuf::from(
                    args.get(i).ok_or_else(|| anyhow::anyhow!("缺少项目路径"))?,
                ));
            }
            "--trigger" => {
                i += 1;
                trigger = args
                    .get(i)
                    .ok_or_else(|| anyhow::anyhow!("缺少 trigger"))?
                    .clone();
            }
            value if !value.starts_with('-') && name.is_none() => name = Some(value.to_string()),
            _ => anyhow::bail!(
                "usage: kz schedule list|history [name] / run|register <name> [--project-root path]"
            ),
        };
        i += 1;
    }
    if !matches!(
        trigger.as_str(),
        "manual" | "timer" | "system" | "server" | "server-manual"
    ) {
        anyhow::bail!("未知 trigger")
    }
    let cwd = std::env::current_dir()?;
    let root = super::main_project_root(
        super::explicit_main_root(root_flag.as_deref()).as_deref(),
        &cwd,
    )?;
    match action {
        "list" => {
            let (defs, diagnostics) = kanzei_tools::schedules::load(&root);
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({"definitions":defs,"diagnostics":diagnostics})
                )?
            );
        }
        "history" => println!(
            "{}",
            serde_json::to_string_pretty(
                &kanzei_tools::schedules::history(&root, name.as_deref(), 100)
                    .map_err(anyhow::Error::msg)?
            )?
        ),
        "register" => {
            let name = name.ok_or_else(|| anyhow::anyhow!("缺少任务名称"))?;
            let path = kanzei_tools::schedules::path(&root, &name).map_err(anyhow::Error::msg)?;
            let def = kanzei_tools::schedules::parse(&std::fs::read_to_string(path)?, &name)
                .map_err(|e| anyhow::anyhow!("{}:{} {}", e.file, e.line, e.message))?;
            match def.host.as_str() {
                "app" => {}
                "system" => kanzei_tools::schedules::hosts::system(&root, &def, def.enabled)
                    .await
                    .map_err(anyhow::Error::msg)?,
                _ => kanzei_tools::schedules::hosts::server(&root, &def, def.enabled)
                    .await
                    .map_err(anyhow::Error::msg)?,
            };
            kanzei_tools::schedules::record(
                &root,
                if def.enabled {
                    "schedule.armed"
                } else {
                    "schedule.disarmed"
                },
                &serde_json::json!({"name":def.name,"at_ms":chrono_now_ms()}),
            )
            .map_err(anyhow::Error::msg)?;
            println!("任务主机登记已更新");
        }
        "run" => {
            let name = name.ok_or_else(|| anyhow::anyhow!("缺少任务名称"))?;
            let path = kanzei_tools::schedules::path(&root, &name).map_err(anyhow::Error::msg)?;
            let def = kanzei_tools::schedules::parse(&std::fs::read_to_string(path)?, &name)
                .map_err(|error| {
                    anyhow::anyhow!("{}:{} {}", error.file, error.line, error.message)
                })?;
            if !def.enabled && !matches!(trigger.as_str(), "manual" | "server-manual") {
                anyhow::bail!("任务已停用")
            }
            if (trigger == "system" && def.host != "system")
                || (trigger.starts_with("server") && !def.host.starts_with("server:"))
                || (trigger == "timer" && def.host != "app")
            {
                anyhow::bail!("触发主机与任务定义不一致，请重新登记")
            }
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_millis() as i64;
            let slot = if trigger == "manual" || trigger == "server-manual" {
                now
            } else {
                match kanzei_tools::schedules::due(&root, &def, now).map_err(anyhow::Error::msg)? {
                    Some(slot) => slot,
                    None => return Ok(()),
                }
            };
            let result = kanzei_tools::schedules::execute(root, def, slot, trigger)
                .await
                .map_err(anyhow::Error::msg)?;
            match result {
                Some(outcome) => {
                    println!("{}", serde_json::to_string_pretty(&outcome)?);
                    if !outcome.ok {
                        anyhow::bail!(
                            "任务运行失败：{}",
                            outcome.error.as_deref().unwrap_or("未知失败")
                        );
                    }
                }
                None => println!("槽位已执行或任务正在运行，已跳过"),
            }
        }
        _ => anyhow::bail!("usage: kz schedule list|history [name] / run|register <name>"),
    }
    Ok(())
}

fn chrono_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}
