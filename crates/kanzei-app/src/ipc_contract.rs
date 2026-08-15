//! D-381:Rust↔JS 的 IPC 形状契约。
//!
//! 为什么需要这个文件:全仓 93 个 `#[tauri::command]` 里 30+ 个返回
//! `serde_json::Value`,错误一律 `String`。也就是说**应用最丰富的数据结构是在 IPC 上
//! 手搓 JSON 过去的**——每个字段名在 Rust 和 JS 两侧各写一遍字符串字面量,中间没有
//! 任何编译期或测试期的连接。而前端冒烟断言的是 `scripts/ui-runtime-smoke.mjs` 里
//! **前端作者手写的 fixture**:后端改一个字段名 → cargo test 全绿 → 六条前端冒烟
//! 全绿 → 真实界面碎。这是全仓唯一一条「两侧都改对了才对、但没人检查」的缝,
//! 而它下游挂着整个界面(D-207 那类「界面展示的值与后端事实对不上」的结构性来源)。
//!
//! 做法不是把 30 个命令一次性改成 typed struct(那是 R 级改造),而是先把**形状**
//! 钉在一份两侧共读的产物上:
//!   - 本模块的测试拿真实命令跑一遍,把键结构抽出来与 `scripts/ipc-contract.json` 比对;
//!   - `scripts/ui-runtime-smoke.mjs` 拿同一份文件校验它的 fixture。
//! 于是「Rust 改了形状」和「fixture 与后端不一致」各自都有一条会红的路径。

/// 把一个 JSON 值抽成**只剩形状**的骨架:对象保留键并递归,数组取第一个元素为样本,
/// 标量退化成类型名。值本身不参与比较——契约管的是"有哪些键、各是什么类型",
/// 不是"这次跑出来的内容"。
pub(crate) fn shape(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.iter()
                .map(|(key, child)| (key.clone(), shape(child)))
                .collect(),
        ),
        // 空数组无从取样:记成 "array" 而不是猜。契约里出现 "array" 就说明这次
        // 取样没覆盖到,应该把夹具造得更真一点,而不是让它长期空着。
        serde_json::Value::Array(items) => match items.first() {
            Some(first) => serde_json::json!([shape(first)]),
            None => serde_json::json!("array"),
        },
        serde_json::Value::String(_) => serde_json::json!("string"),
        serde_json::Value::Number(_) => serde_json::json!("number"),
        serde_json::Value::Bool(_) => serde_json::json!("bool"),
        // null 不能定形:Option 字段取样到 None 时,契约记 "null" 会把
        // "这个键可能是字符串"这一事实丢掉。用 nullable 显式标出来。
        serde_json::Value::Null => serde_json::json!("nullable"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract_path() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/ipc-contract.json")
    }

    /// 造一个内容足够真的临时项目:每种文档至少一条、字段齐全,
    /// 否则抽出来的形状里到处是 "array"(没取到样),契约就成了摆设。
    fn fixture_project() -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kz-ipc-contract-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        std::fs::write(
            root.join(".kanzei/project/requirements.md"),
            "# Requirements\n\n## R-001 一条需求 [doing]\n- 复杂度: 中\n- 优先级: P1\n- 批次: 1/2\n- 取得线: p1\n- refs: D-001\n",
        )
        .unwrap();
        std::fs::write(
            root.join(".kanzei/project/defects.md"),
            "# Defects\n\n## D-001 一条缺陷 [open] (high)\n- 优先级: P2\n- 复杂度: 小\n",
        )
        .unwrap();
        std::fs::write(
            root.join(".kanzei/project/ideas.md"),
            "# Ideas\n\n## I-001 一条想法 [todo]\n- 优先级: P3\n",
        )
        .unwrap();
        std::fs::write(
            root.join(".kanzei/project/conventions.md"),
            "# Conventions\n\n## 1. 一节\n正文\n",
        )
        .unwrap();
        root
    }

    /// D-381:`docs_snapshot` 的形状必须与两侧共读的契约文件一致。
    ///
    /// 这条测试红了有两种可能,处理方式**不同**:
    ///   - 有意改形状:更新 `scripts/ipc-contract.json`,**并且**同步
    ///     `scripts/ui-runtime-smoke.mjs` 的 `payloads.docs_snapshot` fixture
    ///     与真正读这些字段的 `ui/*.js`。三处一起动才叫改完。
    ///   - 无意改形状:那正是本判据要拦的——后端改名不会让任何既有测试变红。
    #[test]
    fn docs_snapshot_形状与ipc契约一致() {
        let root = fixture_project();
        let snapshot = crate::docs::docs_snapshot(root.display().to_string())
            .expect("夹具项目应能取到文档快照");
        let actual = shape(&snapshot);

        // 有意改形状时的更新入口:`KZ_UPDATE_IPC_CONTRACT=1 cargo test -p kanzei-app 形状`。
        // 刻意做成显式开关而不是「自动写回」——自动写回会让契约永远等于现状,
        // 判据也就永远不会红,等于没有。
        if std::env::var("KZ_UPDATE_IPC_CONTRACT").is_ok() {
            let mut all: serde_json::Value = std::fs::read_to_string(contract_path())
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or_else(|| serde_json::json!({}));
            all["docs_snapshot"] = actual.clone();
            std::fs::write(
                contract_path(),
                serde_json::to_string_pretty(&all).unwrap() + "\n",
            )
            .unwrap();
            let _ = std::fs::remove_dir_all(&root);
            return;
        }

        let expected: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(contract_path()).expect("读不到 scripts/ipc-contract.json"),
        )
        .expect("ipc-contract.json 不是合法 JSON");
        let expected = expected
            .get("docs_snapshot")
            .expect("契约缺 docs_snapshot 条目");

        assert_eq!(
            &actual,
            expected,
            "docs_snapshot 的 IPC 形状变了。\n\
             实际:{}\n\
             要么这是有意的——同步 scripts/ipc-contract.json + ui-runtime-smoke.mjs 的 \
             payloads.docs_snapshot + 真正读这些字段的 ui/*.js(三处一起动);\n\
             要么这是无意的——后端改名在 IPC 那侧不会让任何既有测试变红,本判据就是补这一条。",
            serde_json::to_string_pretty(&actual).unwrap_or_default()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn 形状抽取只保留键与类型() {
        let value = serde_json::json!({
            "n": 1, "s": "x", "b": true, "nil": null,
            "list": [{ "k": "v" }, { "k": "另一个" }],
            "empty": [],
        });
        assert_eq!(
            shape(&value),
            serde_json::json!({
                "n": "number", "s": "string", "b": "bool", "nil": "nullable",
                "list": [{ "k": "string" }],
                "empty": "array",
            })
        );
    }
}
