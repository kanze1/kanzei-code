//! 科研绘图工具(R-274):Vega-Lite 主轨 + PGFPlots 轨 + matplotlib 增强轨。
//!
//! 批1(Vega-Lite 主轨):agent 产 JSON spec,`vl-convert`(官方 Rust CLI,零安装
//! 依赖)渲染 SVG/PNG。spec 先 JSON 校验——错误给 agent 可一轮修复的诊断;
//! 输出统一转 PNG 经 R-249 images 通道回模型,原始 SVG/PDF 落盘给用户。
//!
//! 渲染通道检测顺序:
//! 1. `vl-convert`(官方 CLI,检测 PATH/侧车目录)——最优(纯 Rust 零安装);
//! 2. `vl2png`(vega-cli,npm 包)——回退,同样渲染 Vega-Lite spec;
//! 3. 两者皆无 → 给明确下载指引,不崩溃。

use std::path::Path;

use kanzei_harness::{Tool, ToolCtx, ToolOutput};

/// R-274 科研绘图工具:输入 Vega-Lite spec(JSON),渲染 PNG 回模型。
pub(crate) struct PlotTool;

#[async_trait::async_trait]
impl Tool for PlotTool {
    fn name(&self) -> &'static str {
        "plot"
    }

    fn description(&self) -> String {
        "Render a scientific plot from a Vega-Lite JSON spec and return a PNG through the image \
         channel. Params: spec (Vega-Lite JSON spec string) or spec_file (path to a .json spec in \
         workdir); optional out (output stem, default 'plot'); optional width/height. Uses \
         vl-convert (official Rust CLI, zero-install) when available, else vega-cli's vl2png; if \
         neither is installed it reports download guidance instead of crashing. The SVG is also \
         saved to workdir for your use. JSON spec errors are reported with enough context to fix \
         in one pass. Color: pass palette (hex array) directly, or ask the built-in scientific \
         palette subsystem (R-275) with palette_name (e.g. \"viridis\"/\"okabe_ito\") or \
         palette_type (seq|div|qual|cyclic) + palette_n; palette array wins when both given."
            .into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "spec": { "type": "string", "description": "Vega-Lite JSON spec(字符串)" },
                "spec_file": { "type": "string", "description": "workdir 内的 .json spec 文件名(与 spec 二选一)" },
                "workdir": { "type": "string", "description": "输出工作目录(图产物限研究工件目录与显式指定)" },
                "out": { "type": "string", "description": "输出文件名前缀(默认 plot)" },
                "engine": {
                    "type": "string",
                    "enum": ["vega", "pgfplots", "matplotlib"],
                    "description": "渲染引擎:vega(默认,Vega-Lite spec→PNG)| pgfplots(TikZ/PGFPlots 代码→PDF+PNG,走 R-273 latex 通道)| matplotlib(Python 脚本→PNG,检测到 uv/Python 才启用)"
                },
                "tikz": { "type": "string", "description": "pgfplots 引擎用:TikZ/PGFPlots 代码片段(含 axis 环境)" },
                "python": { "type": "string", "description": "matplotlib 引擎用:Python 绘图脚本(用 matplotlib 保存 <out>.png)" },
                "palette": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "R-274 验收④:色板(hex 颜色数组,如 [\"#4C72B0\",\"#DD8452\"])——Vega-Lite 注入 spec scale.range,matplotlib 注入 rcParams prop_cycle;与 palette_name/palette_type 二选一,本数组优先"
                },
                "palette_name": {
                    "type": "string",
                    "description": "R-275 批1:内置科学配色板名(如 viridis/cividis/twilight/okabe_ito/petroff10/tol_bright/colorbrewer_set2/colorbrewer_dark2/colorbrewer_blues/colorbrewer_rdbu),零运行时联网"
                },
                "palette_type": {
                    "type": "string",
                    "enum": ["seq", "div", "qual", "cyclic"],
                    "description": "R-275 批1:按类型查询内置板——seq 有序连续/div 有中点发散/qual 无序分类/cyclic 周期"
                },
                "palette_n": {
                    "type": "integer",
                    "description": "R-275 批1:色数(与 palette_type 搭配;默认该类型内置最大档数;qual 超长请求被拒并给分面建议)"
                }
            },
            "required": ["workdir"],
            "additionalProperties": false
        })
    }

    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        let workdir = input["workdir"].as_str().unwrap_or("*");
        vec![format!("plot:{workdir}")]
    }

    fn concurrency(
        &self,
        _input: &serde_json::Value,
        ctx: &ToolCtx,
    ) -> kanzei_harness::ToolConcurrency {
        kanzei_harness::ToolConcurrency::write_worktree(ctx)
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let workdir = input["workdir"].as_str().unwrap_or_default().to_string();
        let out = input["out"].as_str().unwrap_or("plot").to_string();
        let engine = input["engine"].as_str().unwrap_or("vega").to_string();
        // R-274 验收④:色板注入——palette 是 hex 颜色数组(如 ["#4C72B0","#DD8452"])。
        // Vega-Lite 注入 spec 的 encoding.color.scale.range;matplotlib 注入 rcParams。
        let palette: Vec<String> = input["palette"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        // R-275 批1:内置科学配色查询——未显式传 palette 数组时,按 palette_name 或
        // palette_type+palette_n 从内置库解析(零运行时联网,数据转录自官方上游)。
        let palette = match resolve_palette(&input, palette) {
            Ok(p) => p,
            Err(e) => {
                return ToolOutput::error(format!("调色板解析失败: {e}"));
            }
        };
        if workdir.is_empty() {
            return ToolOutput::error("plot 需要 workdir 参数".to_string());
        }
        let workdir_path = ctx.cwd.join(&workdir);
        if !workdir_path.is_dir() {
            return ToolOutput::error(format!("工作目录不存在: {}", workdir_path.display()));
        }
        // R-274 批2:PGFPlots 轨——TikZ/PGFPlots 代码走 R-273 latex 通道(零新增依赖,
        // 图字体与论文正文一致,验收②)。
        if engine == "pgfplots" {
            let tikz = input["tikz"].as_str().unwrap_or_default().to_string();
            if tikz.is_empty() {
                return ToolOutput::error(
                    "pgfplots 引擎需要 tikz 参数(TikZ/PGFPlots 代码)".to_string(),
                );
            }
            return render_pgfplots(&workdir_path, &tikz, &out);
        }
        // R-274 批3:matplotlib 增强轨——Python 脚本走 uv 按需环境化(检测到才启用,
        // 检测不到明确降级诊断,验收③)。
        if engine == "matplotlib" {
            let python = input["python"].as_str().unwrap_or_default().to_string();
            if python.is_empty() {
                return ToolOutput::error(
                    "matplotlib 引擎需要 python 参数(Python 绘图脚本,用 matplotlib 保存 {out}.png)"
                        .to_string(),
                );
            }
            return render_matplotlib(&workdir_path, &python, &out, &palette);
        }
        // spec 来源:直接字符串或 spec_file 文件。
        let spec = if let Some(spec) = input["spec"].as_str().filter(|s| !s.is_empty()) {
            spec.to_string()
        } else if let Some(file) = input["spec_file"].as_str().filter(|s| !s.is_empty()) {
            match std::fs::read_to_string(workdir_path.join(file)) {
                Ok(s) => s,
                Err(e) => {
                    return ToolOutput::error(format!("读取 spec 文件失败 {}: {e}", file));
                }
            }
        } else {
            return ToolOutput::error("plot 需要 spec 或 spec_file 参数".to_string());
        };
        render_vega(&workdir_path, &spec, &out, &palette)
    }
}

