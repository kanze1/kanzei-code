//! 独立子代理命令的数据载荷与运行实现。

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DefectReviewResult {
    pub(crate) empty: bool,
    pub(crate) report: String,
    pub(crate) defect_count: usize,
}
