//! Evidence-backed paper assembly, review and compilation use the existing TeX backend.
use super::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Claim {
    pub text: String,
    #[serde(default)]
    pub source_ids: Vec<String>,
    #[serde(default)]
    pub result_ids: Vec<String>,
    pub metric: Option<String>,
    /// Arithmetic mean over result_ids when more than one run is cited.
    pub value: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperRecord {
    pub tex: String,
    pub template: String,
    pub claims: Vec<Claim>,
    pub reviewed_hash: Option<String>,
    pub pdf: Option<String>,
    pub manifest: Option<String>,
    #[serde(default)]
    pub compile_attempts: u32,
}

pub fn tex_escape(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '&' => "\\&".into(),
            '%' => "\\%".into(),
            '$' => "\\$".into(),
            '#' => "\\#".into(),
            '_' => "\\_".into(),
            '{' => "\\{".into(),
            '}' => "\\}".into(),
            '\\' => "\\textbackslash{}".into(),
            '~' => "\\textasciitilde{}".into(),
            '^' => "\\textasciicircum{}".into(),
            _ => c.to_string(),
        })
        .collect()
}

fn sources(root: &Path, topic: &str) -> Result<Vec<crate::docstore::Entry>, String> {
    crate::docstore::DocStore::open_topic(root, &crate::docstore::SOURCES, topic)
        .and_then(|s| s.load())
        .map_err(|e| e.to_string())
}

pub fn initialize(
    root: &Path,
    topic: &str,
    revision: u64,
    input: &Value,
) -> Result<Workflow, String> {
    update(root, topic, revision, "paper_init", "agent", |state| {
        expect(state, Stage::WritePaper)?;
        if !state.runnable() {
            return Err("当前不可推进".into());
        }
        if state.paper.is_some() {
            return Err("论文项目已存在，请修改现有 tex 后 submit_paper".into());
        }
        let dir = topic_dir(root, topic)?;
        let tex = state
            .full_rounds
            .last()
            .and_then(|round| round.paper.as_ref())
            .map(|paper| paper.tex.as_str())
            .unwrap_or("latex/paper.tex")
            .to_string();
        let continuing = !state.full_rounds.is_empty() && dir.join(&tex).is_file();
        if dir.join(&tex).exists() && !continuing {
            return Err("latex/paper.tex 已存在，不会覆盖；请用 submit_paper 接续现有稿件".into());
        }
        let (template, body) = if continuing {
            (
                "continued:previous-round".into(),
                std::fs::read_to_string(dir.join(&tex)).map_err(|e| e.to_string())?,
            )
        } else if let Some(file) = input["template_path"].as_str() {
            artifact(root, topic, file)?;
            (
                format!(
                    "{} | {}",
                    file,
                    input["template_source"].as_str().unwrap_or("user-supplied")
                ),
                std::fs::read_to_string(dir.join(file)).map_err(|e| e.to_string())?,
            )
        } else {
            let title = tex_escape(input["title"].as_str().unwrap_or(topic));
            ("generated:article-v1".into(), format!("\\documentclass[11pt]{{article}}\n\\usepackage[margin=25mm]{{geometry}}\n\\title{{{title}}}\n\\author{{}}\n\\date{{}}\n\\begin{{document}}\n\\maketitle\n\\begin{{abstract}}\nTODO abstract\n\\end{{abstract}}\n\\section{{Introduction}}\nTODO question and contribution\n\\section{{Related Work}}\nTODO literature\n\\section{{Methods}}\nTODO protocol and reproducibility\n\\section{{Results}}\n\\input{{results-table}}\nTODO interpretation\n\\section{{Discussion and Limitations}}\nTODO scope and alternatives\n\\section{{Conclusion}}\nTODO conclusion\n\\input{{references}}\n\\end{{document}}\n"))
        };
        std::fs::create_dir_all(dir.join("latex")).map_err(|e| e.to_string())?;
        if !continuing {
            kanzei_base::atomic_file::write_atomic(&dir.join(&tex), &body)
                .map_err(|e| e.to_string())?;
        }
        let summary = lifecycle::results_summary(root, state)?;
        let metric = summary["metric"].as_str().unwrap_or("metric");
        let mut table = format!("\\begin{{center}}\n\\begin{{tabular}}{{lrrr}}\nRole & Runs & Mean {} & Sample SD \\\\\n\\hline\n", tex_escape(metric));
        for role in ["baseline", "main", "ablation", "robustness"] {
            let group = &summary["groups"][role];
            table.push_str(&format!(
                "{} & {} & {:.6} & {} \\\\\n",
                role,
                group["n"],
                group["mean"].as_f64().unwrap_or(0.0),
                group["sample_sd"]
                    .as_f64()
                    .map(|sd| format!("{sd:.6}"))
                    .unwrap_or_else(|| "--".into())
            ));
        }
        table.push_str("\\end{tabular}\n\\end{center}\n");
        kanzei_base::atomic_file::write_atomic(&dir.join("latex/results-table.tex"), &table)
            .map_err(|e| e.to_string())?;
        let mut bibliography = "\\begin{thebibliography}{99}\n".to_string();
        for source in sources(root, topic)? {
            let url = source
                .fields
                .iter()
                .find(|(key, _)| matches!(key.as_str(), "URL" | "url" | "链接"))
                .map(|(_, v)| v.as_str())
                .unwrap_or("");
            bibliography.push_str(&format!(
                "\\bibitem{{{}}} {}. {}\n",
                source.id,
                tex_escape(&source.title),
                tex_escape(url)
            ));
        }
        bibliography.push_str("\\end{thebibliography}\n");
        kanzei_base::atomic_file::write_atomic(&dir.join("latex/references.tex"), &bibliography)
            .map_err(|e| e.to_string())?;
        state.paper = Some(PaperRecord {
            tex,
            template,
            claims: vec![],
            reviewed_hash: None,
            pdf: None,
            manifest: None,
            compile_attempts: 0,
        });
        Ok(())
    })
}

