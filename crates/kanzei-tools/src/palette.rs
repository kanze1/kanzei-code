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
}
