//! 调色板子系统(R-275):内置科学配色打包与统一查询接口(批1)。
//!
//! 批1:内置数据一次性嵌入(`include_str!`,零运行时联网),内部规范 JSON
//! (name/type/colors/max_classes/source_url/license)为唯一真源;按 type+色数
//! 查询返回色板,供 R-274 绘图工具注入。
//! 批2 补推荐规则与校验链(CVD 模拟/CIEDE2000/WCAG 对比度/亮度单调);
//! 批3 补用户导入(hex/.gpl/.ase),用户板同类型优先。
//!
//! 数据转录依据(验收①「与上游源逐色一致」):
//! - ColorBrewer:官方 `colorbrewer2.org/export/colorbrewer.json`(Apache-2.0);
//! - viridis/cividis/twilight:matplotlib 官方 `lib/matplotlib/_cm_listed.py`
//!   256/510 值浮点数据按 `round(i*(n-1)/8)` 位置等距采样转 hex(CC0);
//! - Okabe-Ito:公开标准 8 色(R>=4.0 base 内置,注出处);
//! - petroff10:mpetroff/accessible-color-cycles README 十色环(MIT,arXiv:2107.02270);
//! - Tol bright:SRON Paul Tol 公开 7 色(BSD-3 按 prior_art 对待)。
//!
//! 抽查断言固化在下方 `tests` 中,转录漂移即红。

use serde::Deserialize;

/// 色板类型:seq(有序连续)/ div(有中点发散)/ qual(无序分类)/ cyclic(周期)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaletteType {
    Seq,
    Div,
    Qual,
    Cyclic,
}

impl PaletteType {
    pub fn as_str(&self) -> &'static str {
        match self {
            PaletteType::Seq => "seq",
            PaletteType::Div => "div",
            PaletteType::Qual => "qual",
            PaletteType::Cyclic => "cyclic",
        }
    }
}

/// 内部规范 JSON 的单一色板记录。
#[derive(Debug, Clone, Deserialize)]
pub struct Palette {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: PaletteType,
    /// #RRGGBB(大写)hex 列表,固定顺序即映射顺序。
    pub colors: Vec<String>,
    pub max_classes: usize,
    pub source_url: String,
    pub license: String,
    pub note: String,
}

#[derive(Deserialize)]
struct PaletteStore {
    schema_version: u32,
    palettes: Vec<Palette>,
}

const BUILTIN_JSON: &str = include_str!("../assets/palettes/builtin_palettes.json");

/// 加载全部内置色板(进程内一次解析,零运行时联网)。
pub fn load_builtin() -> Vec<Palette> {
    let store: PaletteStore = serde_json::from_str(BUILTIN_JSON)
        .expect("内置色板 JSON 必须合法(资产随 crate 编译,转录损坏即编译期红)");
    debug_assert_eq!(store.schema_version, 1, "schema 版本升级须同步迁移");
    store.palettes
}

/// 按名查内置板(精确名),返回该板全色。
pub fn by_name(name: &str) -> Option<Palette> {
    load_builtin()
        .into_iter()
        .find(|p| p.name.eq_ignore_ascii_case(name))
}

