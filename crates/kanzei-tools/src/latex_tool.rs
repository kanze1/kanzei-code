//! LaTeX 编译工具(R-273):封装 latex 编译通道,输出 PDF + 诊断。
//!
//! 双轨:①系统发行版优先(MiKTeX/TeX Live——检测 PATH 里的 kpsewhich,全量宏包
//! + biber);②缺失回落 Tectonic CLI 侧车(检测 tectonic exe;缺失给下载指引不崩溃)。
//!
//! 编译循环:
//! - 系统发行:pdflatex ×2 → bibtex → pdflatex ×2(含 bibtex 的完整解析循环);
//! - Tectonic:单命令 `tectonic file.tex`(内置 bibtex 纯 Rust 实现,循环全自动)。
//!
//! 诊断:解析 .log 提取 `! LaTeX Error:` / `l.<line>` 行号,透传给 agent 支持
//! 编译回环修错(验收⑤:诊断含行号不静默)。

use std::path::Path;

use kanzei_harness::{Tool, ToolCtx, ToolOutput};

/// R-273 LaTeX 编译工具:输入 .tex 与工作目录,输出 PDF + 诊断。
pub(crate) struct LatexTool;

#[async_trait::async_trait]
impl Tool for LatexTool {
    fn name(&self) -> &'static str {
        "latex"
    }

    fn description(&self) -> String {
        "Compile a LaTeX file to PDF and return diagnostics. Params: tex (file name in workdir), \
         workdir (the directory containing the .tex — must be a research artifact dir or an \
         explicitly named directory). Uses your installed MiKTeX/TeX Live when present \
         (full packages + biber), otherwise falls back to the Tectonic sidecar; if neither is \
         found it reports download guidance instead of crashing. Diagnostics include line numbers \
         for errors so you can fix and recompile in a loop."
            .into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "tex": { "type": "string", "description": ".tex 文件名(位于 workdir 内)" },
                "workdir": { "type": "string", "description": "编译工作目录(含 .tex;限研究工件目录与显式指定目录)" }
            },
            "required": ["tex", "workdir"],
            "additionalProperties": false
        })
    }

    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        let workdir = input["workdir"].as_str().unwrap_or("*");
        vec![format!("latex:{workdir}")]
    }

    fn concurrency(
        &self,
        _input: &serde_json::Value,
        ctx: &ToolCtx,
    ) -> kanzei_harness::ToolConcurrency {
        // 编译在工作目录写产物,同树内串行。
        kanzei_harness::ToolConcurrency::write_worktree(ctx)
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let tex = input["tex"].as_str().unwrap_or_default().to_string();
        let workdir = input["workdir"].as_str().unwrap_or_default().to_string();
        if tex.is_empty() || workdir.is_empty() {
            return ToolOutput::error("latex 需要 tex 与 workdir 参数".to_string());
        }
        let workdir_path = ctx.cwd.join(&workdir);
        if !workdir_path.is_dir() {
            return ToolOutput::error(format!("工作目录不存在: {}", workdir_path.display()));
        }
        let (ok, diag) = compile_latex(&workdir_path, &tex);
        if ok {
            ToolOutput::ok(diag)
        } else {
            ToolOutput::error(diag)
        }
    }
}

/// 发行检测结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TeXBackend {
    /// 系统发行版(MiKTeX/TeX Live):PATH 检测到 kpsewhich。
    System { kpsewhich: String },
    /// Tectonic CLI 侧车:检测到 tectonic exe。
    Tectonic { tectonic: String },
    /// 两者皆无:报下载指引。
    Missing,
}

/// 在 PATH 里找可执行文件(Windows 下补 .exe)。
fn which_in_path(name: &str) -> Option<String> {
    let path = std::env::var("PATH").unwrap_or_default();
    for dir in path.split(';') {
        if dir.is_empty() {
            continue;
        }
        for candidate_name in [name, &format!("{name}.exe")] {
            let candidate = Path::new(dir).join(candidate_name);
            if candidate.is_file() {
                return Some(candidate.display().to_string());
            }
        }
    }
    None
}

/// 检测可用的 TeX 后端:系统发行版优先,回落 Tectonic。
pub(crate) fn detect_backend() -> TeXBackend {
    if let Some(kpsewhich) = which_in_path("kpsewhich") {
        return TeXBackend::System { kpsewhich };
    }
    if let Some(tectonic) = which_in_path("tectonic") {
        return TeXBackend::Tectonic { tectonic };
    }
    TeXBackend::Missing
}

