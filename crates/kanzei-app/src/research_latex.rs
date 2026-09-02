//! R-348：研究 topic 的内置 LaTeX 模板与项目骨架 command。
//!
//! 这里仅负责模板落盘与路径边界；编译仍由 R-273 的 `latex` 专用通道完成，避免
//! UI/桌面端再养一套编译器。所有用户可写路径都必须落在 `.kanzei/research/<topic>/latex/`。

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

const LATEX_ROOT: &str = ".kanzei/research";

struct LatexTemplate {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    body: &'static str,
}

const TEMPLATES: [LatexTemplate; 4] = [
    LatexTemplate {
        id: "basic_report",
        name: "基础报告",
        description: "适合研究阶段性报告与结论摘要。",
        body: r#"\documentclass[11pt]{article}
\usepackage[margin=2.5cm]{geometry}
\usepackage{booktabs}
\usepackage{hyperref}
\title{{{TITLE}}}
\author{{{AUTHOR}}}
\date{{{DATE}}}
\begin{document}
\maketitle
\begin{abstract}
在这里填写报告摘要。
\end{abstract}
\section{背景}
在这里填写研究背景。
\section{方法}
在这里填写方法与实验设置。
\section{结果}
在这里填写结果与证据。
\section{结论}
在这里填写结论与后续工作。
\end{document}
"#,
    },
    LatexTemplate {
        id: "basic_paper",
        name: "基础论文",
        description: "适合结构完整的研究论文初稿。",
        body: r#"\documentclass[11pt]{article}
\usepackage[margin=2.5cm]{geometry}
\usepackage{amsmath}
\usepackage{booktabs}
\usepackage{natbib}
\usepackage{hyperref}
\title{{{TITLE}}}
\author{{{AUTHOR}}}
\date{{{DATE}}}
\begin{document}
\maketitle
\begin{abstract}
在这里填写论文摘要。
\end{abstract}
\section{引言}
在这里填写研究问题与贡献。
\section{相关工作}
在这里填写相关工作，并使用 \\citep{example} 插入引用。
\section{方法}
在这里填写方法。
\section{实验}
在这里填写实验设置与结果。
\section{结论}
在这里填写结论。
\bibliographystyle{plainnat}
\bibliography{references}
\end{document}
"#,
    },
    LatexTemplate {
        id: "experiment_record",
        name: "实验记录",
        description: "适合按时间记录实验设置、结果与复盘。",
        body: r#"\documentclass[11pt]{article}
\usepackage[margin=2.5cm]{geometry}
\usepackage{booktabs}
\usepackage{longtable}
\usepackage{hyperref}
\title{{{TITLE}}}
\author{{{AUTHOR}}}
\date{{{DATE}}}
\begin{document}
\maketitle
\section{假设}
在这里填写可证伪的研究假设。
\section{实验记录}
\begin{longtable}{p{0.22\textwidth}p{0.68\textwidth}}
\toprule
字段 & 内容 \\
\midrule
参数 & 在这里填写本次实验参数原文 \\
环境 & 在这里填写环境与版本 \\
结果 & 在这里填写结果与关键指标 \\
\bottomrule
\end{longtable}
\section{结论}
在这里填写支持、否定或不确定的结论。
\section{后续}
在这里填写下一步。
\end{document}
"#,
    },
    LatexTemplate {
        id: "paper_with_figures",
        name: "带图表论文",
        description: "适合引用 research topic 中 figures/实验产物的论文。",
        body: r#"\documentclass[11pt]{article}
\usepackage[margin=2.5cm]{geometry}
\usepackage{graphicx}
\usepackage{booktabs}
\usepackage{caption}
\usepackage{hyperref}
\graphicspath{{../figures/}{../explorations/}}
\title{{{TITLE}}}
\author{{{AUTHOR}}}
\date{{{DATE}}}
\begin{document}
\maketitle
\begin{abstract}
在这里填写论文摘要。
\end{abstract}
\section{方法}
在这里填写方法。
\section{结果}
图表引用路径相对于 latex/ 目录，例如：
\begin{figure}[htbp]
  \centering
  % 使用“插入实验图表引用”命令替换下面的占位路径。
  \includegraphics[width=0.85\textwidth]{../figures/example.png}
  \caption{实验结果图}
  \label{fig:experiment-result}
\end{figure}
\section{结论}
在这里填写结论。
\end{document}
"#,
    },
];

fn template(id: &str) -> Option<&'static LatexTemplate> {
    TEMPLATES.iter().find(|item| item.id == id)
}

