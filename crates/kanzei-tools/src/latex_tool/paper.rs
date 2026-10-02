//! Paper compilation shared by section writing, AUTO and the desktop editor.
use std::path::Path;

pub fn compile_paper(workdir: &Path, tex_name: &str) -> (bool, String) {
    let tex = workdir.join(tex_name);
    let lock = tex.with_extension("compile");
    let _lock = match kanzei_base::atomic_file::lock_exclusive(&lock) {
        Ok(lock) => lock,
        Err(error) => return (false, format!("锁定论文编译失败: {error}")),
    };
    let pdf = tex.with_extension("pdf");
    let log = tex.with_extension("log");
    // A failed invocation must never reuse a PDF or log from an earlier attempt.
    for file in [&pdf, &log] {
        if file.is_file() {
            if let Err(error) = std::fs::remove_file(file) {
                return (false, format!("清除旧编译工件失败: {error}"));
            }
        }
    }
    let (ok, mut diagnostics) = super::compile_latex(workdir, tex_name);
    let bytes = std::fs::read(pdf).unwrap_or_default();
    let log = std::fs::read_to_string(log).unwrap_or_default();
    let valid = bytes.starts_with(b"%PDF-") && bytes.len() > 500;
    let references = !log.contains("There were undefined references")
        && !(log.contains("Citation") && log.contains("undefined"));
    if ok && (!valid || !references) {
        diagnostics.push_str(
            "\n论文产物检查失败：需要本次有效 PDF，且引用须全部解析。不能用旧 PDF 作为成功证据。",
        );
    }
    (ok && valid && references, diagnostics)
}

#[cfg(test)]
mod tests {
    #[test]
    fn missing_source_cannot_return_a_stale_pdf_as_success() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("kz-stale-pdf-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.as_path().join("paper.pdf"),
            [b"%PDF-".as_slice(), &vec![b'x'; 600]].concat(),
        )
        .unwrap();
        std::fs::write(dir.as_path().join("paper.log"), "old success").unwrap();
        let (ok, diagnostics) = super::compile_paper(dir.as_path(), "paper.tex");
        assert!(!ok);
        assert!(diagnostics.contains("找不到 .tex"));
        assert!(!dir.as_path().join("paper.pdf").exists());
        assert!(!dir.as_path().join("paper.log").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
