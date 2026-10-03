//! Convert spreadsheet and text attachments into provider-independent text.
use std::io::Cursor;

use anyhow::{bail, Context, Result};
use base64::Engine;
use calamine::{open_workbook_auto_from_rs, Data, Reader};

use crate::PromptAttachment;

const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_TEXT: usize = 160_000;
const MAX_SHEETS: usize = 12;
const MAX_ROWS: u32 = 1_000;
const MAX_COLUMNS: u32 = 80;

pub(crate) fn document_part(attachment: &PromptAttachment) -> Result<kanzei_llm::Part> {
    anyhow::ensure!(
        attachment.data.len() <= MAX_BYTES.div_ceil(3) * 4,
        "附件超过 16 MB：{}",
        attachment.file_name
    );
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&attachment.data)
        .with_context(|| format!("无法读取附件：{}", attachment.file_name))?;
    anyhow::ensure!(
        bytes.len() <= MAX_BYTES,
        "附件超过 16 MB：{}",
        attachment.file_name
    );
    let extension = attachment
        .file_name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let content = match extension.as_str() {
        "xlsx" | "xls" | "xlsm" | "xlsb" | "ods" => {
            spreadsheet_text(&bytes).with_context(|| {
                format!(
                    "无法读取表格 {}，请检查文件是否损坏或加密",
                    attachment.file_name
                )
            })?
        }
        "csv" | "tsv" | "txt" | "md" | "markdown" | "json" | "log" => decode_text(&bytes)?,
        _ => bail!("不支持的附件类型：{}", attachment.file_name),
    };
    let clipped = content.chars().count() > MAX_TEXT;
    let content: String = content.chars().take(MAX_TEXT).collect();
    let name = attachment.file_name.replace(['\n', '\r', '】'], " ");
    Ok(kanzei_llm::Part::Text {
        text: format!(
            "【附件：{name}】\n{content}{}",
            if clipped {
                "\n[内容过长，已截断；以上不是完整文件。]"
            } else {
                ""
            }
        ),
    })
}

fn decode_text(bytes: &[u8]) -> Result<String> {
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        anyhow::ensure!(bytes.len().is_multiple_of(2), "UTF-16 附件不完整");
        let little = bytes[0] == 0xff;
        let chars: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| {
                if little {
                    u16::from_le_bytes([b[0], b[1]])
                } else {
                    u16::from_be_bytes([b[0], b[1]])
                }
            })
            .collect();
        return String::from_utf16(&chars).context("无法读取文字，请另存为 UTF-8 后重试");
    }
    String::from_utf8(
        bytes
            .strip_prefix(&[0xef, 0xbb, 0xbf])
            .unwrap_or(bytes)
            .to_vec(),
    )
    .context("无法读取文字，请另存为 UTF-8 后重试")
}

fn column_label(mut column: u32) -> String {
    let mut label = String::new();
    loop {
        label.insert(0, (b'A' + (column % 26) as u8) as char);
        if column < 26 {
            return label;
        }
        column = column / 26 - 1;
    }
}

fn cell_text(value: Option<&Data>, formula: Option<&String>) -> String {
    let value = match value {
        Some(Data::DateTime(date)) if date.is_duration() => date
            .as_duration()
            .map(|v| v.to_string())
            .unwrap_or_else(|| date.to_string()),
        Some(Data::DateTime(date)) => date
            .as_datetime()
            .map(|v| v.to_string())
            .unwrap_or_else(|| date.to_string()),
        Some(value) => value.to_string(),
        None => String::new(),
    };
    let value = match formula.filter(|f| !f.is_empty()) {
        Some(formula) if value.is_empty() => format!("={formula}"),
        Some(formula) => format!("{value} [={formula}]"),
        None => value,
    };
    let clipped = value.chars().count() > 1_000;
    let mut value: String = value.chars().take(1_000).collect();
    value = value
        .replace('\t', " ")
        .replace('\r', "")
        .replace('\n', " ");
    if clipped {
        value.push_str("…[单元格已截断]");
    }
    value
}