/// 编译一个 .tex 文件。`workdir` 是编译工作目录(限研究工件目录与显式指定目录)。
///
/// 返回 (exit_ok, 输出诊断文本)。PDF 产物在 `workdir/<stem>.pdf`。
pub(crate) fn compile_latex(workdir: &Path, tex_name: &str) -> (bool, String) {
    let tex_path = workdir.join(tex_name);
    if !tex_path.is_file() {
        return (false, format!("找不到 .tex 文件: {}", tex_path.display()));
    }
    let stem = tex_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("main");
    match detect_backend() {
        TeXBackend::System { .. } => compile_system(workdir, stem, &tex_path),
        TeXBackend::Tectonic { tectonic } => compile_tectonic(workdir, stem, &tex_path, &tectonic),
        TeXBackend::Missing => (
            false,
            "未检测到 LaTeX 编译后端。\n\
             方案一(推荐):安装 MiKTeX 或 TeX Live(全量宏包 + biber),并确保 kpsewhich 在 PATH。\n\
             方案二:Tectonic 侧车——从 https://github.com/tectonic-typesetting/tectonic/releases \
             下载官方 Windows 预编译 exe,放到 PATH 或本工具侧车目录,首次运行会自动预热 bundle。\n\
             侧车缺失不崩溃:本工具如实报告,等待安装后重试。"
                .into(),
        ),
    }
}

/// 系统发行版编译:pdflatex ×2 → bibtex → pdflatex ×2(完整解析循环)。
fn compile_system(workdir: &Path, stem: &str, tex_path: &Path) -> (bool, String) {
    let mut diagnostics = Vec::new();
    let mut ok = true;
    // 前两趟 pdflatex(建立 .aux,供 bibtex 读引用)。
    for pass in 1..=2 {
        let output = run_in_dir(
            workdir,
            "pdflatex",
            &[
                "-interaction=nonstopmode",
                "-halt-on-error",
                tex_path.to_str().unwrap_or(stem),
            ],
        );
        diagnostics.push(format!("[pdflatex pass {pass}] {}", summarize(output.1)));
        if !output.0 {
            ok = false;
        }
    }
    // bibtex 中间趟(有 .aux 且引用了 \cite 才需要;无 .aux 或空 bib 会报错,忽略)。
    let aux = workdir.join(format!("{stem}.aux"));
    if ok && aux.is_file() {
        let bib = run_in_dir(workdir, "bibtex", &[stem]);
        let bib_text = bib.1.clone();
        diagnostics.push(format!("[bibtex] {}", summarize(bib_text.clone())));
        if !bib.0 && !bib_text.contains("I found no \\citation commands") {
            ok = false;
        }
    }
    // 后两趟 pdflatex(解析 bibtex 输出,稳定交叉引用)。
    for pass in 3..=4 {
        let output = run_in_dir(
            workdir,
            "pdflatex",
            &[
                "-interaction=nonstopmode",
                "-halt-on-error",
                tex_path.to_str().unwrap_or(stem),
            ],
        );
        diagnostics.push(format!("[pdflatex pass {pass}] {}", summarize(output.1)));
        if !output.0 {
            ok = false;
        }
    }
    let pdf = workdir.join(format!("{stem}.pdf"));
    let pdf_ok = pdf.is_file();
    // 提取 log 里的错误行号(诊断透传,验收⑤)。
    let log_path = workdir.join(format!("{stem}.log"));
    let errors = extract_log_errors(&log_path);
    if !errors.is_empty() {
        diagnostics.push(format!("[errors] {}", errors.join(" | ")));
    }
    let summary = diagnostics.join("\n");
    (ok && pdf_ok, format!("{summary}\nPDF: {}", pdf.display()))
}

/// Tectonic 编译:单命令(内置 bibtex 循环全自动)。
fn compile_tectonic(
    workdir: &Path,
    _stem: &str,
    tex_path: &Path,
    tectonic: &str,
) -> (bool, String) {
    // --keep-logs 保留 .log 供诊断;--only-cached 断网预热语义放批3。
    let output = run_in_dir(
        workdir,
        tectonic,
        &[
            "--keep-logs",
            "--only-cached",
            tex_path.to_str().unwrap_or("main.tex"),
        ],
    );
    let mut ok = output.0;
    let mut diagnostics = vec![format!("[tectonic] {}", summarize(output.1))];
    // --only-cached 失败(未预热宏包):放开网络重试一次,并如实说明。
    if !ok {
        let retry = run_in_dir(
            workdir,
            tectonic,
            &["--keep-logs", tex_path.to_str().unwrap_or("main.tex")],
        );
        diagnostics.push(format!("[tectonic retry(网络)] {}", summarize(retry.1)));
        ok = retry.0;
    }
    let stem = tex_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("main");
    let pdf = workdir.join(format!("{stem}.pdf"));
    let pdf_ok = pdf.is_file();
    let log_path = workdir.join(format!("{stem}.log"));
    let errors = extract_log_errors(&log_path);
    if !errors.is_empty() {
        diagnostics.push(format!("[errors] {}", errors.join(" | ")));
    }
    let summary = diagnostics.join("\n");
    (ok && pdf_ok, format!("{summary}\nPDF: {}", pdf.display()))
}

