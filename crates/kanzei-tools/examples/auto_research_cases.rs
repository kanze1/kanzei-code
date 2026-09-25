//! Reproducible acceptance driver: real literature fetch, CUDA subprocesses and TeX compilation.
//! The driver supplies researcher decisions and prose; it does not claim to evaluate model autonomy.
use kanzei_harness::{Tool, ToolCtx};
use kanzei_tools::research_workflow::{self as workflow, ResearchWorkflowTool, Stage, Workflow};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};

struct Case {
    root: PathBuf,
    topic: String,
    ctx: ToolCtx,
    next_id: u32,
}
impl Case {
    fn state(&self) -> Workflow {
        workflow::load(&self.root, &self.topic).unwrap().unwrap()
    }
    fn dir(&self) -> PathBuf {
        self.root.join(".kanzei/research").join(&self.topic)
    }
    fn write(&self, name: &str, text: &str) {
        let path = self.dir().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    fn audit(&self, action: &str, output: &kanzei_harness::ToolOutput) {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(self.root.join("case-events.jsonl"))
            .unwrap();
        writeln!(
            file,
            "{}",
            json!({"action":action,"error":output.is_error,"output":output.content})
        )
        .unwrap();
    }
    async fn action(&self, action: &str, mut input: Value) -> kanzei_harness::ToolOutput {
        input["action"] = json!(action);
        input["topic"] = json!(self.topic);
        input["revision"] = json!(self.state().revision);
        let out = ResearchWorkflowTool {
            topic: Some(self.topic.clone()),
        }
        .execute(input, &self.ctx)
        .await;
        self.audit(action, &out);
        println!(
            "{}: {} -> {:?} {}",
            self.topic,
            action,
            self.state().stage,
            if out.is_error { "ERROR" } else { "ok" }
        );
        out
    }
    async fn ok(&self, action: &str, input: Value) {
        let out = self.action(action, input).await;
        assert!(!out.is_error, "{action}: {}", out.content);
    }
    async fn run(
        &mut self,
        kind: &str,
        role: &str,
        seed: u64,
        experiment: Option<&str>,
        fail: bool,
    ) -> String {
        let id = format!("E-001-{:02}", self.next_id);
        self.next_id += 1;
        let python = if cfg!(windows) {
            ".auto-venv\\Scripts\\python.exe"
        } else {
            ".auto-venv/bin/python"
        };
        let command = format!(
            "{python} memory_probe.py --case {kind} --role {role} --seed {seed} {}",
            if fail { "--fail" } else { "" }
        );
        let params = json!({"experiment_id":experiment,"role":role,"seed":seed,"case":kind});
        let hash = format!(
            "{:x}",
            Sha256::digest(std::fs::read(self.dir().join("memory_probe.py")).unwrap())
        );
        let input = json!({"action":"run","topic":self.topic,"exploration_id":"E-001","result_id":id,
            "execution":{"kind":"local","command":command},"params_text":params.to_string(),"code_ref":{"sha256":hash,"file":"memory_probe.py"},"max_duration_ms":120000});
        let out = kanzei_tools::research_runner::ResearchRunnerTool
            .execute(input, &self.ctx)
            .await;
        self.audit("research_runner", &out);
        assert_eq!(out.is_error, fail, "run {id}: {}", out.content);
        println!(
            "{}: run {} {} seed={} {}",
            self.topic,
            id,
            role,
            seed,
            if fail { "expected failure" } else { "ok" }
        );
        id
    }
}

async fn run_case(base: &Path, kind: &str) -> Value {
    let root = base.join(kind);
    let topic = format!("agentmem-{kind}");
    let dir = root.join(".kanzei/research").join(&topic);
    assert!(
        !dir.exists(),
        "Case directory already exists; choose a new output root"
    );
    std::fs::create_dir_all(&dir).unwrap();
    let mut case = Case {
        root: root.clone(),
        topic: topic.clone(),
        ctx: ToolCtx::new(root.clone(), root.clone()),
        next_id: 1,
    };
    workflow::start(&root, &topic, Default::default(), 20).unwrap();
    case.write(
        "memory_probe.py",
        include_str!("../../../scripts/fixtures/auto-research-memory-probe.py"),
    );
    case.write(
        "requirements.txt",
        "# Reuse the installed CUDA wheel inside a topic venv.\ntorch>=2.0\n",
    );
    let source_text="# Sources\n\n## S-001 MemGPT [active]\n- URL: https://arxiv.org/abs/2310.08560\n- 年份: 2023\n- 证据深度: 摘要级\n\n## S-002 LLMLingua [active]\n- URL: https://arxiv.org/abs/2310.05736\n- 年份: 2023\n- 证据深度: 摘要级\n";
    case.write("sources.md", source_text);
    for (id, url) in [
        ("S-001", "https://arxiv.org/abs/2310.08560"),
        ("S-002", "https://arxiv.org/abs/2310.05736"),
    ] {
        let out = kanzei_tools::research_verify::ResearchVerifyTool
            .execute(
                json!({"action":"capture_source","topic":topic,"source_id":id,"url":url}),
                &case.ctx,
            )
            .await;
        case.audit("capture_source", &out);
        assert!(!out.is_error, "{}", out.content);
    }
    let question = if kind == "freshness" {
        "Does filtering obsolete memory improve retrieval accuracy?"
    } else {
        "Does 32x dimensional compression preserve retrieval accuracy within one percentage point?"
    };
    case.write("survey.md","# Literature review\n\nMemGPT studies memory tiers for extended context (S-001, literature V1, abstract only). LLMLingua studies prompt compression (S-002, literature V1, abstract only). These papers motivate memory selection and budget tradeoffs; neither validates our synthetic benchmark. The case does not reproduce their methods.\n\n# Research gaps\nControlled freshness and capacity interventions can separate representation effects from a language model's reasoning. Generalization to a deployed agent remains untested.\n");
    case.ok("survey_complete", json!({"artifact":"survey.md"}))
        .await;
    case.write("research-map.md","# Agent memory research map\n\n- Freshness: obsolete entries can conflict with current facts (S-001). Test a timestamp filter against unfiltered retrieval.\n- Capacity: compression saves memory but may remove discriminative information (S-002). Sweep retained dimensions against full-dimensional retrieval.\nBoth routes use matched synthetic inputs; neither establishes performance on an LLM agent.\n");
    let candidates = vec![
        json!({"id":kind,"title":format!("Memory {kind}"),"question":question,"rationale":"Controlled mechanism test motivated by memory management and compression literature","uncertainty":"Synthetic retrieval may not generalize to LLM agents","cost":"One local GPU, bounded short runs","validation":"Paired seeds, baseline, main, ablation and robustness","source_ids":["S-001","S-002"]}),
    ];
    case.ok(
        "publish_map",
        json!({"artifact":"research-map.md","directions":candidates}),
    )
    .await;
    assert_eq!(case.state().stage, Stage::ChooseDirection);
    assert!(workflow::check_run(&root, &topic, "E-001").is_err());
    // Explicit test-user selection, never a model action.
    workflow::user_action(&root, &topic, case.state().revision, "select", Some(kind)).unwrap();
    case.write("protocol.md",&format!("# Protocol\n\n{question}\n\n768 synthetic key/value pairs, 128 dimensions, CUDA matrix retrieval. Match random seeds and inputs. Accuracy is the fraction of exact current-value matches. Freshness intervention uses oracle timestamps; compression retains the first four dimensions. Full matrix: seeds 0,1,2 for baseline and main; seed 3 for ablation and seed 4 for shifted query noise. No LLM evaluation or publication-level evidence is claimed.\n"));
    case.write("explorations/E-001.md",&format!("---\nkind: exploration\nid: E-001\ntopic: {topic}\ntitle: Synthetic memory retrieval\nstatus: running\nhypothesis: {question}\ndepends_on:\nsupersedes:\nentry_refs:\nenvironment: local\nbudget: 1 gpu-hour\ncreated_at: 1\nupdated_at: 1\n---\n\n## 假设\n{question}\n\n## 实验结果\n| 实验 | 参数 | 状态 | 关键指标 | 产物 | 结论 |\n| --- | --- | --- | --- | --- | --- |\n\n## 结论\nPending real measurements.\n\n## 后续\nFull experiment suite.\n"));
    let mvp = json!({"mvp":{"question":question,"hypothesis":question,"baseline":"Full unfiltered representation","metric":"accuracy","success":if kind=="freshness" {"paired accuracy improves by at least 0.05"} else {"main accuracy >= baseline accuracy - 0.01"},"failure":"criterion is not met","exploration_id":"E-001","protocol":"protocol.md"}});
    case.ok("define_mvp", mvp.clone()).await;
    case.ok("prepare_compute",json!({"compute":{"environment_id":null,"python":"python","gpu_required":true,"min_vram_mb":1024,"requirements":"requirements.txt","upload_files":["memory_probe.py"]}})).await;
    assert_eq!(
        case.state().compute.as_ref().unwrap().snapshot["gpu_available"],
        true
    );
    let baseline = case.run(kind, "baseline", 42, None, false).await;
    case.write("prepare.md","# Preparation\nIsolated topic venv, existing torch CUDA wheel, device allocation smoke and installed package snapshot recorded by prepare_compute. Baseline executed through research_runner.\n");
    case.ok(
        "environment_ready",
        json!({"artifact":"prepare.md","baseline_result":baseline}),
    )
    .await;
    if kind == "compression" {
        let failed = case.run(kind, "main", 42, None, true).await;
        case.ok("record_mvp", json!({"result_ids":[failed]})).await;
        case.write("interpretation.md","# MVP interpretation\nThe injected process error supplies no evidence about compression quality. Repair the command and retry under the same protocol.\n");
        assert!(
            case.action(
                "interpret",
                json!({"artifact":"interpretation.md","verdict":"supported","next":"complete"})
            )
            .await
            .is_error
        );
        case.ok(
            "interpret",
            json!({"artifact":"interpretation.md","verdict":"inconclusive","next":"iterate"}),
        )
        .await;
        case.ok("define_mvp", mvp).await;
        case.ok(
            "environment_ready",
            json!({"artifact":"prepare.md","baseline_result":baseline}),
        )
        .await;
    }
    let mvp_run = case.run(kind, "main", 42, None, false).await;
    case.ok("record_mvp", json!({"result_ids":[mvp_run]})).await;
    case.write("interpretation.md","# MVP interpretation\nThe baseline and intervention now have successful CUDA runs. The exploratory result warrants the registered paired-seed experiment matrix. It is not evidence of agent-level generalization.\n");
    case.ok(
        "interpret",
        json!({"artifact":"interpretation.md","verdict":"inconclusive","next":"complete"}),
    )
    .await;
    let mut specs = vec![];
    for role in ["baseline", "main"] {
        for seed in 0..3 {
            specs.push(json!({"id":format!("{role}-{seed}"),"role":role,"seed":seed,"description":"Matched synthetic query set"}));
        }
    }
    specs.push(json!({"id":"ablation-3","role":"ablation","seed":3,"description":"Remove filter or increase retained dimensions"}));
    specs.push(json!({"id":"robustness-4","role":"robustness","seed":4,"description":"Higher query noise"}));
    case.ok(
        "define_full",
        json!({"full_plan":{"protocol":"protocol.md","experiments":specs}}),
    )
    .await;
    // Stop/reload/resume while preserving the exact plan.
    let paused =
        workflow::user_action(&root, &topic, case.state().revision, "pause", None).unwrap();
    assert!(workflow::check_run(&root, &topic, "E-001").is_err());
    workflow::user_action(&root, &topic, paused.revision, "resume", None).unwrap();
    for spec in specs {
        let id = case
            .run(
                kind,
                spec["role"].as_str().unwrap(),
                spec["seed"].as_u64().unwrap(),
                spec["id"].as_str(),
                false,
            )
            .await;
        case.ok(
            "record_full",
            json!({"full_results":[{"experiment_id":spec["id"],"result_id":id}]}),
        )
        .await;
    }
    let summary: Value =
        serde_json::from_str(&std::fs::read_to_string(case.dir().join("results.json")).unwrap())
            .unwrap();
    let baseline_mean = summary["groups"]["baseline"]["mean"].as_f64().unwrap();
    let main_mean = summary["groups"]["main"]["mean"].as_f64().unwrap();
    let supported = if kind == "freshness" {
        main_mean - baseline_mean >= 0.05
    } else {
        main_mean >= baseline_mean - 0.01
    };
    let verdict = if supported { "supported" } else { "rejected" };
    let analysis=format!("# Integrated analysis\n\nQuestion: {question}\n\nBaseline mean accuracy: {baseline_mean:.6}. Main mean accuracy: {main_mean:.6}. Difference: {:.6}. Verdict: {verdict}.\n\nThe experiment uses three paired seeds, an ablation and a shifted-noise robustness slice. Each run's exact metrics, seed, code hash and environment reference are in results.json and state.db.\n\n# Limitations\nThese are synthetic key/value retrieval tasks on CUDA, not an LLM agent benchmark or a reproduction of MemGPT/LLMLingua. Oracle timestamps make freshness filtering easy; coordinate truncation is not learned prompt compression. Three seeds do not establish broad statistical significance.\n",main_mean-baseline_mean);
    case.write("analysis.md", &analysis);
    case.ok(
        "submit_analysis",
        json!({"artifact":"analysis.md","verdict":verdict}),
    )
    .await;
    let title = if kind == "freshness" {
        "A Controlled Check of Memory Freshness in CUDA Retrieval"
    } else {
        "A Negative Result for Aggressive Memory Compression"
    };
    let mut init = json!({"title":title});
    if kind == "compression" {
        case.write("local-template.tex","\\documentclass[11pt]{article}\n\\usepackage[margin=25mm]{geometry}\n\\begin{document}\nTODO\n\\end{document}\n");
        init["template_path"] = json!("local-template.tex");
        init["template_source"] = json!("locally generated acceptance template");
    }
    case.ok("paper_init", init).await;
    let mut claims = vec![];
    let mut findings = String::new();
    for role in ["baseline", "main", "ablation", "robustness"] {
        let value = summary["groups"][role]["mean"].as_f64().unwrap();
        let claim = format!("The {role} mean accuracy was {value:.6}.");
        findings.push_str(&format!("{claim}\n\n"));
        let ids: Vec<_> = summary["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["role"] == role)
            .map(|r| r["result_id"].clone())
            .collect();
        claims.push(json!({"text":claim,"result_ids":ids,"metric":"accuracy","value":value}));
    }
    let literature="Prior work investigates memory tiers and prompt compression as responses to context constraints.";
    claims
        .push(json!({"text":literature,"source_ids":["S-001","S-002"],"metric":null,"value":null}));
    let paper = format!(
        r#"\documentclass[11pt]{{article}}
\usepackage[margin=25mm]{{geometry}}
\title{{{title}}}
\author{{AUTO research acceptance case}}
\date{{}}
\begin{{document}}
\maketitle
\begin{{abstract}}
We test one narrowly defined memory intervention using synthetic key/value retrieval on an actual CUDA device. The baseline mean accuracy is {baseline_mean:.6} and the intervention mean is {main_mean:.6}. The registered criterion is {verdict}. This is an executable system acceptance experiment, not a claim about general language-model agents.
\end{{abstract}}
\section{{Introduction}}
{question} Memory policies can change which evidence a retrieval system exposes. This experiment isolates a small mechanism so that the complete research workflow can be audited. Its contribution is a reproducible observation in the specified generator, together with traceable execution, rather than a new state-of-the-art method.
\section{{Related Work}}
{literature} MemGPT motivates tiered memory management \cite{{S-001}} and LLMLingua motivates compression tradeoffs \cite{{S-002}}. Only their abstracts were consulted here. Our numerical result is not attributed to either paper and does not reproduce either architecture.
\section{{Methods}}
The generator creates 768 normalized random keys with 128 coordinates. Queries perturb the keys. Retrieval uses a CUDA matrix product and reports the fraction of exact current-value matches. Baseline and main conditions use seeds 0, 1 and 2 with matched inputs. A separate ablation uses seed 3 and a higher-noise robustness condition uses seed 4. The MVP uses seed 42 and is excluded from the full-experiment summary.

For freshness, obsolete entries are closer to the query but store contradictory values; the intervention filters them using oracle timestamps. The ablation removes filtering. For compression, the intervention retains four coordinates, the baseline retains all 128, and the ablation retains 32. The compression success margin was at most a 0.01 accuracy loss. The freshness success margin was a gain of at least 0.05.
\section{{Results}}
\input{{results-table}}
{findings}
The registered decision is {verdict}. The table reports arithmetic means and sample standard deviations across available runs; single-run slices have no estimated between-run variability. Every numeric claim is checked against the persisted callbacks before compilation.
\section{{Discussion and Limitations}}
The result applies to this synthetic generator only. Oracle timestamps and independent random vectors omit language understanding, ambiguous facts and learned memory policies. Coordinate truncation is not semantic prompt compression. Three paired seeds and one run per stress slice cannot establish broad statistical significance. Hardware execution demonstrates that the experiment path works, not that an LLM agent would improve. Failed executions are preserved as operational events and excluded from scientific support.
\section{{Reproducibility}}
The delivery directory includes the Python generator, installed-package and GPU snapshots, protocols, raw predictions, per-run terminal logs, callback metrics, the complete matrix and a manifest with source and PDF hashes. The experiment can be rerun with the recorded case, role and seed. Full results are stored in results.json; individual code hashes and environment references are retained in state.db.
\section{{Conclusion}}
The specified hypothesis is {verdict} under the registered criterion. A follow-up should test realistic temporal metadata, semantic encoders and downstream agent tasks before extending this conclusion. The acceptance case itself demonstrates an auditable path from a research map through real experiments to a compiled paper.
\input{{references}}
\end{{document}}
"#
    );
    case.write("latex/paper.tex", &paper);
    if kind == "compression" {
        case.write(
            "latex/paper.tex",
            &paper.replace(
                "\\section{Results}",
                "\\UndefinedAcceptanceCommand\n\\section{Results}",
            ),
        );
        case.ok("submit_paper", json!({"claims":claims})).await;
        case.ok("review_paper", json!({})).await;
        assert!(case.action("compile_paper", json!({})).await.is_error);
        assert_eq!(case.state().stage, Stage::CompilePaper);
        case.write("latex/paper.tex", &paper);
    }
    case.ok("submit_paper", json!({"claims":claims})).await;
    case.ok("review_paper", json!({})).await;
    case.ok("compile_paper", json!({})).await;
    assert_eq!(case.state().stage, Stage::Completed);
    let result = json!({"case":kind,"topic":topic,"baseline":baseline_mean,"main":main_mean,"verdict":verdict,"full_runs":8,"total_runs":case.next_id-1,"paper":case.dir().join("latex/paper.pdf"),"manifest":case.dir().join("delivery.json"),"compile_attempts":case.state().paper.unwrap().compile_attempts,"driver":"deterministic researcher decisions; real tools, HTTP, GPU and TeX"});
    std::fs::write(
        root.join("case-result.json"),
        serde_json::to_string_pretty(&result).unwrap(),
    )
    .unwrap();
    result
}

#[tokio::main]
async fn main() {
    let base = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("usage: auto_research_cases <new-output-directory>"),
    );
    std::fs::create_dir_all(&base).unwrap();
    let base = base.canonicalize().unwrap();
    let mut results = vec![];
    for kind in ["freshness", "compression"] {
        results.push(run_case(&base, kind).await);
    }
    std::fs::write(
        base.join("results.json"),
        serde_json::to_string_pretty(&results).unwrap(),
    )
    .unwrap();
    println!("{}", serde_json::to_string_pretty(&results).unwrap());
}