/// Hash transitive TeX inputs, bibliography, styles and figures; all must remain inside the topic.
pub(super) fn inputs(root: &Path, state: &Workflow) -> Result<BTreeMap<PathBuf, Vec<u8>>, String> {
    let dir = topic_dir(root, &state.topic)?;
    let paper = state.paper.as_ref().ok_or("缺少论文")?;
    artifact(root, &state.topic, &paper.tex)?;
    let mut files = BTreeMap::new();
    let mut pending = vec![dir.join(&paper.tex)];
    let re = regex::Regex::new(r"\\(input|include|includegraphics|bibliography|documentclass|usepackage)(?:\[[^\]]*\])?\{([^}]+)\}").unwrap();
    while let Some(path) = pending.pop() {
        let path = path.canonicalize().map_err(|e| e.to_string())?;
        if !path.starts_with(&dir) {
            return Err("论文依赖越出当前课题".into());
        }
        if files.contains_key(&path) {
            continue;
        }
        if files.len() >= 100 {
            return Err("论文依赖文件超过 100 个".into());
        }
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        if path
            .extension()
            .is_some_and(|e| e == "tex" || e == "sty" || e == "cls")
        {
            let text = String::from_utf8_lossy(&bytes);
            for capture in re.captures_iter(&text) {
                let kind = &capture[1];
                for name in capture[2].split(',') {
                    let base = path.parent().unwrap().join(name.trim());
                    let extensions: &[&str] = match kind {
                        "includegraphics" => &["pdf", "png", "jpg", "jpeg"],
                        "bibliography" => &["bib"],
                        "documentclass" => &["cls"],
                        "usepackage" => &["sty"],
                        _ => &["tex"],
                    };
                    let dependency = if base.is_file() {
                        Some(base.clone())
                    } else {
                        extensions
                            .iter()
                            .map(|ext| base.with_extension(ext))
                            .find(|p| p.is_file())
                    };
                    if let Some(dependency) = dependency {
                        pending.push(dependency);
                    } else if !matches!(kind, "documentclass" | "usepackage") {
                        return Err(format!("论文依赖缺失: {}", base.display()));
                    }
                }
            }
        }
        files.insert(path, bytes);
    }
    Ok(files)
}

fn fingerprint(root: &Path, state: &Workflow) -> Result<String, String> {
    let mut digest = Sha256::new();
    let dir = topic_dir(root, &state.topic)?;
    for (path, bytes) in inputs(root, state)? {
        digest.update(
            path.strip_prefix(&dir)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/")
                .as_bytes(),
        );
        digest.update(bytes);
    }
    for file in [
        state.analysis.as_deref(),
        state.full_plan.as_ref().map(|p| p.protocol.as_str()),
        Some("results.json"),
        Some("sources.md"),
    ]
    .into_iter()
    .flatten()
    {
        artifact(root, &state.topic, file)?;
        digest.update(file.as_bytes());
        digest.update(std::fs::read(dir.join(file)).map_err(|e| e.to_string())?);
    }
    digest.update(serde_json::to_vec(&state.paper.as_ref().unwrap().claims).unwrap());
    Ok(format!("{:x}", digest.finalize()))
}