fn project_root(project_dir: &str) -> PathBuf {
    kanzei_harness::config::discover_project_root(Path::new(project_dir))
        .unwrap_or_else(|| PathBuf::from(project_dir))
}

fn validate_topic(topic: &str) -> Result<(), String> {
    kanzei_tools::docstore::DocStore::validate_topic(topic)
        .map(|_| ())
        .map_err(|error| format!("topic 校验失败: {error}"))
}

fn topic_dir(project_dir: &str, topic: &str) -> Result<PathBuf, String> {
    validate_topic(topic)?;
    Ok(project_root(project_dir).join(LATEX_ROOT).join(topic))
}

fn safe_document_name(raw: &str) -> Result<String, String> {
    let mut name = raw.trim().trim_end_matches(".tex").to_string();
    if name.is_empty() {
        name = "main".to_string();
    }
    if name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
    {
        return Err("文档名只能是 latex/ 内的单个文件名".into());
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err("文档名只能包含 ASCII 字母、数字、短横线、下划线和点".into());
    }
    Ok(name)
}

#[tauri::command]
pub fn research_latex_templates() -> Value {
    Value::Array(
        TEMPLATES
            .iter()
            .map(
                |item| json!({ "id": item.id, "name": item.name, "description": item.description }),
            )
            .collect(),
    )
}

#[tauri::command]
pub fn research_latex_create(
    project_dir: String,
    topic: String,
    template_id: String,
    document_name: Option<String>,
    title: Option<String>,
) -> Result<Value, String> {
    let selected =
        template(&template_id).ok_or_else(|| format!("未知 LaTeX 模板: {template_id}"))?;
    let topic_root = topic_dir(&project_dir, &topic)?;
    let latex_dir = topic_root.join("latex");
    let figures_dir = topic_root.join("figures");
    std::fs::create_dir_all(&latex_dir).map_err(|error| format!("创建 latex/ 失败: {error}"))?;
    std::fs::create_dir_all(&figures_dir)
        .map_err(|error| format!("创建 figures/ 失败: {error}"))?;
    let name = safe_document_name(document_name.as_deref().unwrap_or("main"))?;
    let tex_name = format!("{name}.tex");
    let tex_path = latex_dir.join(&tex_name);
    if tex_path.exists() {
        return Err(format!("LaTeX 文档已存在: {tex_name}"));
    }
    let title = title.unwrap_or_else(|| "Untitled Research Document".into());
    let body = selected
        .body
        .replace("{{TITLE}}", &title)
        .replace("{{AUTHOR}}", "Kanzei Research")
        .replace("{{DATE}}", "\\today");
    std::fs::write(&tex_path, body)
        .map_err(|error| format!("写入 .tex 失败: {error} ({})", tex_path.display()))?;
    let bib_path = latex_dir.join("references.bib");
    if !bib_path.exists() {
        std::fs::write(
            &bib_path,
            "% 内置模板的参考文献入口，按需添加 BibTeX 条目。\n",
        )
        .map_err(|error| format!("写入 references.bib 失败: {error}"))?;
    }
    Ok(json!({
        "topic": topic,
        "template_id": selected.id,
        "template_name": selected.name,
        "tex_name": tex_name,
        "tex_path": tex_path.strip_prefix(project_root(&project_dir)).unwrap_or(&tex_path).display().to_string(),
        "workdir": latex_dir.strip_prefix(project_root(&project_dir)).unwrap_or(&latex_dir).display().to_string(),
        "figures_dir": figures_dir.strip_prefix(project_root(&project_dir)).unwrap_or(&figures_dir).display().to_string(),
    }))
}