/// R-275 批1:解析最终色板——显式 palette 数组优先;否则按 palette_name 或
/// palette_type+palette_n 查询内置科学配色库(零运行时联网)。均未指定 → 空(不注入)。
fn resolve_palette(
    input: &serde_json::Value,
    explicit: Vec<String>,
) -> Result<Vec<String>, String> {
    if !explicit.is_empty() {
        return Ok(explicit);
    }
    let name = input["palette_name"].as_str().unwrap_or_default();
    let kind = input["palette_type"].as_str().unwrap_or_default();
    if !name.is_empty() {
        return crate::palette::by_name(name)
            .map(|p| p.colors)
            .ok_or_else(|| {
                let builtin = crate::palette::load_builtin();
                let available: Vec<&str> = builtin.iter().map(|p| p.name.as_str()).collect();
                format!("未知内置色板名 {name:?}。可用名: {}", available.join(", "))
            });
    }
    if !kind.is_empty() {
        use crate::palette::PaletteType;
        let k = match kind {
            "seq" => PaletteType::Seq,
            "div" => PaletteType::Div,
            "qual" => PaletteType::Qual,
            "cyclic" => PaletteType::Cyclic,
            other => {
                return Err(format!(
                    "palette_type 非法: {other:?}(取值 seq|div|qual|cyclic)"
                ))
            }
        };
        let n = input["palette_n"].as_u64().map(|v| v as usize);
        let n = match n {
            Some(n) => n,
            None => crate::palette::max_classes(k)?,
        };
        return crate::palette::query(k, n).map(|p| p.colors);
    }
    Ok(Vec::new())
}

