//! `kz config schema` 配置参考(R-256 批3,纯搬迁自 main.rs)。
//!
//! 独立理由:config 是「打印用户面配置参考」的只读命令,纯同步、零项目上下文依赖,
//! 与 run/tracker 正交;拆出后配置键名单变更(kanzei-harness 同源)不必读懂 run 的装配。

pub(crate) fn config_cli(args: &[String]) -> anyhow::Result<()> {
    let action = args.first().map(String::as_str).unwrap_or("schema");
    if action != "schema" {
        anyhow::bail!("kz config 只支持 schema(用户面配置参考)。用法:`kz config schema`");
    }
    print!("{}", kanzei_harness::config::config_reference());
    Ok(())
}