fn safe_figure_name(raw: &str) -> Result<String, String> {
    let name = raw.trim();
    let path = Path::new(name);
    if name.is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        || name.contains('\\')
        || name.contains('/')
    {
        return Err("图表名必须是 figures/ 下的单个文件名".into());
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if !matches!(
        extension.to_ascii_lowercase().as_str(),
        "png" | "jpg" | "jpeg" | "pdf" | "svg"
    ) {
        return Err("图表只支持 png、jpg、jpeg、pdf 或 svg".into());
    }
    Ok(name.to_string())
}

fn escape_latex_text(text: &str) -> String {
    let mut escaped = String::new();
    for ch in text.chars() {
        match ch {
            '\\' => escaped.push_str("\\textbackslash{}"),
            '{' => escaped.push_str("\\{"),
            '}' => escaped.push_str("\\}"),
            '$' => escaped.push_str("\\$"),
            '&' => escaped.push_str("\\&"),
            '#' => escaped.push_str("\\#"),
            '%' => escaped.push_str("\\%"),
            '_' => escaped.push_str("\\_"),
            '^' => escaped.push_str("\\textasciicircum{}"),
            '~' => escaped.push_str("\\textasciitilde{}"),
            other => escaped.push(other),
        }
    }
    escaped
}

#[tauri::command]
pub fn research_latex_insert_figure(
    project_dir: String,
    topic: String,
    document_name: String,
    figure_name: String,
    caption: Option<String>,
    label: Option<String>,
) -> Result<Value, String> {
    let (root, latex_dir, tex_name, tex_path) =
        compilation_paths(&project_dir, &topic, &document_name)?;
    let figure_name = safe_figure_name(&figure_name)?;
    let figure_path = topic_dir(&project_dir, &topic)?
        .join("figures")
        .join(&figure_name);
    if !figure_path.is_file() {
        return Err(format!("找不到 topic/figures 下的图表: {figure_name}"));
    }
    let mut source =
        std::fs::read_to_string(&tex_path).map_err(|error| format!("读取 .tex 失败: {error}"))?;
    let marker = "\\end{document}";
    let position = source
        .rfind(marker)
        .ok_or_else(|| ".tex 缺少 \\end{document}，无法插入图表".to_string())?;
    let stem = Path::new(&figure_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("figure");
    let label = label.unwrap_or_else(|| format!("fig:{stem}"));
    if label.is_empty()
        || !label
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, ':' | '-' | '_'))
    {
        return Err("图表 label 只能包含 ASCII 字母、数字、冒号、短横线和下划线".into());
    }
    let caption = escape_latex_text(caption.as_deref().unwrap_or("实验结果图"));
    let snippet = format!(
        "\\n\\begin{{figure}}[htbp]\n  \\centering\n  \\includegraphics[width=0.85\\textwidth]{{../figures/{figure_name}}}\n  \\caption{{{caption}}}\n  \\label{{{label}}}\n\\end{{figure}}\n"
    );
    source.insert_str(position, &snippet);
    std::fs::write(&tex_path, source).map_err(|error| format!("写入图表引用失败: {error}"))?;
    Ok(json!({
        "document_name": tex_name,
        "figure_name": figure_name,
        "reference": format!("../figures/{figure_name}"),
        "figure_path": project_relative(&root, &figure_path),
        "tex_path": project_relative(&root, &tex_path),
        "latex_dir": project_relative(&root, &latex_dir),
        "label": label,
    }))
}

fn now_ms() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn project_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn copy_if_exists(source: &Path, destination: &Path) -> Result<bool, String> {
    if !source.is_file() {
        return Ok(false);
    }
    std::fs::copy(source, destination)
        .map(|_| true)
        .map_err(|error| format!("复制编译产物失败: {error}"))
}

fn compilation_paths(
    project_dir: &str,
    topic: &str,
    document_name: &str,
) -> Result<(PathBuf, PathBuf, String, PathBuf), String> {
    let root = project_root(project_dir);
    let topic_root = topic_dir(project_dir, topic)?;
    let latex_dir = topic_root.join("latex");
    let name = safe_document_name(document_name)?;
    let tex_name = format!("{name}.tex");
    let tex_path = latex_dir.join(&tex_name);
    if !tex_path.is_file() {
        return Err(format!("找不到 latex/ 下的 .tex 文件: {tex_name}"));
    }
    Ok((root, latex_dir, tex_name, tex_path))
}