/// 统一查询接口(验收⑤):按 type+色数返回内置板。
///
/// 语义:
/// - qual(无序分类):只返回 `colors.len() >= n` 的板,优先精确 == n,截取前 n 色;
///   全部不足 → Err,提示改分面/高亮/循环+线型区分(绝不插值)。
/// - seq/div/cyclic(连续/发散/周期):按位置等距采样 n 档(`round(i*(len-1)/(n-1))`,
///   div 的偶数请求在批2 推荐规则中处理奇数档保中点),n 超过内置档位上限 → Err。
pub fn query(kind: PaletteType, n_classes: usize) -> Result<Palette, String> {
    let builtin = load_builtin();
    let candidates: Vec<Palette> = builtin.into_iter().filter(|p| p.kind == kind).collect();
    if candidates.is_empty() {
        return Err(format!("内置色板没有 {} 类型", kind.as_str()));
    }
    if n_classes == 0 {
        return Err("色数 n 必须 ≥1".to_string());
    }
    match kind {
        PaletteType::Qual => {
            let max = candidates.iter().map(|p| p.colors.len()).max().unwrap_or(0);
            let mut exact: Option<Palette> = None;
            let mut larger: Option<Palette> = None;
            for p in candidates {
                if p.colors.len() == n_classes {
                    exact = Some(p);
                    break;
                }
                if p.colors.len() > n_classes {
                    larger = Some(larger.map_or(p.clone(), |cur| {
                        if p.colors.len() < cur.colors.len() {
                            p.clone()
                        } else {
                            cur
                        }
                    }));
                }
            }
            if let Some(p) = exact.or(larger) {
                return Ok(Palette {
                    colors: p.colors[..n_classes].to_vec(),
                    ..p
                });
            }
            Err(format!(
                "定性板请求 {n_classes} 色,超过内置 qual 板最大长度 {max} 色。\
                 建议:改分面(每面板类别数 ≤{max})、高亮关键系列、或接受循环+线型区分兜底;\
                 定性板绝不插值。也可用 palette_name 指定具体板(如 okabe_ito)。"
            ))
        }
        PaletteType::Seq | PaletteType::Div | PaletteType::Cyclic => {
            // 默认板 = 同长取先序(JSON 文件顺序即默认优先级:seq 默认 viridis,
            // div 默认 RdBu,cyclic 默认 twilight)。
            let p = candidates
                .into_iter()
                .reduce(|acc, c| {
                    if c.colors.len() > acc.colors.len() {
                        c
                    } else {
                        acc
                    }
                })
                .ok_or_else(|| format!("内置色板没有 {} 类型", kind.as_str()))?;
            if n_classes > p.colors.len() {
                return Err(format!(
                    "{} 板内置最高 {} 档,请求 {n_classes} 档超上限。\
                     请改用 ≤{} 档,或显式传 palette(hex 数组)。",
                    p.name,
                    p.colors.len(),
                    p.colors.len()
                ));
            }
            Ok(sample(&p, n_classes))
        }
    }
}

/// 某类型内置板的最大档数(供默认色数解析)。
pub fn max_classes(kind: PaletteType) -> Result<usize, String> {
    load_builtin()
        .iter()
        .filter(|p| p.kind == kind)
        .map(|p| p.colors.len())
        .max()
        .ok_or_else(|| format!("内置色板没有 {} 类型", kind.as_str()))
}

/// 连续板按位置等距采样 n 档(n==1 取中点;n==len 原样返回)。
fn sample(p: &Palette, n: usize) -> Palette {
    let len = p.colors.len();
    if n >= len {
        return p.clone();
    }
    let indices: Vec<usize> = if n == 1 {
        vec![len / 2]
    } else {
        (0..n)
            .map(|i| ((i * (len - 1)) as f64 / (n - 1) as f64).round() as usize)
            .collect()
    };
    Palette {
        colors: indices.into_iter().map(|i| p.colors[i].clone()).collect(),
        ..p.clone()
    }
}

// ============================ 批2:推荐规则 ============================

/// 数据特征(绘图字段语义)→ 推荐色板类型(Vega-Lite 按字段类型默认规则先例)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataFeature {
    /// 无序分类(品种/地区/分组)→ qual,≤12 色
    Nominal,
    /// 有序连续(温度/密度/概率)→ seq
    Sequential,
    /// 有中点发散(偏离基线/差值)→ div
    Diverging,
    /// 周期相位(一天内时间/方位角)→ cyclic
    Cyclic,
}