pub(super) fn submit(root: &Path, state: &mut Workflow, input: &Value) -> Result<(), String> {
    if !matches!(
        state.stage,
        Stage::WritePaper | Stage::ReviewPaper | Stage::CompilePaper
    ) {
        return Err("当前不是论文阶段".into());
    }
    let tex = input["artifact"]
        .as_str()
        .unwrap_or("latex/paper.tex")
        .to_string();
    artifact(root, &state.topic, &tex)?;
    let claims: Vec<Claim> =
        serde_json::from_value(input["claims"].clone()).map_err(|e| e.to_string())?;
    if claims.is_empty() {
        return Err("论文必须提交关联来源/实验的主张表".into());
    }
    let template = state
        .paper
        .as_ref()
        .map(|p| p.template.clone())
        .unwrap_or_else(|| "existing:topic-project".into());
    let attempts = state
        .paper
        .as_ref()
        .map(|p| p.compile_attempts)
        .unwrap_or(0);
    state.paper = Some(PaperRecord {
        tex,
        template,
        claims,
        reviewed_hash: None,
        pdf: None,
        manifest: None,
        compile_attempts: attempts,
    });
    state.stage = Stage::ReviewPaper;
    Ok(())
}

pub(super) fn review(root: &Path, state: &mut Workflow) -> Result<(), String> {
    expect(state, Stage::ReviewPaper)?;
    let paper = state.paper.as_ref().ok_or("缺少论文")?;
    let input_files = inputs(root, state)?;
    let text = input_files
        .iter()
        .filter(|(p, _)| p.extension().is_some_and(|e| e == "tex"))
        .map(|(_, b)| String::from_utf8_lossy(b).into_owned())
        .collect::<Vec<_>>()
        .join("\n");
    let lower = text.to_lowercase();
    for placeholder in [
        "todo",
        "tbd",
        "在这里填写",
        "example.png",
        "lorem ipsum",
        "{{content}}",
    ] {
        if lower.contains(placeholder) {
            return Err(format!("论文仍有占位内容: {placeholder}"));
        }
    }
    if text.len() < 800 || !text.contains("\\begin{abstract}") || !text.contains("\\end{document}")
    {
        return Err("论文缺少完整正文或摘要".into());
    }
    for alternatives in [
        ["introduction", "引言"],
        ["method", "方法"],
        ["result", "结果"],
        ["limitation", "局限"],
        ["conclusion", "结论"],
    ] {
        if !alternatives.iter().any(|s| lower.contains(s)) {
            return Err(format!("论文缺少 {} 内容", alternatives[0]));
        }
    }
    let sources = sources(root, &state.topic)?;
    let cite_re = regex::Regex::new(r"\\cite\w*\*?(?:\[[^\]]*\])*\{([^}]+)\}").unwrap();
    let bib_re = regex::Regex::new(r"(?:\\bibitem(?:\[[^\]]*\])?\{|@\w+\s*\{)([^,}\s]+)").unwrap();
    let bibliography = input_files
        .values()
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .collect::<Vec<_>>()
        .join("\n");
    let keys: Vec<_> = bib_re
        .captures_iter(&bibliography)
        .map(|c| c[1].to_string())
        .collect();
    for cite in cite_re.captures_iter(&text) {
        for key in cite[1].split(',').map(str::trim) {
            if !keys.iter().any(|k| k == key) {
                return Err(format!("引用 {key} 没有 bibliography 条目"));
            }
        }
    }
    let mut covered = std::collections::HashSet::new();
    for claim in &paper.claims {
        if claim.text.trim().is_empty() || !text.contains(&claim.text) {
            return Err("主张表中的原文必须出现在论文中".into());
        }
        if claim.source_ids.is_empty() && claim.result_ids.is_empty() {
            return Err("论文主张必须绑定来源或运行结果".into());
        }
        for id in &claim.source_ids {
            if !sources.iter().any(|s| s.id == *id) {
                return Err(format!("论文引用未登记来源 {id}"));
            }
        }
        let mut values = vec![];
        for id in &claim.result_ids {
            if !state.full_results.iter().any(|r| r.result_id == *id) {
                return Err(format!("论文使用未纳入完整实验的结果 {id}"));
            }
            let run = run_fact(root, state, id)?;
            if run.status != "succeeded" {
                return Err("不能用失败运行支持论文主张".into());
            }
            covered.insert(id.clone());
            if let Some(metric) = &claim.metric {
                let metrics: Value =
                    serde_json::from_str(&run.metrics_last_json).map_err(|e| e.to_string())?;
                values.push(
                    metrics[metric]
                        .as_f64()
                        .filter(|v| v.is_finite())
                        .ok_or("主张指标不在真实运行记录中")?,
                );
            }
        }
        if let Some(value) = claim.value {
            if values.is_empty() || !value.is_finite() {
                return Err("数值主张需要 metric 和实际结果".into());
            }
            let actual = values.iter().sum::<f64>() / values.len() as f64;
            if (actual - value).abs() > 1e-6 * actual.abs().max(1.0) {
                return Err(format!("论文数值 {value} 与运行记录 {actual} 不一致"));
            }
        }
    }
    if state
        .full_results
        .iter()
        .any(|r| !covered.contains(&r.result_id))
    {
        return Err("主张表须覆盖完整实验全部结果，包括消融与鲁棒性".into());
    }
    let hash = fingerprint(root, state)?;
    state.paper.as_mut().unwrap().reviewed_hash = Some(hash);
    state.stage = Stage::CompilePaper;
    Ok(())
}