#[tauri::command]
pub fn research_latex_compile(
    project_dir: String,
    topic: String,
    document_name: String,
) -> Result<Value, String> {
    let (root, latex_dir, tex_name, tex_path) =
        compilation_paths(&project_dir, &topic, &document_name)?;
    let history_id = format!("{}-{}", now_ms(), std::process::id());
    let history_dir = latex_dir.join("history").join(&history_id);
    std::fs::create_dir_all(&history_dir)
        .map_err(|error| format!("创建编译历史目录失败: {error}"))?;
    let (success, diagnostics) = kanzei_tools::latex_tool::compile_latex(&latex_dir, &tex_name);
    let stem = Path::new(&tex_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("main");
    let pdf_source = latex_dir.join(format!("{stem}.pdf"));
    let log_source = latex_dir.join(format!("{stem}.log"));
    let history_tex = history_dir.join(&tex_name);
    let history_pdf = history_dir.join(format!("{stem}.pdf"));
    let history_log = history_dir.join("compile.log");
    std::fs::copy(&tex_path, &history_tex)
        .map_err(|error| format!("保存源文件快照失败: {error}"))?;
    std::fs::write(&history_log, &diagnostics)
        .map_err(|error| format!("保存编译日志失败: {error}"))?;
    let has_pdf = copy_if_exists(&pdf_source, &history_pdf)?;
    let has_log = copy_if_exists(&log_source, &history_dir.join("latex.log"))?;
    let environment_path = history_dir.join("environment.json");
    let environment = json!({
        "captured_at_ms": now_ms(),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "current_dir": std::env::current_dir().ok().map(|path| path.display().to_string()),
        "path": std::env::var("PATH").unwrap_or_default(),
    });
    std::fs::write(
        &environment_path,
        serde_json::to_vec_pretty(&environment).unwrap_or_default(),
    )
    .map_err(|error| format!("保存环境快照失败: {error}"))?;
    let manifest_path = history_dir.join("compile.json");
    let manifest = json!({
        "run_id": history_id,
        "topic": topic,
        "document_name": tex_name,
        "success": success && has_pdf,
        "diagnostics": diagnostics,
        "source_path": project_relative(&root, &history_tex),
        "pdf_path": has_pdf.then(|| project_relative(&root, &history_pdf)),
        "log_path": project_relative(&root, &history_log),
        "latex_log_path": has_log.then(|| project_relative(&root, &history_dir.join("latex.log"))),
        "environment_path": project_relative(&root, &environment_path),
        "manifest_path": project_relative(&root, &manifest_path),
        "created_at_ms": now_ms(),
    });
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap_or_default(),
    )
    .map_err(|error| format!("保存编译 manifest 失败: {error}"))?;
    Ok(manifest)
}

#[tauri::command]
pub fn research_latex_history(
    project_dir: String,
    topic: String,
    document_name: Option<String>,
) -> Result<Value, String> {
    let history_dir = topic_dir(&project_dir, &topic)?.join("latex/history");
    if !history_dir.is_dir() {
        return Ok(Value::Array(Vec::new()));
    }
    let wanted = document_name
        .map(|name| safe_document_name(&name).map(|value| format!("{value}.tex")))
        .transpose()?;
    let mut entries = std::fs::read_dir(&history_dir)
        .map_err(|error| format!("读取编译历史失败: {error}"))?
        .flatten()
        .filter_map(|entry| {
            let manifest_path = entry.path().join("compile.json");
            let text = std::fs::read_to_string(manifest_path).ok()?;
            let value: Value = serde_json::from_str(&text).ok()?;
            if wanted
                .as_ref()
                .is_some_and(|name| value["document_name"] != *name)
            {
                return None;
            }
            Some(value)
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        right["created_at_ms"]
            .as_u64()
            .cmp(&left["created_at_ms"].as_u64())
    });
    Ok(Value::Array(entries))
}

fn resolve_pdf_path(
    project_dir: &str,
    topic: &str,
    relative_path: &str,
) -> Result<PathBuf, String> {
    if relative_path.trim().is_empty() || Path::new(relative_path).is_absolute() {
        return Err("PDF 路径必须是项目内相对路径".into());
    }
    if Path::new(relative_path)
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err("PDF 路径不允许包含 ..".into());
    }
    let root = project_root(project_dir);
    let latex_root = topic_dir(project_dir, topic)?.join("latex");
    let candidate = root.join(relative_path);
    let canonical_latex =
        std::fs::canonicalize(&latex_root).map_err(|error| format!("latex/ 不存在: {error}"))?;
    let canonical_candidate =
        std::fs::canonicalize(&candidate).map_err(|error| format!("PDF 不存在: {error}"))?;
    if !canonical_candidate.starts_with(&canonical_latex)
        || canonical_candidate.extension().and_then(|ext| ext.to_str()) != Some("pdf")
    {
        return Err("PDF 预览路径必须位于当前 topic 的 latex/ 下".into());
    }
    Ok(canonical_candidate)
}