fn spreadsheet_text(bytes: &[u8]) -> Result<String> {
    let mut workbook = open_workbook_auto_from_rs(Cursor::new(bytes))?;
    let sheets = workbook.sheet_names().to_vec();
    anyhow::ensure!(!sheets.is_empty(), "表格没有工作表");
    let mut text = format!(
        "工作簿含 {} 个工作表。数值来自保存时的单元格值；方括号保留公式，未重新计算公式。\n",
        sheets.len()
    );
    let mut remaining = MAX_TEXT;
    for name in sheets.iter().take(MAX_SHEETS) {
        let range = workbook.worksheet_range(name)?;
        let formulas = workbook.worksheet_formula(name)?;
        text.push_str(&format!("\n工作表：{}\n", name.replace(['\n', '\r'], " ")));
        let starts: Vec<_> = [range.start(), formulas.start()]
            .into_iter()
            .flatten()
            .collect();
        let ends: Vec<_> = [range.end(), formulas.end()]
            .into_iter()
            .flatten()
            .collect();
        if starts.is_empty() {
            text.push_str("(空工作表)\n");
            continue;
        }
        let start = (
            starts.iter().map(|p| p.0).min().unwrap(),
            starts.iter().map(|p| p.1).min().unwrap(),
        );
        let end = (
            ends.iter().map(|p| p.0).max().unwrap(),
            ends.iter().map(|p| p.1).max().unwrap(),
        );
        let row_end = end.0.min(start.0.saturating_add(MAX_ROWS - 1));
        let col_end = end.1.min(start.1.saturating_add(MAX_COLUMNS - 1));
        text.push_str(&format!(
            "原始范围：{}{}:{}{}\n行",
            column_label(start.1),
            start.0 + 1,
            column_label(end.1),
            end.0 + 1
        ));
        for col in start.1..=col_end {
            text.push('\t');
            text.push_str(&column_label(col));
        }
        text.push('\n');
        for row in start.0..=row_end {
            let mut line = (row + 1).to_string();
            for col in start.1..=col_end {
                line.push('\t');
                line.push_str(&cell_text(
                    range.get_value((row, col)),
                    formulas.get_value((row, col)),
                ));
            }
            line.push('\n');
            let length = line.chars().count();
            if length > remaining {
                text.push_str("[工作簿内容已达到读取上限，后续单元格及工作表未读。]\n");
                return Ok(text);
            }
            remaining -= length;
            text.push_str(&line);
        }
        if row_end < end.0 || col_end < end.1 {
            text.push_str("[该工作表已截断：最多读取前 1000 行、80 列。]\n");
        }
    }
    if sheets.len() > MAX_SHEETS {
        text.push_str("\n[已截断：只读取前 12 个工作表。]\n");
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn attachment(name: &str, bytes: &[u8]) -> PromptAttachment {
        PromptAttachment {
            file_name: name.into(),
            media_type: "application/octet-stream".into(),
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
        }
    }
    #[test]
    fn spreadsheet_values_sheets_and_formulas_reach_the_model() {
        let part = document_part(&attachment(
            "预算.xlsx",
            include_bytes!("../../../tests/fixtures/attachments/budget.xlsx"),
        ))
        .unwrap();
        let kanzei_llm::Part::Text { text } = part else {
            panic!("Expected parsed text")
        };
        assert!(text.contains("【附件：预算.xlsx】"));
        assert!(text.contains("工作表：预算") && text.contains("工作表：备注"));
        assert!(text.contains("苹果") && text.contains("12.5") && text.contains("[=B2*C2]"));
        assert!(text.contains("原始范围：A1:D3"));
    }
    #[test]
    fn spreadsheet_corruption_fails_instead_of_sending_binary_text() {
        assert!(document_part(&attachment("bad.xlsx", b"not an excel file")).is_err());
        assert!(document_part(&attachment("program.exe", b"MZ")).is_err());
    }
    #[test]
    fn spreadsheet_durations_keep_their_meaning_in_model_text() {
        let kanzei_llm::Part::Text { text } = document_part(&attachment(
            "工时.xlsx",
            include_bytes!("../../../tests/fixtures/attachments/duration.xlsx"),
        ))
        .unwrap() else {
            panic!("Expected parsed text")
        };
        assert!(
            text.contains("2\tPT129600S\t2026-10-04 12:00:00\n"),
            "{text}"
        );
        assert!(text.contains("3\t-PT21600S\t\n"), "{text}");
        assert!(text.contains("4\tPT1.5S\t\n"), "{text}");
        assert!(text.contains("5\tP0D\t\n"), "{text}");
    }
    #[test]
    fn duration_cells_preserve_formula_and_ignore_calendar_epoch() {
        for is_1904 in [false, true] {
            let duration = Data::DateTime(calamine::ExcelDateTime::new(
                1.5,
                calamine::ExcelDateTimeType::TimeDelta,
                is_1904,
            ));
            assert_eq!(
                cell_text(Some(&duration), Some(&"SUM(A1:A2)".into())),
                "PT129600S [=SUM(A1:A2)]"
            );
        }
    }
    #[test]
    fn text_bom_and_length_are_explicit() {
        let utf16: Vec<_> = [0xff, 0xfe]
            .into_iter()
            .chain("商品\t数量".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert_eq!(decode_text(&utf16).unwrap(), "商品\t数量");
        let kanzei_llm::Part::Text { text } = document_part(&attachment(
            "notes.csv",
            "列,值\n".repeat(MAX_TEXT).as_bytes(),
        ))
        .unwrap() else {
            panic!()
        };
        assert!(text.ends_with("[内容过长，已截断；以上不是完整文件。]"));
    }
}