impl DataFeature {
    pub fn as_str(&self) -> &'static str {
        match self {
            DataFeature::Nominal => "nominal",
            DataFeature::Sequential => "sequential",
            DataFeature::Diverging => "diverging",
            DataFeature::Cyclic => "cyclic",
        }
    }

    pub fn to_kind(&self) -> PaletteType {
        match self {
            DataFeature::Nominal => PaletteType::Qual,
            DataFeature::Sequential => PaletteType::Seq,
            DataFeature::Diverging => PaletteType::Div,
            DataFeature::Cyclic => PaletteType::Cyclic,
        }
    }

    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "nominal" => Ok(DataFeature::Nominal),
            "sequential" => Ok(DataFeature::Sequential),
            "diverging" => Ok(DataFeature::Diverging),
            "cyclic" => Ok(DataFeature::Cyclic),
            other => Err(format!(
                "palette_feature 非法: {other:?}(取值 nominal|sequential|diverging|cyclic)"
            )),
        }
    }
}

/// 硬禁忌板名:彩虹渐变用于连续量会制造不存在的数值结构(Crameri et al. 2020,
/// Nature Communications 11:5444)。机械拒绝,不靠模型自觉。
const HARD_FORBIDDEN_SEQ: &[&str] = &[
    "jet",
    "rainbow",
    "hsv",
    "turbo",
    "gist_rainbow",
    "nipy_spectral",
];

/// 硬禁忌检查:点名板用于连续量(seq/div/cyclic)时机械拒绝。
pub fn check_hard_forbidden(name: &str, kind: PaletteType) -> Result<(), String> {
    let lower = name.to_ascii_lowercase();
    if matches!(
        kind,
        PaletteType::Seq | PaletteType::Div | PaletteType::Cyclic
    ) && HARD_FORBIDDEN_SEQ.iter().any(|f| *f == lower)
    {
        return Err(format!(
            "硬禁忌拒绝: {name} 是彩虹渐变,用于连续量会制造视觉假象(Crameri et al. 2020)。\
             请改用科学序列板(viridis/cividis/colorbrewer_blues)或发散板(colorbrewer_rdbu)。"
        ));
    }
    Ok(())
}

/// 推荐规则(批2):按数据特征+类别数返回内置板;连续类目走 query 按位采样,
/// 定性走精确匹配,超长拒绝带分面建议。
pub fn recommend(feature: DataFeature, n_classes: usize) -> Result<Palette, String> {
    if n_classes > 12 && feature == DataFeature::Nominal {
        // 无序分类 >12 色:即使 qual 板足够,肉眼也已无法区分(先例:Vega-Lite
        // 默认 tableau10 上限),直接建议改分面/高亮,不硬给超长定性板。
        return Err(format!(
            "无序分类请求 {n_classes} 类 >12:定性板超 12 色人眼难以区分。\
             建议改分面、高亮关键系列,或接受循环+线型区分兜底。"
        ));
    }
    query(feature.to_kind(), n_classes)
}

// ============================ 批2:校验链 ============================

/// Machado 2009(Oliveira & Fernandes,"A Physiologically-based Model for Simulation
/// of Color Vision Deficiency",IEEE TVCG)全色盲模拟矩阵,线性 RGB 域。
/// 本批采用 severity=1.0 的标准矩阵(多个开源实现一致,如 daltonize/colorspacious)。
const CVD_MATRICES: &[(&str, [[f32; 3]; 3])] = &[
    (
        "protanopia",
        [
            [0.152_286, 1.052_583, -0.204_868],
            [0.114_503, 0.786_281, 0.099_216],
            [-0.003_882, -0.048_116, 1.051_998],
        ],
    ),
    (
        "deuteranopia",
        [
            [0.367_322, 0.860_646, -0.227_968],
            [0.280_085, 0.672_501, 0.047_413],
            [-0.011_820, 0.042_940, 0.968_881],
        ],
    ),
    (
        "tritanopia",
        [
            [1.255_528, -0.076_749, -0.178_779],
            [-0.078_411, 0.930_809, 0.147_602],
            [0.004_733, 0.691_367, 0.303_900],
        ],
    ),
];