#[tauri::command]
pub fn research_latex_pdf(
    project_dir: String,
    topic: String,
    pdf_path: String,
) -> Result<Value, String> {
    let path = resolve_pdf_path(&project_dir, &topic, &pdf_path)?;
    let data = std::fs::read(&path).map_err(|error| format!("读取 PDF 失败: {error}"))?;
    use base64::Engine as _;
    Ok(
        json!({ "path": pdf_path, "media_type": "application/pdf", "data": base64::engine::general_purpose::STANDARD.encode(data) }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn temp_project() -> PathBuf {
        static NEXT_TEMP_PROJECT: AtomicUsize = AtomicUsize::new(0);
        let suffix = NEXT_TEMP_PROJECT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("kz-r348-{}-{suffix}", std::process::id()));
        std::fs::create_dir_all(root.join(".kanzei/research")).unwrap();
        root
    }

    #[test]
    fn templates_expose_exactly_four_builtin_entries() {
        let value = research_latex_templates();
        assert_eq!(value.as_array().unwrap().len(), 4);
        assert!(value
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["id"].is_string()));
    }

    #[test]
    fn create_copies_selected_template_into_topic_latex() {
        let root = temp_project();
        let result = research_latex_create(
            root.display().to_string(),
            "demo".into(),
            "paper_with_figures".into(),
            Some("paper".into()),
            Some("实验论文".into()),
        )
        .unwrap();
        let path = root.join(".kanzei/research/demo/latex/paper.tex");
        let body = std::fs::read_to_string(path).unwrap();
        assert!(body.contains("实验论文"));
        assert!(body.contains("includegraphics"));
        assert_eq!(result["template_id"], "paper_with_figures");
        assert!(root.join(".kanzei/research/demo/figures").is_dir());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn rejects_path_traversal_and_unknown_template() {
        let root = temp_project();
        assert!(research_latex_create(
            root.display().to_string(),
            "demo".into(),
            "nope".into(),
            None,
            None
        )
        .is_err());
        assert!(research_latex_create(
            root.display().to_string(),
            "demo".into(),
            "basic_report".into(),
            Some("../escape".into()),
            None
        )
        .is_err());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn compile_persists_history_log_and_environment_manifest() {
        let root = temp_project();
        research_latex_create(
            root.display().to_string(),
            "demo".into(),
            "basic_report".into(),
            Some("main".into()),
            Some("编译回归".into()),
        )
        .unwrap();
        let result =
            research_latex_compile(root.display().to_string(), "demo".into(), "main".into())
                .unwrap();
        assert!(result["run_id"].is_string());
        assert!(result["manifest_path"].as_str().is_some());
        assert!(result["environment_path"].as_str().is_some());
        assert!(result["log_path"].as_str().is_some());
        assert!(root
            .join(result["manifest_path"].as_str().unwrap())
            .is_file());
        assert!(root
            .join(result["environment_path"].as_str().unwrap())
            .is_file());
        assert!(root.join(result["log_path"].as_str().unwrap()).is_file());
        let history = research_latex_history(
            root.display().to_string(),
            "demo".into(),
            Some("main".into()),
        )
        .unwrap();
        assert_eq!(history.as_array().unwrap().len(), 1);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn pdf_preview_reads_only_current_topic_latex_files() {
        let root = temp_project();
        let latex = root.join(".kanzei/research/demo/latex");
        std::fs::create_dir_all(&latex).unwrap();
        std::fs::write(latex.join("manual.pdf"), b"%PDF-smoke").unwrap();
        let path = ".kanzei/research/demo/latex/manual.pdf";
        let preview =
            research_latex_pdf(root.display().to_string(), "demo".into(), path.into()).unwrap();
        assert_eq!(preview["media_type"], "application/pdf");
        assert_eq!(preview["data"], "JVBERi1zbW9rZQ==");
        assert!(research_latex_pdf(
            root.display().to_string(),
            "demo".into(),
            "../manual.pdf".into(),
        )
        .is_err());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn insert_figure_writes_stable_relative_reference_and_rejects_escape() {
        let root = temp_project();
        research_latex_create(
            root.display().to_string(),
            "demo".into(),
            "paper_with_figures".into(),
            Some("paper".into()),
            None,
        )
        .unwrap();
        std::fs::write(
            root.join(".kanzei/research/demo/figures/result.png"),
            b"png-smoke",
        )
        .unwrap();
        let result = research_latex_insert_figure(
            root.display().to_string(),
            "demo".into(),
            "paper".into(),
            "result.png".into(),
            Some("结果 & 对照".into()),
            Some("fig:result".into()),
        )
        .unwrap();
        let body =
            std::fs::read_to_string(root.join(".kanzei/research/demo/latex/paper.tex")).unwrap();
        assert!(body.contains("../figures/result.png"));
        assert!(body.contains("结果 \\& 对照"));
        assert_eq!(result["reference"], "../figures/result.png");
        assert!(research_latex_insert_figure(
            root.display().to_string(),
            "demo".into(),
            "paper".into(),
            "../outside.png".into(),
            None,
            None,
        )
        .is_err());
        std::fs::remove_dir_all(root).ok();
    }
}
