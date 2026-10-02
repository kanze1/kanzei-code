//! Explicit publication/Python adapters; dependencies are detected only on dispatch.
use super::{base64_engine_encode, run_in_dir, summarize, which_in_path};
use kanzei_harness::ToolOutput;
use std::path::Path;

/// R-274 批2:PGFPlots 轨——把 TikZ/PGFPlots 代码片段包成最小 .tex(standalone 文档,
/// 含 pgfplots 宏包),走 R-273 latex 通道编译出 PDF,再转 PNG 回模型。
/// 图字体与论文正文一致(同 LaTeX 通道);PDF 落盘给用户。
pub(super) fn render_pgfplots(workdir: &Path, tikz: &str, out: &str) -> ToolOutput {
    if tikz.trim().is_empty() {
        return ToolOutput::error(
            "pgfplots 引擎需要 tikz 参数(TikZ/PGFPlots 代码片段)".to_string(),
        );
    }
    let tex = pgfplots_tex_template(tikz);
    let tex_path = workdir.join(format!("{out}.tex"));
    if std::fs::write(&tex_path, &tex).is_err() {
        return ToolOutput::error(format!("写入 .tex 失败: {}", tex_path.display()));
    }
    // 走 R-273 latex 通道(系统发行优先/回落 Tectonic)。
    let (ok, diag) = crate::latex_tool::compile_latex(workdir, &format!("{out}.tex"));
    if !ok {
        return ToolOutput::error(format!("PGFPlots 编译失败:\n{diag}"));
    }
    // PDF 首页转 PNG 回模型(复用 R-273 pdf_to_png)。
    let pdf = workdir.join(format!("{out}.pdf"));
    match crate::latex_tool::pdf_to_png(&pdf, workdir, out) {
        Ok(png_bytes) => {
            let base64 = base64_engine_encode(&png_bytes);
            let mut output = ToolOutput::ok(format!(
                "PGFPlots 渲染成功:\ntex: {}\nPDF: {}(已落盘)\nPNG({} 字节)已回模型",
                tex_path.display(),
                pdf.display(),
                png_bytes.len()
            ));
            output = output.with_images(vec![kanzei_harness::ToolImage {
                media_type: "image/png".into(),
                data: base64,
            }]);
            output
        }
        Err(e) => ToolOutput::ok(format!("{diag}\n[PNG] 转换失败(PDF 已产出): {e}")),
    }
}

/// PGFPlots .tex 模板:standalone 文档 + pgfplots 宏包 + TikZ 代码片段。
/// 独立函数便于单测(不依赖真实 latex 环境)。
pub(super) fn pgfplots_tex_template(tikz: &str) -> String {
    format!(
        "\\documentclass[border=2pt]{{standalone}}\n\
         \\usepackage{{tikz}}\n\
         \\usepackage{{pgfplots}}\n\
         \\pgfplotsset{{compat=1.18}}\n\
         \\begin{{document}}\n\
         {tikz}\n\
         \\end{{document}}\n"
    )
}

/// R-274 批3:matplotlib 增强轨——Python 绘图脚本走 uv 按需环境化
/// (`uv run --isolated --with matplotlib,scienceplots python <script>`)。检测到 uv/Python
/// 才启用;检测不到给明确降级诊断(验收③两路径)。脚本用 matplotlib 保存
/// `<out>.png`,产物转 PNG 回模型。
///
/// R-274 验收④:`palette` 非空时注入 rcParams 前导代码(prop_cycle 系列颜色),
/// 图中系列颜色与色板逐色一致。
pub(super) fn render_matplotlib(
    workdir: &Path,
    python: &str,
    out: &str,
    palette: &[String],
) -> ToolOutput {
    // 检测 uv(优先,按需环境化)或 python(需已装 matplotlib)。
    let uv = which_in_path("uv");
    let python_bin = which_in_path("python").or_else(|| which_in_path("py"));
    let (program, args, mode) = match (&uv, &python_bin) {
        (Some(uv), _) => (
            uv,
            vec![
                "run".to_string(),
                "--isolated".to_string(),
                "--with".to_string(),
                "matplotlib".to_string(),
                "--with".to_string(),
                "scienceplots".to_string(),
                "python".to_string(),
            ],
            "uv 隔离按需环境化(matplotlib+scienceplots)",
        ),
        (None, Some(py)) => (
            py,
            vec![],
            "系统 Python(需已安装 matplotlib;缺则运行时报错)",
        ),
        (None, None) => {
            return ToolOutput::error(
                "未检测到 uv 或 Python——matplotlib 增强轨不可用。\n\
                 方案一(推荐):安装 uv(`pip install uv` 或 https://astral.sh/uv),本工具用 \
                 `uv run --with matplotlib,scienceplots` 按需环境化,零全局安装。\n\
                 方案二:安装 Python 与 matplotlib,放入 PATH。\n\
                 检测不到明确降级:本工具如实报告,vega/pgfplots 轨不受影响。"
                    .to_string(),
            );
        }
    };
    // R-274 验收④:palette 注入 rcParams 前导代码(prop_cycle 设置系列颜色)。
    let mut script = String::new();
    if !palette.is_empty() {
        let colors = palette
            .iter()
            .map(|c| format!("\"{c}\""))
            .collect::<Vec<_>>()
            .join(", ");
        script.push_str(&format!(
            "import matplotlib\nmatplotlib.rcParams['axes.prop_cycle'] = matplotlib.cycler(color=[{colors}])\n"
        ));
    }
    script.push_str(python);
    // 写 Python 脚本到工作目录。
    let script_path = workdir.join(format!("{out}.py"));
    if std::fs::write(&script_path, &script).is_err() {
        return ToolOutput::error(format!("写入 Python 脚本失败: {}", script_path.display()));
    }
    // 执行:uv run --isolated --with ... python <script> 或 python <script>。
    let mut full_args = args;
    full_args.push(script_path.to_str().unwrap_or(out).to_string());
    let arg_refs: Vec<&str> = full_args.iter().map(String::as_str).collect();
    let (ok, diag) = run_in_dir(workdir, program.as_str(), arg_refs.as_slice());
    if !ok {
        return ToolOutput::error(format!("matplotlib 执行失败({mode}):\n{}", summarize(diag)));
    }
    let png_path = workdir.join(format!("{out}.png"));
    let Ok(png_bytes) = std::fs::read(&png_path) else {
        return ToolOutput::error(format!(
            "matplotlib 脚本执行成功但找不到 PNG 产物 {}(脚本需保存 {}.png)。诊断: {}",
            png_path.display(),
            out,
            summarize(diag)
        ));
    };
    if png_bytes.len() < 8 || png_bytes[..8] != [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A] {
        return ToolOutput::error(format!(
            "matplotlib 产物不是合法 PNG({} 字节)。诊断: {}",
            png_bytes.len(),
            summarize(diag)
        ));
    }
    let base64 = base64_engine_encode(&png_bytes);
    let mut output = ToolOutput::ok(format!(
        "matplotlib 渲染成功({mode}):\nscript: {}\nPNG: {}({} 字节)\n{}",
        script_path.display(),
        png_path.display(),
        png_bytes.len(),
        summarize(diag)
    ));
    output = output.with_images(vec![kanzei_harness::ToolImage {
        media_type: "image/png".into(),
        data: base64,
    }]);
    output
}