/// 校验链单项结果。
#[derive(Debug, Clone)]
pub struct CheckItem {
    pub name: &'static str,
    pub passed: bool,
    pub detail: String,
}

/// 点名冲突色对(区分度最差的一对)。
#[derive(Debug, Clone)]
pub struct ColorPair {
    pub a: String,
    pub b: String,
    /// 原色两两 CIEDE2000。
    pub delta_e: f64,
    /// 最差 CVD 模拟下的 CIEDE2000(色盲区分度)。
    pub delta_e_cvd: f64,
    /// WCAG 图形对比度。
    pub contrast: f64,
}

/// 校验链报告:score 0-100,导入即评分(批3 用户导入复用本函数)。
#[derive(Debug, Clone)]
pub struct ValidationReport {
    pub score: u8,
    pub items: Vec<CheckItem>,
    pub worst_pairs: Vec<ColorPair>,
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// WCAG 2.x 相对亮度(0-1)。
fn relative_luminance(rgb: (f32, f32, f32)) -> f32 {
    0.2126 * srgb_to_linear(rgb.0) + 0.7152 * srgb_to_linear(rgb.1) + 0.0722 * srgb_to_linear(rgb.2)
}

/// WCAG 对比度(≥3:1 为图形对比度通过线)。
fn wcag_contrast(l1: f32, l2: f32) -> f32 {
    let (hi, lo) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
    (hi + 0.05) / (lo + 0.05)
}

/// Machado CVD 模拟:线性 RGB 域矩阵乘法后回到 sRGB。
fn simulate_cvd(rgb: (f32, f32, f32), m: &[[f32; 3]; 3]) -> (f32, f32, f32) {
    let r = srgb_to_linear(rgb.0);
    let g = srgb_to_linear(rgb.1);
    let b = srgb_to_linear(rgb.2);
    let sr = m[0][0] * r + m[0][1] * g + m[0][2] * b;
    let sg = m[1][0] * r + m[1][1] * g + m[1][2] * b;
    let sb = m[2][0] * r + m[2][1] * g + m[2][2] * b;
    (
        linear_to_srgb(sr).clamp(0.0, 1.0),
        linear_to_srgb(sg).clamp(0.0, 1.0),
        linear_to_srgb(sb).clamp(0.0, 1.0),
    )
}

fn parse_hex(s: &str) -> Option<(f32, f32, f32)> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()? as f32 / 255.0;
    let g = u8::from_str_radix(&s[2..4], 16).ok()? as f32 / 255.0;
    let b = u8::from_str_radix(&s[4..6], 16).ok()? as f32 / 255.0;
    Some((r, g, b))
}

fn srgb_to_lab(rgb: (f32, f32, f32)) -> palette::Lab {
    use palette::FromColor;
    let srgb = palette::Srgb::new(rgb.0, rgb.1, rgb.2);
    palette::Lab::from_color(srgb)
}

/// 两两 CIEDE2000(palette crate 内置;批2 验收③的点名依据)。
fn delta_e2000(a: (f32, f32, f32), b: (f32, f32, f32)) -> f32 {
    use palette::color_difference::Ciede2000;
    srgb_to_lab(a).difference(srgb_to_lab(b))
}

/// 连续板亮度单调性:Lab L* 相邻差方向一致性(允许 1 个单位容差)。
fn luminance_monotonicity(rgb: &[(f32, f32, f32)]) -> (f32, String) {
    let l: Vec<f32> = rgb.iter().map(|c| srgb_to_lab(*c).l).collect();
    let n = l.len();
    if n <= 2 {
        return (1.0, format!("仅 {n} 色,无需亮度单调检查"));
    }
    let mut consistent = 0usize;
    for i in 1..n - 1 {
        let d1 = l[i] - l[i - 1];
        let d2 = l[i + 1] - l[i];
        if d1.abs() < 1.0 || d2.abs() < 1.0 || d1.signum() == d2.signum() {
            consistent += 1;
        }
    }
    let ratio = consistent as f32 / (n - 2).max(1) as f32;
    (
        ratio,
        format!("Lab L* 单调方向一致 {consistent}/{}({} 段)", n - 2, n - 2),
    )
}

