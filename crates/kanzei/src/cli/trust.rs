//! Explicit approval lives in user-owned state, never inside the repository.

pub(crate) fn trust_cli(args: &[String]) -> anyhow::Result<()> {
    let action = args.first().map(String::as_str).unwrap_or("status");
    let explicit = args
        .windows(2)
        .find(|pair| pair[0] == "--project-root")
        .map(|pair| std::path::PathBuf::from(&pair[1]));
    let cwd = std::env::current_dir()?;
    let root = super::main_project_root(explicit.as_deref(), &cwd)?;
    match action {
        "status" => println!(
            "{}",
            serde_json::to_string_pretty(&kanzei_harness::project_trust::status(&root)?)?
        ),
        "approve" => {
            let fingerprint = args.get(1).filter(|value| !value.starts_with('-')).ok_or_else(|| anyhow::anyhow!("usage: kz trust approve <reviewed-fingerprint> [--project-root <path>]; first run kz trust status and review the listed sources"))?;
            kanzei_harness::project_trust::approve(&root, fingerprint)?;
            println!("Approved the reviewed source hashes for {}", root.display());
        }
        "revoke" => {
            kanzei_harness::project_trust::revoke(&root)?;
            println!(
                "Revoked project configuration/agent approvals for {}",
                root.display()
            );
        }
        _ => anyhow::bail!(
            "usage: kz trust status|approve <fingerprint>|revoke [--project-root <path>]"
        ),
    }
    Ok(())
}