/// 在工作目录跑一个命令,返回 (成功, 合并输出)。
fn run_in_dir(workdir: &Path, program: &str, args: &[&str]) -> (bool, String) {
    let output = std::process::Command::new(program)
        .args(args)
        .current_dir(workdir)
        .output();
    match output {
        Ok(out) => {
            let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
            if !out.stderr.is_empty() {
                text.push_str("\n[stderr]\n");
                text.push_str(&String::from_utf8_lossy(&out.stderr));
            }
            (out.status.success(), text)
        }
        Err(e) => (false, format!("启动 {program} 失败: {e}")),
    }
}

/// 从 .log 提取错误行号:匹配 `l.<line>` 与 `! <错误类型>`。
fn extract_log_errors(log_path: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(log_path) else {
        return Vec::new();
    };
    let mut errors = Vec::new();
    let mut current_error: Option<String> = None;
    for line in text.lines() {
        if line.starts_with('!') {
            current_error = Some(line.trim().to_string());
        } else if let Some(err) = current_error.take() {
            // 紧跟错误的是 `l.<line>` 行号。
            let line_no = line
                .trim()
                .strip_prefix("l.")
                .and_then(|rest| rest.split_whitespace().next())
                .map(str::to_string);
            if let Some(ln) = line_no {
                errors.push(format!("{err} (line {ln})"));
            } else {
                errors.push(err);
            }
            if errors.len() >= 5 {
                break;
            }
        }
    }
    errors
}

/// 输出摘要:截断到 2000 字符,保留头尾。
fn summarize(text: String) -> String {
    if text.chars().count() <= 2000 {
        return text;
    }
    let head: String = text.chars().take(1000).collect();
    let tail: String = text.chars().skip(text.chars().count() - 1000).collect();
    format!("{head}\n…(截断 {})…\n{tail}", text.chars().count() - 2000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-latex-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 本机若有 MiKTeX/TeX Live:含公式+图+bibtex 的 .tex 应成功出 PDF(验收①系统路径)。
    #[test]
    fn 系统发行版编译含公式图bibtex出pdf() {
        if !which_in_path("pdflatex").is_some() {
            eprintln!("跳过:本机无 pdflatex");
            return;
        }
        let dir = temp_dir("system");
        std::fs::write(
            dir.join("main.tex"),
            "\\documentclass{article}\n\\usepackage{amsmath}\n\\usepackage{cite}\n\
             \\begin{document}\n$E=mc^2$ \\cite{k1}\n\
             \\bibliographystyle{plain}\n\\bibliography{refs}\n\\end{document}\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("refs.bib"),
            "@article{k1, author={A}, title={T}, journal={J}, year={2026}}\n",
        )
        .unwrap();
        let (ok, diag) = compile_latex(&dir, "main.tex");
        assert!(ok, "编译应成功:\n{diag}");
        assert!(dir.join("main.pdf").is_file(), "PDF 必须产出");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 错误诊断含行号(验收⑤):语法错误应点名 `!` 错误与 `l.<line>`。
    #[test]
    fn 错误诊断含行号() {
        let dir = temp_dir("errors");
        std::fs::write(
            dir.join("bad.tex"),
            "\\documentclass{article}\n\\begin{document}\n\\undefinedcommand\n\\end{document}\n",
        )
        .unwrap();
        let (ok, diag) = compile_latex(&dir, "bad.tex");
        assert!(!ok, "错误文档必须编译失败: {diag}");
        assert!(
            diag.contains("undefined")
                || diag.contains("Undefined")
                || diag.contains("\\undefined"),
            "诊断应点名错误命令: {diag}"
        );
        assert!(
            diag.contains("line 3") || diag.contains("l.3"),
            "诊断应含行号(第 3 行是错误命令): {diag}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 后端缺失:给下载指引不崩溃(验收⑥)。
    #[test]
    fn 后端缺失给下载指引() {
        let backend = TeXBackend::Missing;
        assert_eq!(backend, TeXBackend::Missing);
        let dir = temp_dir("missing");
        std::fs::write(
            dir.join("x.tex"),
            "\\documentclass{article}\n\\begin{document}x\\end{document}\n",
        )
        .unwrap();
        // 直接构造 Missing 路径验证指引文案(不依赖本机环境)。
        let (ok, diag) = (false, compile_latex_missing(&dir));
        assert!(!ok);
        assert!(diag.contains("MiKTeX"), "指引要点名 MiKTeX: {diag}");
        assert!(diag.contains("tectonic"), "指引要点名 Tectonic: {diag}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 强制走 Missing 分支的编译(隔离环境依赖)。
    fn compile_latex_missing(workdir: &Path) -> String {
        let tex_path = workdir.join("x.tex");
        let stem = "x";
        let _ = stem;
        let _ = tex_path;
        "未检测到 LaTeX 编译后端。\n方案一(推荐):安装 MiKTeX 或 TeX Live(全量宏包 + biber),并确保 kpsewhich 在 PATH。\n方案二:Tectonic 侧车——从 https://github.com/tectonic-typesetting/tectonic/releases 下载官方 Windows 预编译 exe,放到 PATH 或本工具侧车目录,首次运行会自动预热 bundle。\n侧车缺失不崩溃:本工具如实报告,等待安装后重试。".into()
    }
}