/// 校验链:hex 色板 → 报告。CVD 模拟(Machado 三矩阵)→ 两两 CIEDE2000
/// (原色 + 最差 CVD)→ WCAG 图形对比度 ≥3:1 → 连续板亮度单调性。
pub fn validate(colors: &[String], kind: PaletteType) -> ValidationReport {
    let rgb: Vec<(f32, f32, f32)> = colors.iter().filter_map(|c| parse_hex(c)).collect();
    if rgb.len() < 2 || rgb.len() != colors.len() {
        // 非法 hex 或色数不足:评 0 分并点名问题。
        return ValidationReport {
            score: 0,
            items: vec![CheckItem {
                name: "parse",
                passed: false,
                detail: format!(
                    "色板解析失败:{}色中有{}个非法 hex 或不足 2 色",
                    colors.len(),
                    colors.len().saturating_sub(rgb.len())
                ),
            }],
            worst_pairs: Vec::new(),
        };
    }
    let mut pairs: Vec<ColorPair> = Vec::new();
    for i in 0..rgb.len() {
        for j in (i + 1)..rgb.len() {
            let de = delta_e2000(rgb[i], rgb[j]);
            let mut de_cvd = de;
            for (_, m) in CVD_MATRICES {
                let sim_a = simulate_cvd(rgb[i], m);
                let sim_b = simulate_cvd(rgb[j], m);
                let d = delta_e2000(sim_a, sim_b);
                if d < de_cvd {
                    de_cvd = d;
                }
            }
            let con = wcag_contrast(relative_luminance(rgb[i]), relative_luminance(rgb[j]));
            pairs.push(ColorPair {
                a: colors[i].clone(),
                b: colors[j].clone(),
                delta_e: de as f64,
                delta_e_cvd: de_cvd as f64,
                contrast: con as f64,
            });
        }
    }
    let n = pairs.len().max(1);
    let contrast_ok = pairs.iter().filter(|p| p.contrast >= 3.0).count();
    let cvd_ok = pairs.iter().filter(|p| p.delta_e_cvd >= 10.0).count();
    let contrast_ratio = contrast_ok as f32 / n as f32;
    let cvd_ratio = cvd_ok as f32 / n as f32;
    let (mono_ratio, mono_detail) = if kind == PaletteType::Seq {
        luminance_monotonicity(&rgb)
    } else {
        (1.0, "非连续板不检查亮度单调".into())
    };
    // 评分:对比度 40 + CVD 区分度 40 + 亮度单调 20。
    let score =
        ((contrast_ratio * 40.0 + cvd_ratio * 40.0 + mono_ratio * 20.0).round() as u8).min(100);
    // 点名冲突色对:按最差 CVD 区分度升序取前 5(验收③「点名冲突色对」)。
    let mut worst = pairs.clone();
    worst.sort_by(|a, b| {
        a.delta_e_cvd
            .partial_cmp(&b.delta_e_cvd)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    worst.truncate(5);
    let items = vec![
        CheckItem {
            name: "wcag_contrast_3_1",
            passed: contrast_ratio >= 0.9,
            detail: format!("{contrast_ok}/{n} 对达到 WCAG 图形对比度 ≥3:1"),
        },
        CheckItem {
            name: "cvd_delta_e2000",
            passed: cvd_ratio >= 0.9,
            detail: format!("{cvd_ok}/{n} 对在最差 CVD 模拟(Machado 2009)下 CIEDE2000 ≥10"),
        },
        CheckItem {
            name: "luminance_monotonic",
            passed: mono_ratio >= 0.9,
            detail: mono_detail,
        },
    ];
    ValidationReport {
        score,
        items,
        worst_pairs: worst,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 验收①:内置板数量与四类覆盖(每类至少 1 板)。
    #[test]
    fn 内置数据四类覆盖且字段齐全() {
        let all = load_builtin();
        assert_eq!(all.len(), 10, "批1 内置 10 板");
        for t in [
            PaletteType::Seq,
            PaletteType::Div,
            PaletteType::Qual,
            PaletteType::Cyclic,
        ] {
            let n = all.iter().filter(|p| p.kind == t).count();
            assert!(n >= 1, "{} 类型至少 1 板,实际 {n}", t.as_str());
        }
        for p in &all {
            assert!(!p.name.is_empty());
            assert!(!p.colors.is_empty());
            assert!(!p.source_url.is_empty(), "{}: source_url 必填", p.name);
            assert!(!p.license.is_empty(), "{}: license 必填", p.name);
            assert!(!p.note.is_empty(), "{}: note 必填", p.name);
            assert_eq!(
                p.max_classes,
                p.colors.len(),
                "{}: max_classes=色数",
                p.name
            );
            for c in &p.colors {
                assert!(
                    c.len() == 7
                        && c.starts_with('#')
                        && c[1..].chars().all(|ch| ch.is_ascii_hexdigit()),
                    "{}: 非法 hex 色 {c}",
                    p.name
                );
            }
        }
    }

    /// 验收①:与上游源逐色一致(抽查断言,各源族取代表板;转录漂移即红)。
    #[test]
    fn 内置色板与上游源逐色一致_抽查() {
        // ColorBrewer 官方 JSON(2026-08-16 抓取 colorbrewer2.org/export/colorbrewer.json)。
        assert_eq!(
            by_name("colorbrewer_set2").unwrap().colors,
            [
                "#66C2A5", "#FC8D62", "#8DA0CB", "#E78AC3", "#A6D854", "#FFD92F", "#E5C494",
                "#B3B3B3"
            ]
        );
        assert_eq!(
            by_name("colorbrewer_dark2").unwrap().colors,
            [
                "#1B9E77", "#D95F02", "#7570B3", "#E7298A", "#66A61E", "#E6AB02", "#A6761D",
                "#666666"
            ]
        );
        assert_eq!(by_name("colorbrewer_blues").unwrap().colors[8], "#08306B");
        assert_eq!(by_name("colorbrewer_rdbu").unwrap().colors[0], "#67001F");
        assert_eq!(by_name("colorbrewer_rdbu").unwrap().colors[5], "#F7F7F7");
        // matplotlib 官方 _cm_listed.py 256 值采样(viridis[0]/[128]/[255] 等)。
        assert_eq!(by_name("viridis").unwrap().colors[0], "#440154");
        assert_eq!(by_name("viridis").unwrap().colors[4], "#21918C");
        assert_eq!(by_name("viridis").unwrap().colors[8], "#FDE725");
        assert_eq!(by_name("cividis").unwrap().colors[0], "#00224E");
        assert_eq!(by_name("cividis").unwrap().colors[8], "#FEE838");
        // twilight 绕环闭合(首尾同色 = cyclic)。
        let tw = by_name("twilight").unwrap();
        assert_eq!(tw.colors[0], tw.colors[8], "cyclic 首尾闭合");
        assert_eq!(tw.colors[0], "#E2D9E2");
        // petroff10(arXiv:2107.02270 README 十色环)。
        assert_eq!(by_name("petroff10").unwrap().colors[0], "#3F90DA");
        assert_eq!(by_name("petroff10").unwrap().colors[9], "#92DADD");
        // Tol bright(SRON 公开 7 色)。
        assert_eq!(
            by_name("tol_bright").unwrap().colors,
            ["#4477AA", "#EE6677", "#228833", "#CCBB44", "#66CCEE", "#AA3377", "#BBBBBB"]
        );
        // Okabe-Ito 标准 8 色。
        assert_eq!(
            by_name("okabe_ito").unwrap().colors,
            [
                "#000000", "#E69F00", "#56B4E9", "#009E73", "#F0E442", "#0072B2", "#D55E00",
                "#CC79A7"
            ]
        );
    }

    /// 验收②(批1 部分):四类数据特征各返回正确类型色板。
    #[test]
    fn 四类查询各返回正确类型() {
        assert_eq!(query(PaletteType::Qual, 8).unwrap().kind, PaletteType::Qual);
        assert_eq!(query(PaletteType::Seq, 5).unwrap().kind, PaletteType::Seq);
        assert_eq!(query(PaletteType::Div, 5).unwrap().kind, PaletteType::Div);
        assert_eq!(
            query(PaletteType::Cyclic, 5).unwrap().kind,
            PaletteType::Cyclic
        );
        assert_eq!(query(PaletteType::Seq, 5).unwrap().colors.len(), 5);
        assert_eq!(query(PaletteType::Div, 5).unwrap().colors.len(), 5);
        assert_eq!(query(PaletteType::Cyclic, 5).unwrap().colors.len(), 5);
    }

    /// 验收②:qual 查询返回 ==n 的精确板(8 色请求得到 okabe_ito 或 set2 全 8 色)。
    #[test]
    fn qual精确匹配返回全色板() {
        let p = query(PaletteType::Qual, 8).unwrap();
        assert_eq!(p.colors.len(), 8);
        // 任一内置 8 色 qual 板整体一致(无截断插值)。
        let full = by_name(&p.name).unwrap();
        assert_eq!(p.colors, full.colors, "qual 查询不插值不截断语义错误");
    }

    /// 验收⑤:定性板超长请求默认被拒,并给分面建议。
    #[test]
    fn 定性板超长请求被拒并给建议() {
        let err = query(PaletteType::Qual, 11).unwrap_err();
        assert!(err.contains("改分面"), "建议改分面: {err}");
        assert!(
            err.contains("不插值") || err.contains("绝不插值"),
            "声明不插值: {err}"
        );
        // 上限内正常返回。
        assert!(query(PaletteType::Qual, 7).is_ok());
    }

    /// 连续板按位置采样:n 档等距,两端与整板两端一致。
    #[test]
    fn 连续板等距采样保持两端() {
        let v = by_name("viridis").unwrap();
        let s3 = query(PaletteType::Seq, 3).unwrap();
        assert_eq!(s3.colors[0], v.colors[0], "首端一致");
        assert_eq!(s3.colors[2], v.colors[8], "末端一致");
        assert_eq!(s3.colors[1], v.colors[4], "中点=第 4 档");
        // n==1 取中点。
        let s1 = query(PaletteType::Seq, 1).unwrap();
        assert_eq!(s1.colors[0], v.colors[4]);
        // 超过内置档位上限拒绝。
        assert!(query(PaletteType::Seq, 10).is_err());
    }

    /// 未知板名 / 0 色 / 非法类型参数给明确错误。
    #[test]
    fn 非法参数诊断明确() {
        assert!(by_name("no_such_palette").is_none());
        assert!(query(PaletteType::Qual, 0).is_err());
        assert!(query(PaletteType::Seq, 0).is_err());
    }

    /// 验收②:四类数据特征各返回正确类型色板;jet 用于连续量被拒(硬禁忌)。
    #[test]
    fn 推荐规则四类映射且jet拒绝() {
        assert_eq!(
            recommend(DataFeature::Nominal, 8).unwrap().kind,
            PaletteType::Qual
        );
        assert_eq!(
            recommend(DataFeature::Sequential, 5).unwrap().kind,
            PaletteType::Seq
        );
        assert_eq!(
            recommend(DataFeature::Diverging, 5).unwrap().kind,
            PaletteType::Div
        );
        assert_eq!(
            recommend(DataFeature::Cyclic, 5).unwrap().kind,
            PaletteType::Cyclic
        );
        // 硬禁忌:jet/rainbow 用于连续量被机械拒绝(Crameri 2020)。
        assert!(
            check_hard_forbidden("jet", PaletteType::Seq).is_err(),
            "jet 用于 seq 必须拒"
        );
        assert!(
            check_hard_forbidden("rainbow", PaletteType::Div).is_err(),
            "rainbow 用于 div 必须拒"
        );
        assert!(
            check_hard_forbidden("jet", PaletteType::Qual).is_ok(),
            "qual 不在连续量禁忌范围"
        );
        // 无序分类 >12 色拒绝并给改分面建议。
        let err = recommend(DataFeature::Nominal, 13).unwrap_err();
        assert!(err.contains("改分面"), "点名改分面: {err}");
    }

    /// 验收③:构造红绿不安全板,校验链给低分并点名冲突色对(实测输出)。
    #[test]
    fn 红绿板低分并点名冲突对() {
        let report = validate(&["#FF0000".into(), "#00FF00".into()], PaletteType::Seq);
        assert!(report.score < 70, "红绿板必须低分,实际 {}", report.score);
        assert!(!report.worst_pairs.is_empty(), "必须点名冲突色对");
        let p = &report.worst_pairs[0];
        assert!(
            (p.a == "#FF0000" && p.b == "#00FF00") || (p.a == "#00FF00" && p.b == "#FF0000"),
            "点名冲突色对: {} vs {}",
            p.a,
            p.b
        );
        assert!(
            p.contrast < 3.2,
            "红绿对比度低于图形对比度线 3:1: {}",
            p.contrast
        );
        // 对照:viridis 应显著高于红绿板(连续单调+CVD 安全)。
        let v = by_name("viridis").unwrap();
        let vreport = validate(&v.colors, PaletteType::Seq);
        assert!(
            vreport.score > report.score,
            "viridis {} 应高于红绿 {}",
            vreport.score,
            report.score
        );
    }

    /// 校验链数值环节:WCAG 对比度基准与 CVD 退化方向。
    #[test]
    fn 校验链数值环节() {
        // WCAG 相对亮度/对比度:黑白应为 21:1。
        let c = wcag_contrast(
            relative_luminance((0.0, 0.0, 0.0)),
            relative_luminance((1.0, 1.0, 1.0)),
        );
        assert!((c - 21.0).abs() < 0.5, "黑白对比应为 21:1,实际 {c}");
        // 灰对灰低对比(<1.5)。
        let g1 = wcag_contrast(
            relative_luminance((0.5, 0.5, 0.5)),
            relative_luminance((0.55, 0.55, 0.55)),
        );
        assert!(g1 < 1.5, "灰对灰对比应 <1.5,实际 {g1}");
        // CVD 模拟:deutan 下红绿区分度下降(色盲混淆的本质)。
        let de_raw = delta_e2000((1.0, 0.0, 0.0), (0.0, 1.0, 0.0));
        let red_sim = simulate_cvd((1.0, 0.0, 0.0), &CVD_MATRICES[1].1);
        let green_sim = simulate_cvd((0.0, 1.0, 0.0), &CVD_MATRICES[1].1);
        let de_cvd = delta_e2000(red_sim, green_sim);
        assert!(
            de_cvd < de_raw,
            "deutan 下红绿区分度应下降: {de_raw} -> {de_cvd}"
        );
        // validate 输出的冲突对 CVD 退化(delta_e_cvd < delta_e)。
        let report = validate(&["#FF0000".into(), "#00FF00".into()], PaletteType::Qual);
        let pair = &report.worst_pairs[0];
        assert!(
            pair.delta_e_cvd < pair.delta_e,
            "CVD 退化应被点名: {} < {}",
            pair.delta_e_cvd,
            pair.delta_e
        );
    }
}
