//! Shared existence and ownership checks; semantic coverage still requires review.
pub(crate) fn validate_reference(
    root: &std::path::Path,
    item_id: &str,
    reference: &str,
) -> Result<(), String> {
    if reference.trim().is_empty() {
        return Err("验收证据引用不能为空".into());
    }
    if reference.starts_with("verification:") {
        return crate::verification::validate_evidence_reference(root, item_id, reference)
            .map_err(|error| format!("后台验证证据无效: {error:#}"));
    }
    if reference.starts_with("T-") {
        let records = crate::test_record::records_for_entry(root, item_id);
        if !records
            .iter()
            .any(|record| record["id"] == reference && record["status"] == "passed")
        {
            return Err(format!(
                "测试证据 {reference} 不存在、未通过或未关联 {item_id}"
            ));
        }
        return Ok(());
    }
    let path = reference
        .rsplit_once(':')
        .filter(|(_, line)| line.parse::<usize>().is_ok())
        .map(|(path, _)| path)
        .unwrap_or(reference);
    if !root.join(path).is_file() {
        return Err(format!(
            "证据文件不存在: {reference}；请引用可读取的文件或本条目已通过的 T- 测试记录"
        ));
    }
    Ok(())
}