pub async fn compile(root: &Path, topic: &str, revision: u64) -> Result<Workflow, String> {
    let state = load(root, topic)?.ok_or("尚未启动")?;
    expect(&state, Stage::CompilePaper)?;
    if state.revision != revision || !state.runnable() {
        return Err("研究已更新或暂停，请回读".into());
    }
    let paper = state.paper.as_ref().ok_or("缺少论文")?;
    let hash = fingerprint(root, &state)?;
    if paper.reviewed_hash.as_ref() != Some(&hash) {
        return Err("论文或引用文件已修改，请重新 submit_paper/review_paper".into());
    }
    let dir = topic_dir(root, topic)?;
    let tex = dir.join(&paper.tex);
    let pdf = tex.with_extension("pdf");
    if pdf.is_file() {
        std::fs::remove_file(&pdf).map_err(|e| e.to_string())?;
    }
    let workdir = tex.parent().unwrap().to_path_buf();
    let filename = tex.file_name().unwrap().to_string_lossy().into_owned();
    let (ok, diagnostics) =
        tokio::task::spawn_blocking(move || crate::latex_tool::compile_latex(&workdir, &filename))
            .await
            .map_err(|e| e.to_string())?;
    kanzei_base::atomic_file::write_atomic(&dir.join("latex/compile-log.txt"), &diagnostics)
        .map_err(|e| e.to_string())?;
    kanzei_base::atomic_file::write_atomic(
        &dir.join(format!(
            "latex/compile-attempt-{}.txt",
            paper.compile_attempts + 1
        )),
        &diagnostics,
    )
    .map_err(|e| e.to_string())?;
    let bytes = std::fs::read(&pdf).unwrap_or_default();
    let log = std::fs::read_to_string(tex.with_extension("log")).unwrap_or_default();
    let passed = ok
        && bytes.starts_with(b"%PDF-")
        && bytes.len() > 500
        && !log.contains("There were undefined references")
        && !(log.contains("Citation") && log.contains("undefined"));
    let new_state = update(root, topic, revision, "compile_paper", "agent", |current| {
        if !current.runnable() || fingerprint(root, current)? != hash {
            return Err("编译期间流程或论文已变化，请回读并重新检查".into());
        }
        let record = current.paper.as_mut().ok_or("缺少论文")?;
        record.compile_attempts += 1;
        if passed {
            record.pdf = Some(
                pdf.strip_prefix(&dir)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
            record.manifest = Some("delivery.json".into());
            let manifest = json!({"topic":topic,"round":current.full_rounds.len()+1,"previous_rounds":current.full_rounds,"compiled_at":now_ms(),"source_sha256":hash,"pdf_sha256":format!("{:x}",Sha256::digest(&bytes)),"paper":record,"analysis":current.analysis,"compute":current.compute,"results":current.full_results,"results_file":"results.json","workflow":"workflow.json"});
            kanzei_base::atomic_file::write_atomic(
                &dir.join("delivery.json"),
                &serde_json::to_string_pretty(&manifest).unwrap(),
            )
            .map_err(|e| e.to_string())?;
            current.stage = Stage::Completed;
        }
        Ok(())
    })?;
    if !passed {
        return Err(format!(
            "LaTeX 编译或引用检查未通过；诊断见 latex/compile-log.txt。{}",
            diagnostics
                .chars()
                .rev()
                .take(1800)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>()
        ));
    }
    Ok(new_state)
}