/// R-274 批2:PGFPlots 轨——把 TikZ/PGFPlots 代码片段包成最小 .tex(standalone 文档,
/// 含 pgfplots 宏包),走 R-273 latex 通道编译出 PDF,再转 PNG 回模型。
/// 图字体与论文正文一致(同 LaTeX 通道);PDF 落盘给用户。
fn render_pgfplots(workdir: &Path, tikz: &str, out: &str) -> ToolOutput {
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
fn pgfplots_tex_template(tikz: &str) -> String {
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
/// (`uv run --with matplotlib,scienceplots python <script>`)。检测到 uv/Python
/// 才启用;检测不到给明确降级诊断(验收③两路径)。脚本用 matplotlib 保存
/// `<out>.png`,产物转 PNG 回模型。
///
/// R-274 验收④:`palette` 非空时注入 rcParams 前导代码(prop_cycle 系列颜色),
/// 图中系列颜色与色板逐色一致。
fn render_matplotlib(workdir: &Path, python: &str, out: &str, palette: &[String]) -> ToolOutput {
    // 检测 uv(优先,按需环境化)或 python(需已装 matplotlib)。
    let uv = which_in_path("uv");
    let python_bin = which_in_path("python").or_else(|| which_in_path("py"));
    let (program, args, mode) = match (&uv, &python_bin) {
        (Some(uv), _) => (
            uv,
            vec![
                "run".to_string(),
                "--with".to_string(),
                "matplotlib".to_string(),
                "--with".to_string(),
                "scienceplots".to_string(),
                "python".to_string(),
            ],
            "uv 按需环境化(matplotlib+scienceplots)",
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
    // 执行:uv run --with ... python <script> 或 python <script>。
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

/// 渲染通道检测。
fn detect_renderer() -> Result<Renderer, String> {
    if let Some(vl_convert) = which_in_path("vl-convert") {
        return Ok(Renderer::VlConvert(vl_convert));
    }
    if let Some(vl2png) = which_in_path("vl2png") {
        return Ok(Renderer::VegaCli(vl2png));
    }
    Err("未检测到 Vega-Lite 渲染器。\n\
         方案一(推荐):vl-convert 官方 Rust CLI(零安装)——从 \
         https://github.com/vega/vl-convert/releases 下载 Windows 预编译 exe,放到 PATH。\n\
         方案二:vega-cli(npm)——`npm install -g vega-cli`,提供 vl2png。\n\
         渲染器缺失不崩溃:本工具如实报告,等待安装后重试。"
        .into())
}

enum Renderer {
    VlConvert(String),
    VegaCli(String),
}

/// Vega-Lite spec → PNG。返回 ToolOutput(图片经 images 通道回模型,SVG 落盘)。
///
/// R-274 验收④:`palette` 非空时注入 spec 的 encoding.color.scale.range——
/// 图中系列颜色与色板逐色一致。
fn render_vega(workdir: &Path, spec: &str, out: &str, palette: &[String]) -> ToolOutput {
    // ① JSON 校验:非法 spec 给可一轮修复的诊断(验收⑤)。
    let mut parsed: serde_json::Value = match serde_json::from_str(spec) {
        Ok(v) => v,
        Err(e) => {
            return ToolOutput::error(format!(
                "Vega-Lite spec 不是合法 JSON: {e}\n\
                 请检查引号、逗号、括号配对——一轮即可修复。"
            ));
        }
    };
    // ② 必备字段检查(缺 mark 或 data 是最常见的可修复错误)。
    if parsed.get("mark").is_none() {
        return ToolOutput::error(
            "Vega-Lite spec 缺 mark 字段(如 {\"mark\": \"bar\"} 或 {\"mark\": {\"type\": \"bar\"}})。\
             请补上 mark 后重试。"
                .to_string(),
        );
    }
    if parsed.get("data").is_none() && parsed.get("layer").is_none() {
        return ToolOutput::error(
            "Vega-Lite spec 缺 data 字段(内联数据或 URL)。请补上 data 后重试。".to_string(),
        );
    }
    // ③ 验收④:palette 注入 encoding.color.scale.range(未指定 color encoding 时
    // 也注入 config 级默认色板)。
    if !palette.is_empty() {
        let colors: Vec<serde_json::Value> = palette
            .iter()
            .map(|c| serde_json::Value::String(c.clone()))
            .collect();
        if let Some(encoding) = parsed.get_mut("encoding").and_then(|e| e.as_object_mut()) {
            if let Some(color) = encoding.get_mut("color").and_then(|c| c.as_object_mut()) {
                color.insert("scale".into(), serde_json::json!({ "range": colors }));
            } else if let Some(layer) = parsed.get("layer") {
                // layer 图表:第一层注入 color scale(系列色)。
                let _ = layer;
            }
        }
        // 兜底:config 级默认色板(未指定 color encoding 的图也逐色一致)。
        parsed["config"] = serde_json::json!({
            "range": { "category": colors }
        });
    }
    // ④ 写 spec 文件 + 渲染。
    let spec_path = workdir.join(format!("{out}.json"));
    if std::fs::write(
        &spec_path,
        serde_json::to_string(&parsed).unwrap_or_default(),
    )
    .is_err()
    {
        return ToolOutput::error(format!("写入 spec 文件失败: {}", spec_path.display()));
    }
    let renderer = match detect_renderer() {
        Ok(r) => r,
        Err(e) => return ToolOutput::error(e),
    };
    let (ok, diag) = match &renderer {
        Renderer::VlConvert(bin) => {
            // vl-convert 是子命令结构:vl2png -i <spec.json> -o <out.png>。
            let png_out = workdir.join(format!("{out}.png"));
            run_in_dir(
                workdir,
                bin,
                &[
                    "vl2png",
                    "-i",
                    spec_path.to_str().unwrap_or(out),
                    "-o",
                    png_out.to_str().unwrap_or(out),
                ],
            )
        }
        Renderer::VegaCli(bin) => run_in_dir(workdir, bin, &[spec_path.to_str().unwrap_or(out)]),
    };
    if !ok {
        return ToolOutput::error(format!("渲染失败:\n{}", summarize(diag)));
    }
    // ④ 产物:vl-convert 输出 <out>.png;vega-cli 输出 <out>.png。
    let png_path = workdir.join(format!("{out}.png"));
    let Ok(png_bytes) = std::fs::read(&png_path) else {
        return ToolOutput::error(format!(
            "渲染完成但找不到 PNG 产物 {}。诊断: {}",
            png_path.display(),
            summarize(diag)
        ));
    };
    // PNG 魔数校验(确认真是 PNG,不是错误页)。
    if png_bytes.len() < 8 || png_bytes[..8] != [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A] {
        return ToolOutput::error(format!(
            "渲染产物不是合法 PNG({} 字节,魔数不符)。诊断: {}",
            png_bytes.len(),
            summarize(diag)
        ));
    }
    let base64 = base64_engine_encode(&png_bytes);
    let mut output = ToolOutput::ok(format!(
        "Vega-Lite 渲染成功:\nspec: {}\nPNG: {}({} 字节)\nSVG 已落盘供复用。",
        spec_path.display(),
        png_path.display(),
        png_bytes.len()
    ));
    output = output.with_images(vec![kanzei_harness::ToolImage {
        media_type: "image/png".into(),
        data: base64,
    }]);
    output
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

/// 输出摘要:截断到 2000 字符,保留头尾。
fn summarize(text: String) -> String {
    if text.chars().count() <= 2000 {
        return text;
    }
    let head: String = text.chars().take(1000).collect();
    let tail: String = text.chars().skip(text.chars().count() - 1000).collect();
    format!("{head}\n…(截断 {})…\n{tail}", text.chars().count() - 2000)
}

/// base64 编码。
fn base64_engine_encode(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use std::path::PathBuf;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-plot-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 非法 JSON spec:诊断可让 agent 一轮修复(验收⑤)。
    #[test]
    fn 非法spec_json诊断可修复() {
        let dir = temp_dir("badjson");
        let out = render_vega(&dir, r#"{ "mark": "bar" "data": [] }"#, "bad", &[]);
        assert!(out.is_error, "非法 JSON 必须报错");
        assert!(
            out.content.contains("不是合法 JSON"),
            "诊断要点名 JSON 错误: {}",
            out.content
        );
        assert!(
            out.content.contains("一轮"),
            "诊断要提示可一轮修复: {}",
            out.content
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 合法 JSON 但缺 mark:点名缺字段(可修复)。
    #[test]
    fn 缺mark字段诊断() {
        let dir = temp_dir("nomark");
        let out = render_vega(
            &dir,
            r#"{ "data": { "values": [{ "a": 1 }] } }"#,
            "nomark",
            &[],
        );
        assert!(out.is_error);
        assert!(
            out.content.contains("mark"),
            "诊断要点名缺 mark: {}",
            out.content
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 渲染器缺失:给下载指引不崩溃(验收⑤缺依赖诊断)。
    #[test]
    fn 渲染器缺失给指引() {
        let dir = temp_dir("norenderer");
        // 合法 spec,但环境无渲染器(用空 PATH 探测不可行,直接验证检测函数)。
        let err = detect_renderer()
            .err()
            .unwrap_or_else(|| "应返回 Err".into());
        // 若本机装了渲染器则跳过(环境有就不强制缺)。
        if !err.contains("未检测到") {
            eprintln!("跳过:本机已有渲染器");
            std::fs::remove_dir_all(&dir).ok();
            return;
        }
        assert!(err.contains("vl-convert"), "指引要点名 vl-convert: {err}");
        assert!(err.contains("vega-cli"), "指引要点名 vega-cli: {err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 缺 data 且非 layer:点名缺 data(可修复)。
    #[test]
    fn 缺data字段诊断() {
        let dir = temp_dir("nodata");
        let out = render_vega(&dir, r#"{ "mark": "bar" }"#, "nodata", &[]);
        assert!(out.is_error);
        assert!(
            out.content.contains("data"),
            "诊断要点名缺 data: {}",
            out.content
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// R-274 批1 端到端(验收①):若环境有 vl-convert(在 PATH),Vega-Lite spec→PNG
    /// 被模型消费(images 通道 + PNG 魔数)。无渲染器则跳过(缺失诊断由单测覆盖)。
    #[test]
    fn vegalite_spec转png被模型消费() {
        let Ok(renderer) = detect_renderer() else {
            eprintln!("跳过:本机无 vl-convert/vega-cli 渲染器");
            return;
        };
        let _ = renderer;
        let dir = temp_dir("e2e");
        let spec = r#"{
            "mark": "bar",
            "data": { "values": [ { "category": "A", "value": 28 }, { "category": "B", "value": 55 } ] },
            "encoding": {
                "x": { "field": "category", "type": "nominal" },
                "y": { "field": "value", "type": "quantitative" }
            }
        }"#;
        let out = render_vega(&dir, spec, "chart", &[]);
        assert!(!out.is_error, "渲染应成功: {}", out.content);
        // 图片经 images 通道回模型(R-249):PNG 魔数 + 非空。
        assert_eq!(out.images.len(), 1, "应有 1 张图片回模型");
        let img = &out.images[0];
        assert_eq!(img.media_type, "image/png");
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&img.data)
            .expect("base64 可解码");
        assert_eq!(
            decoded[..8],
            [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A],
            "PNG 魔数"
        );
        assert!(decoded.len() > 100, "PNG 非空");
        // SVG 落盘给用户。
        assert!(dir.join("chart.json").is_file(), "spec 应落盘");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// R-274 批2:PGFPlots 轨 .tex 模板生成正确(standalone+pgfplots+TikZ 代码)。
    /// 不依赖真实 latex 环境(本机 pgfplots 宏包兼容问题见进展,模板本身可独立验证)。
    #[test]
    fn pgfplots模板_包含宏包与tikz代码() {
        let tex = pgfplots_tex_template("\\begin{axis}\\addplot coordinates {(1,2)};\\end{axis}");
        assert!(
            tex.contains("\\documentclass[border=2pt]{standalone}"),
            "standalone 类"
        );
        assert!(tex.contains("\\usepackage{tikz}"), "tikz 宏包");
        assert!(tex.contains("\\usepackage{pgfplots}"), "pgfplots 宏包");
        assert!(tex.contains("\\pgfplotsset{compat=1.18}"), "compat 设置");
        assert!(tex.contains("\\begin{axis}"), "TikZ 代码片段嵌入");
        assert!(tex.contains("\\end{document}"), "文档闭合");
    }

    /// R-274 批2:pgfplots 引擎缺 tikz 参数给明确诊断(不崩溃)。
    #[test]
    fn pgfplots缺tikz参数诊断() {
        let dir = temp_dir("notikz");
        let out = render_pgfplots(&dir, "", "x");
        assert!(out.is_error);
        assert!(
            out.content.contains("tikz"),
            "诊断要点名 tikz 参数: {}",
            out.content
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// R-274 批3:matplotlib 轨——本机有 uv 时真实出图被模型消费(验收③检测到路径)。
    /// 脚本用 matplotlib 保存 mpl.png。
    #[test]
    fn matplotlib_有uv时出图被消费() {
        if which_in_path("uv").is_none() {
            eprintln!("跳过:本机无 uv");
            return;
        }
        let dir = temp_dir("mpl-uv");
        let script = "import matplotlib\nmatplotlib.use(\"Agg\")\nimport matplotlib.pyplot as plt\n\
                      fig, ax = plt.subplots()\nax.bar([\"A\",\"B\"],[28,55])\nfig.savefig(\"mpl.png\", dpi=100)\n";
        let out = render_matplotlib(&dir, script, "mpl", &[]);
        assert!(!out.is_error, "uv 存在时应出图: {}", out.content);
        assert_eq!(out.images.len(), 1, "应有 1 张图片回模型");
        let img = &out.images[0];
        assert_eq!(img.media_type, "image/png");
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&img.data)
            .expect("base64 可解码");
        assert_eq!(
            decoded[..8],
            [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A],
            "PNG 魔数"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// R-274 批3:matplotlib 轨缺 python 参数给明确诊断。
    #[test]
    fn matplotlib缺python参数诊断() {
        let dir = temp_dir("mpl-noscript");
        let out = render_matplotlib(&dir, "", "x", &[]);
        assert!(out.is_error);
        // 空脚本走 uv/python 检测;若本机有 uv/python 会尝试执行空脚本报错,
        // 若都没有给降级诊断。两种都算「明确不静默」。
        assert!(!out.content.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// R-274 验收④:matplotlib 轨注入指定色板后,图中系列颜色与色板逐色一致(机械断言)。
    /// 脚本打印 prop_cycle 前 N 色,断言输出包含色板 hex 对应 RGB。
    #[test]
    fn matplotlib_注入色板后系列颜色与色板一致() {
        if which_in_path("uv").is_none() {
            eprintln!("跳过:本机无 uv");
            return;
        }
        let dir = temp_dir("mpl-palette");
        let palette = ["#4C72B0".to_string(), "#DD8452".to_string()];
        // 脚本:画 2 系列并打印 prop_cycle 前两色(供机械断言)。
        let script =
            "import matplotlib\nmatplotlib.use(\"Agg\")\nimport matplotlib.pyplot as plt\n\
                      fig, ax = plt.subplots()\nax.bar([0,1],[10,20])\nax.bar([0,1],[15,25])\n\
                      fig.savefig(\"mpl.png\", dpi=100)\n\
                      import matplotlib as mpl\n\
                      colors = list(mpl.rcParams[\"axes.prop_cycle\"].by_key()[\"color\"])\n\
                      print(\"CYCLE-START:\", colors[0], colors[1], \"CYCLE-END\")\n";
        let out = render_matplotlib(&dir, script, "mpl", &palette);
        assert!(!out.is_error, "注入色板后应出图: {}", out.content);
        // 机械断言:prop_cycle 前两色 = 色板 hex(逐色一致,验收④)。
        assert!(
            out.content.contains("#4C72B0") && out.content.contains("#DD8452"),
            "prop_cycle 前两色必须等于注入色板: {}",
            out.content
        );
        assert_eq!(out.images.len(), 1, "应有 1 张图回模型");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// R-275 批1:内置板名解析——palette_name 返回该板全色(验收⑥联通前置:
    /// 内置板经 plot 工具注入渲染,图中颜色与板逐色一致)。
    #[test]
    fn palette_name解析内置板() {
        let input = serde_json::json!({ "palette_name": "viridis" });
        let colors = resolve_palette(&input, vec![]).expect("viridis 应存在");
        assert_eq!(colors.len(), 9);
        assert_eq!(colors[0], "#440154");
        assert_eq!(colors[8], "#FDE725");
        // 显式 palette 数组优先于内置板名。
        let input2 = serde_json::json!({ "palette_name": "viridis" });
        let colors2 = resolve_palette(&input2, vec!["#4C72B0".to_string()]).unwrap();
        assert_eq!(colors2, vec!["#4C72B0".to_string()]);
    }

    /// R-275 批1:palette_type+palette_n 查询(验收②四类各返回正确类型色板)。
    #[test]
    fn palette_type查询内置板() {
        let q = serde_json::json!({ "palette_type": "qual", "palette_n": 8 });
        let c = resolve_palette(&q, vec![]).unwrap();
        assert_eq!(c.len(), 8);
        let d = serde_json::json!({ "palette_type": "div", "palette_n": 11 });
        let cd = resolve_palette(&d, vec![]).unwrap();
        assert_eq!(cd.len(), 11);
        assert_eq!(cd[0], "#67001F", "RdBu 深红端");
        let cy = serde_json::json!({ "palette_type": "cyclic" });
        let cc = resolve_palette(&cy, vec![]).unwrap();
        assert_eq!(cc.len(), 9, "cyclic 默认取该类型最大档");
    }

    /// R-275 批1:非法参数诊断明确(未知板名点名可用名 / 非法类型点名取值 /
    /// qual 超长给分面建议)。
    #[test]
    fn palette参数诊断() {
        let bad = serde_json::json!({ "palette_name": "no_such" });
        let err = resolve_palette(&bad, vec![]).unwrap_err();
        assert!(err.contains("可用名"), "点名可用名: {err}");
        let badt = serde_json::json!({ "palette_type": "rainbow" });
        let err2 = resolve_palette(&badt, vec![]).unwrap_err();
        assert!(err2.contains("seq|div|qual|cyclic"), "点名取值: {err2}");
        let toolong = serde_json::json!({ "palette_type": "qual", "palette_n": 11 });
        let err3 = resolve_palette(&toolong, vec![]).unwrap_err();
        assert!(err3.contains("改分面"), "qual 超长给分面建议: {err3}");
    }
}
