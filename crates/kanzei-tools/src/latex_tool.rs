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
                "workdir": { "type": "string", "description": "编译工作目录(含 .tex;限研究工件目录与显式指定目录)" },
                "to_png": { "type": "boolean", "description": "编译成功后把 PDF 首页转 PNG 回模型(默认 true;R-273 批2)" }
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
        // R-273 批2:to_png=true(默认)时编译成功后将 PDF 转 PNG 经 images 通道回模型。
        let to_png = input["to_png"].as_bool().unwrap_or(true);
        if tex.is_empty() || workdir.is_empty() {
            return ToolOutput::error("latex 需要 tex 与 workdir 参数".to_string());
        }
        // D-393:workdir 路径边界——相对路径、防 `..`、canonicalize 后限研究工件目录。
        let workdir_path = match crate::resolve_research_workdir(&ctx.cwd, &workdir) {
            Ok(p) => p,
            Err(e) => return ToolOutput::error(format!("workdir 校验失败: {e}")),
        };
        let (ok, diag) = compile_latex(&workdir_path, &tex);
        if !ok {
            return ToolOutput::error(diag);
        }
        if !to_png {
            return ToolOutput::ok(diag);
        }
        // 编译成功:PDF 首页转 PNG 回模型(验收②轨迹)。
        // D-391:stem 与编译侧口径一致(file_stem)——含点文件名(如 my.paper.tex)
        // 编译产物是 my.paper.pdf;split('.') 取 my 会找错 PDF 静默丢 PNG。
        let stem = stem_of(&tex);
        let pdf = workdir_path.join(format!("{stem}.pdf"));
        let png = pdf_to_png(&pdf, &workdir_path, stem);
        match png {
            Ok(png_bytes) => {
                let base64 = base64_engine_encode(&png_bytes);
                let mut output = ToolOutput::ok(format!(
                    "{diag}\n[PNG] 已转 PDF 首页为 PNG({} 字节)回模型",
                    png_bytes.len()
                ));
                output = output.with_images(vec![kanzei_harness::ToolImage {
                    media_type: "image/png".into(),
                    data: base64,
                }]);
                output
            }
            Err(e) => ToolOutput::ok(format!("{diag}\n[PNG] 转换失败(编译已成功): {e}")),
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
#[cfg(test)]
thread_local! {
    /// D-584 测试注入缝:当前线程的 PATH 替身。测试模拟"无后端"不得清进程级
    /// PATH——cargo test 同进程多线程,清 PATH 会让并行测试按名拉起 git 等
    /// 可执行时误报 not found;线程级覆写只影响本测试线程。
    static PATH_OVERRIDE: std::cell::RefCell<Option<String>> =
        std::cell::RefCell::new(None);
}

fn lookup_path() -> String {
    #[cfg(test)]
    if let Some(path) = PATH_OVERRIDE.with(|o| o.borrow().clone()) {
        return path;
    }
    std::env::var("PATH").unwrap_or_default()
}

fn which_in_path(name: &str) -> Option<String> {
    let path = lookup_path();
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
    // D-391:stem 口径统一走 stem_of(file_stem),与 execute 的 PNG 转换一致。
    let stem = stem_of(tex_name);
    match detect_backend() {
        TeXBackend::System { .. } => compile_system(workdir, stem, &tex_path),
        TeXBackend::Tectonic { tectonic } => compile_tectonic(workdir, stem, &tex_path, &tectonic),
        TeXBackend::Missing => (false, missing_guidance()),
    }
}

/// D-394:后端缺失指引文案单源——生产 Missing 分支与测试共用,防测试内硬编码
/// 副本漂移(此前测试断言的是副本,生产分支零执行)。
pub(crate) fn missing_guidance() -> String {
    "未检测到 LaTeX 编译后端。\n\
     方案一(推荐):安装 MiKTeX 或 TeX Live(全量宏包 + biber),并确保 kpsewhich 在 PATH。\n\
     方案二:Tectonic 侧车——从 https://github.com/tectonic-typesetting/tectonic/releases \
     下载官方 Windows 预编译 exe,放到 PATH 或本工具侧车目录,首次运行会自动预热 bundle。\n\
     侧车缺失不崩溃:本工具如实报告,等待安装后重试。"
        .into()
}

/// D-391:统一「文件名 → stem」口径(与 `Path::file_stem` 一致)。execute 与
/// compile_latex 共用,避免 `split('.')` 与 `file_stem` 分裂(含点文件名丢 PNG)。
fn stem_of(tex_name: &str) -> &str {
    Path::new(tex_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("main")
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

/// R-273 批3:biber 是否可用(PATH 检测)。Tectonic 默认 natbib/bibtex(内置纯 Rust,
/// 循环全自动);biblatex 仅在检测到 biber 二进制时可用,向 agent 显式声明。
pub(crate) fn biber_available() -> bool {
    which_in_path("biber").is_some()
}

/// Tectonic 编译:单命令(内置 bibtex 循环全自动)。
fn compile_tectonic(
    workdir: &Path,
    _stem: &str,
    tex_path: &Path,
    tectonic: &str,
) -> (bool, String) {
    // --keep-logs 保留 .log 供诊断;--only-cached 免每次联网核对 bundle(#1224)。
    // 先只读缓存:已预热宏包的文档断网也能编;未预热则失败并给明确诊断(验收③)。
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
    let mut diagnostics = vec![format!("[tectonic --only-cached] {}", summarize(output.1))];
    // --only-cached 失败(未预热宏包/断网):给明确诊断——区分「未预热」与「编译错误」。
    if !ok {
        let retry = run_in_dir(
            workdir,
            tectonic,
            &["--keep-logs", tex_path.to_str().unwrap_or("main.tex")],
        );
        diagnostics.push(format!("[tectonic retry(网络)] {}", summarize(retry.1)));
        ok = retry.0;
        if !ok {
            diagnostics.push(
                "tectonic 在 --only-cached 与网络重试下均失败:若为断网且宏包未预热,\
                 请先联网跑一次完整编译预热 bundle,之后即可 --only-cached 断网编译。"
                    .to_string(),
            );
        }
    }
    // bib 路线声明(验收④ Tectonic 路径):默认 natbib/bibtex(内置);biber 可用才声明 biblatex。
    if biber_available() {
        diagnostics.push(
            "bib 路线:检测到 biber——biblatex 可用;默认仍建议 natbib/bibtex(Tectonic 内置纯 Rust,循环全自动)。"
                .to_string(),
        );
    } else {
        diagnostics.push(
            "bib 路线:未检测到 biber——biblatex 不可用;默认 natbib/bibtex(Tectonic 内置纯 Rust 实现,循环全自动)。"
                .to_string(),
        );
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

/// R-273 批2:PDF 首页转 PNG(pdftoppm,poppler——MiKTeX/TeX Live 自带)。
/// 返回 PNG 字节。pdftoppm 缺失给明确诊断(不静默降级)。
/// pub(crate):R-274 批2 PGFPlots 轨复用(图转 PNG 回模型)。
///
/// D-391:显式 `-f 1 -l 1` 只渲染首页——①产物页号恒为 `-1`(pdftoppm 多页时
/// 按总页数零填充,≥10 页产 `-01.png`,只找 `-1.png` 必失败);②长文档不再
/// 全页 150dpi 渲染纯浪费。失败路径也清理临时 PNG(不再提前 return 跳过清理)。
pub(crate) fn pdf_to_png(pdf: &Path, workdir: &Path, stem: &str) -> Result<Vec<u8>, String> {
    if !pdf.is_file() {
        return Err(format!("PDF 不存在: {}", pdf.display()));
    }
    let pdftoppm = which_in_path("pdftoppm").ok_or_else(|| {
        "未找到 pdftoppm(poppler PDF 转 PNG 工具)。MiKTeX/TeX Live 自带;若用 Tectonic 侧车 \
         需单独装 poppler 或将 pdftoppm 放入 PATH。"
            .to_string()
    })?;
    // 输出到 workdir 下的临时前缀,避免污染工件目录。
    let out_prefix = workdir.join(format!("{stem}-pngtmp"));
    // D-391:只渲染首页(-f 1 -l 1)——页号确定 + 长文档不浪费。
    let output = std::process::Command::new(&pdftoppm)
        .args(["-png", "-r", "150", "-f", "1", "-l", "1"])
        .arg(pdf)
        .arg(&out_prefix)
        .current_dir(workdir)
        .output()
        .map_err(|e| format!("启动 pdftoppm 失败: {e}"))?;
    let result = if !output.status.success() {
        Err(format!(
            "pdftoppm 转换失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    } else {
        // D-391:只渲染首页 → 产物唯一;但页号零填充位数取决于总页数
        // (poppler 对 10 页 PDF 产 -01.png,1 页产 -1.png),不猜页号——
        // 直接扫描 <prefix>-*.png 取唯一产物(零填充任意位数都成立)。
        let pngs: Vec<std::path::PathBuf> = std::fs::read_dir(workdir)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| {
                        let n = e.file_name().to_string_lossy().to_string();
                        n.starts_with(&format!("{stem}-pngtmp")) && n.ends_with(".png")
                    })
                    .map(|e| e.path())
                    .collect()
            })
            .unwrap_or_default();
        let first = pngs
            .into_iter()
            .next()
            .ok_or_else(|| "pdftoppm 未产出 PNG(只渲染首页,应有 1 个)".to_string())?;
        std::fs::read(&first).map_err(|e| format!("读取转换产物失败: {e}"))
    };
    // D-391:失败路径也清理临时 PNG(成功/失败统一走这里,不提前 return 跳过)。
    cleanup_pngtmp(workdir, stem);
    result
}

/// D-391:清理 `<stem>-pngtmp*.png` 临时产物(成功/失败路径共用)。
fn cleanup_pngtmp(workdir: &Path, stem: &str) {
    if let Ok(entries) = std::fs::read_dir(workdir) {
        let prefix = format!("{stem}-pngtmp");
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(&prefix) && name.ends_with(".png") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// base64 编码(标准 RFC 4648,无 padding 变体由调用方约定)。
fn base64_engine_encode(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
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
    use serial_test::serial;
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
    /// D-394:加 skip guard——本机无 LaTeX 后端时 compile_latex 走 Missing 分支
    /// (指引文案无行号),测试不得假失败。
    #[test]
    fn 错误诊断含行号() {
        if which_in_path("pdflatex").is_none() && which_in_path("tectonic").is_none() {
            eprintln!("跳过:本机无 LaTeX 后端,无法验证行号诊断");
            return;
        }
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

    /// 后端缺失:给下载指引不崩溃(验收⑥)。D-394:走真生产分支——临时把 PATH
    /// 指向空目录让 detect_backend 真返回 Missing,compile_latex 产出必须等于
    /// 单源 missing_guidance()(不再断言测试内硬编码副本)。
    #[serial]
    #[test]
    fn 后端缺失给下载指引() {
        let dir = temp_dir("missing");
        std::fs::write(
            dir.join("x.tex"),
            "\\documentclass{article}\n\\begin{document}x\\end{document}\n",
        )
        .unwrap();
        let (ok, diag) = with_empty_path(|| compile_latex(&dir, "x.tex"));
        assert!(!ok, "无后端必须失败");
        assert!(
            diag.contains("MiKTeX") && diag.contains("tectonic"),
            "指引要点名 MiKTeX 与 Tectonic: {diag}"
        );
        assert_eq!(diag, missing_guidance(), "真 Missing 分支产出=单源文案");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// D-394/D-584:线程级 PATH 覆写为空——detect_backend/pdf_to_png 的 which_in_path
    /// 查不到任何后端,走真生产 Missing/缺失分支。走 PATH_OVERRIDE 注入缝,
    /// 不改进程级 PATH,并行测试拉起 git/node 不再被误伤。
    fn with_empty_path<T>(f: impl FnOnce() -> T) -> T {
        PATH_OVERRIDE.with(|o| *o.borrow_mut() = Some(String::new()));
        let result = f();
        PATH_OVERRIDE.with(|o| *o.borrow_mut() = None);
        result
    }

    /// R-273 批2:PDF 首页转 PNG(验收②轨迹——编译产物页面转 PNG 被模型消费)。
    /// 用本机 MiKTeX 的 pdftoppm 实测:编译含公式的 .tex → PDF → PNG 字节可解码为 PNG。
    #[test]
    fn pdf首页转png被消费() {
        if !which_in_path("pdflatex").is_some() || !which_in_path("pdftoppm").is_some() {
            eprintln!("跳过:本机无 pdflatex/pdftoppm");
            return;
        }
        let dir = temp_dir("png");
        std::fs::write(
            dir.join("doc.tex"),
            "\\documentclass{article}\n\\usepackage{amsmath}\n\\begin{document}\n$E=mc^2$\n\\end{document}\n",
        )
        .unwrap();
        let (ok, diag) = compile_latex(&dir, "doc.tex");
        assert!(ok, "编译应成功:\n{diag}");
        let png = pdf_to_png(&dir.join("doc.pdf"), &dir, "doc").expect("PDF→PNG 应成功");
        // PNG magic bytes + IHDR:证明是可解码的 PNG 图像(模型消费的前提)。
        assert_eq!(
            &png[..8],
            &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A],
            "PNG 魔数"
        );
        assert!(png.len() > 100, "PNG 不应是空文件");
        // 临时 PNG 已清理(不污染工件目录)。
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.file_name().to_string_lossy().contains("pngtmp"))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        assert!(leftovers.is_empty(), "临时 PNG 必须清理");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// D-391:多页 PDF(论文常态 10+ 页)——首页转 PNG 成功(页号零填充修复:
    /// 只渲染首页 -f 1 -l 1,产物恒为 -1.png,不受 ≥10 页零填充影响),
    /// 且临时 PNG 无残留。
    #[test]
    fn 多页pdf首页转png成功无残留() {
        if !which_in_path("pdflatex").is_some() || !which_in_path("pdftoppm").is_some() {
            eprintln!("跳过:本机无 pdflatex/pdftoppm");
            return;
        }
        let dir = temp_dir("multipage");
        // 10 页文档(论文常态规模;纯英文,避免 pdflatex 默认字体不支持中文)。
        let mut tex = "\\documentclass{article}\n\\begin{document}\n".to_string();
        for i in 1..=10 {
            tex.push_str(&format!("Page number {i}\\newpage\n"));
        }
        tex.push_str("\\end{document}\n");
        std::fs::write(dir.join("paper.tex"), tex).unwrap();
        let (ok, diag) = compile_latex(&dir, "paper.tex");
        assert!(ok, "10 页编译应成功:\n{diag}");
        let png =
            pdf_to_png(&dir.join("paper.pdf"), &dir, "paper").expect("多页 PDF 首页必须转换成功");
        assert_eq!(
            &png[..8],
            &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A],
            "PNG 魔数"
        );
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.file_name().to_string_lossy().contains("pngtmp"))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        assert!(
            leftovers.is_empty(),
            "10 页转换后临时 PNG 必须清理,残留: {:?}",
            leftovers
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// D-391:失败路径也清理临时 PNG(此前失败提前 return 跳过清理循环)。
    /// 用损坏 PDF 触发 pdftoppm 失败,预置假临时文件断言被清。
    #[test]
    fn 转换失败也清理临时png() {
        let dir = temp_dir("failclean");
        // 损坏 PDF(非合法内容,pdftoppm 必失败)。
        std::fs::write(dir.join("bad.pdf"), b"not a real pdf at all").unwrap();
        // 预置假临时文件(模拟上次失败遗留)。
        std::fs::write(dir.join("doc-pngtmp-1.png"), b"junk").unwrap();
        let err = pdf_to_png(&dir.join("bad.pdf"), &dir, "doc").unwrap_err();
        assert!(!err.is_empty(), "必须报错");
        assert!(
            !dir.join("doc-pngtmp-1.png").exists(),
            "失败路径也必须清理临时 PNG"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// D-391:stem 口径统一(file_stem)——含点文件名不再被 split('.') 截断。
    #[test]
    fn stem口径含点文件名不截断() {
        assert_eq!(stem_of("my.paper.tex"), "my.paper", "含点文件名取完整 stem");
        assert_eq!(stem_of("main.tex"), "main");
        assert_eq!(stem_of("doc"), "doc", "无扩展名原样");
    }

    /// R-273 批2:pdftoppm 缺失时给明确诊断(不静默)。D-394:走真生产分支——
    /// 临时清空 PATH + 真实存在的 PDF(否则落在「PDF 不存在」分支,名不副实)。
    #[serial]
    #[test]
    fn pdftoppm缺失给诊断() {
        let dir = temp_dir("nopng");
        // 真实存在的 PDF(内容无关,is_file 检查通过后才会走到 pdftoppm 检测)。
        std::fs::write(dir.join("real.pdf"), b"placeholder").unwrap();
        let err = with_empty_path(|| pdf_to_png(&dir.join("real.pdf"), &dir, "real")).unwrap_err();
        assert!(err.contains("pdftoppm"), "真缺失分支应点名 pdftoppm: {err}");
        assert!(
            !err.contains("PDF 不存在"),
            "不得落在 PDF 不存在分支: {err}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// R-273 批3:假 tectonic 脚本——已预热(--only-cached 成功)断网可编译(验收③前半)。
    /// 用批处理脚本模拟 tectonic:收到 --only-cached 参数即成功并生成 PDF。
    #[test]
    fn tectonic已预热_onlycached成功() {
        let dir = temp_dir("tectonic-warm");
        std::fs::write(
            dir.join("doc.tex"),
            "\\documentclass{article}\n\\begin{document}x\\end{document}\n",
        )
        .unwrap();
        // 假 tectonic:只要带 --only-cached 就成功(已预热),并造一个 PDF 伪文件。
        let fake = dir.join("tectonic.cmd");
        std::fs::write(
            &fake,
            "@echo off\r\nsetlocal\r\nset OUT=%CD%\\doc.pdf\r\ncopy /y NUL \"%OUT%\" >NUL\r\nexit /b 0\r\n",
        )
        .unwrap();
        let (ok, diag) =
            compile_tectonic(&dir, "doc", &dir.join("doc.tex"), fake.to_str().unwrap());
        assert!(ok, "已预热 --only-cached 应成功:\n{diag}");
        assert!(
            diag.contains("--only-cached"),
            "诊断应体现 only-cached 路径: {diag}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// R-273 批3:假 tectonic——未预热(--only-cached 失败)给明确诊断(验收③后半),
    /// 并声明 bib 路线(验收④ Tectonic 路径)。
    #[test]
    fn tectonic未预热_明确诊断含bib声明() {
        let dir = temp_dir("tectonic-cold");
        std::fs::write(
            dir.join("doc.tex"),
            "\\documentclass{article}\n\\begin{document}x\\end{document}\n",
        )
        .unwrap();
        // 假 tectonic:带 --only-cached 时失败(未预热),不带时也失败(断网),绝不造 PDF。
        let fake = dir.join("tectonic.cmd");
        std::fs::write(
            &fake,
            "@echo off\r\necho tectonic: bundle not cached (simulated)\r\nexit /b 1\r\n",
        )
        .unwrap();
        let (ok, diag) =
            compile_tectonic(&dir, "doc", &dir.join("doc.tex"), fake.to_str().unwrap());
        assert!(!ok, "未预热且断网应失败: {diag}");
        assert!(
            diag.contains("未预热") || diag.contains("预热"),
            "诊断应点名未预热: {diag}"
        );
        assert!(
            diag.contains("bib 路线"),
            "诊断应声明 bib 路线(验收④): {diag}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// R-273 验收③:D-394——tectonic 真 exe 至少一次真编译实测(真文档→真 PDF,
    /// 不再用假 .cmd 脚本替代)。本机无 tectonic 时跳过(测试就位,具备 tectonic
    /// 的环境自动真跑,留记录)。
    #[test]
    fn tectonic真exe真编译() {
        let Some(tectonic) = which_in_path("tectonic") else {
            eprintln!("跳过:本机无 tectonic 真 exe;真编译实测由具备 tectonic 的环境执行");
            return;
        };
        let dir = temp_dir("tectonic-real");
        std::fs::write(
            dir.join("doc.tex"),
            "\\documentclass{article}\n\\begin{document}x\\end{document}\n",
        )
        .unwrap();
        let (ok, diag) = compile_tectonic(&dir, "doc", &dir.join("doc.tex"), &tectonic);
        assert!(ok, "真 tectonic 应编译真文档:\n{diag}");
        assert!(dir.join("doc.pdf").is_file(), "真 PDF 必须产出");
        std::fs::remove_dir_all(&dir).ok();
    }
}
