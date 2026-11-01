//! QQ's ordinary text messages have no Markdown renderer.
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

pub(super) const STYLE: &str = "QQ 普通聊天不渲染 Markdown。面向对方的正文使用自然纯文本和换行，不使用 Markdown 标题、粗体、代码围栏、表格或链接语法；网址直接写出，代码保留原始内容和缩进。需要复杂排版时在 /workspace 制作图片或文档，再通过 qq_reply 的附件发送。";

pub(super) fn render(input: &str) -> String {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut output = String::new();
    let mut lists: Vec<Option<u64>> = Vec::new();
    let mut links = Vec::new();
    let mut table_cell = 0;
    for event in Parser::new_ext(input, options) {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { .. } | Tag::CodeBlock(_) | Tag::Table(_) => {
                    line_break(&mut output, 2);
                }
                Tag::BlockQuote(_) => {
                    line_break(&mut output, 2);
                    output.push_str("引用：\n");
                }
                Tag::List(start) => {
                    line_break(&mut output, 1);
                    lists.push(start);
                }
                Tag::Item => {
                    line_break(&mut output, 1);
                    output.push_str(&"  ".repeat(lists.len().saturating_sub(1)));
                    match lists.last_mut() {
                        Some(Some(number)) => {
                            output.push_str(&format!("{number}. "));
                            *number = number.saturating_add(1);
                        }
                        _ => output.push_str("• "),
                    }
                }
                Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
                    links.push((dest_url, output.len()));
                }
                Tag::TableHead | Tag::TableRow => {
                    line_break(&mut output, 1);
                    table_cell = 0;
                }
                Tag::TableCell => {
                    if table_cell > 0 {
                        output.push('　');
                    }
                    table_cell += 1;
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph if lists.is_empty() => line_break(&mut output, 2),
                TagEnd::Heading(_) | TagEnd::CodeBlock | TagEnd::BlockQuote(_) | TagEnd::Table => {
                    line_break(&mut output, 2);
                }
                TagEnd::List(_) => {
                    lists.pop();
                    line_break(&mut output, if lists.is_empty() { 2 } else { 1 });
                }
                TagEnd::Item | TagEnd::TableHead | TagEnd::TableRow => {
                    line_break(&mut output, 1);
                }
                TagEnd::Link | TagEnd::Image => {
                    if let Some((url, start)) = links.pop() {
                        if !url.is_empty() && output[start..] != *url {
                            output.push_str(&format!(" ({url})"));
                        }
                    }
                }
                _ => {}
            },
            Event::Text(text)
            | Event::Code(text)
            | Event::Html(text)
            | Event::InlineHtml(text)
            | Event::InlineMath(text)
            | Event::DisplayMath(text) => output.push_str(&text),
            Event::SoftBreak | Event::HardBreak => output.push('\n'),
            Event::Rule => line_break(&mut output, 2),
            Event::TaskListMarker(checked) => {
                output.push_str(if checked { "☑ " } else { "☐ " })
            }
            Event::FootnoteReference(label) => output.push_str(&format!("[{label}]")),
        }
    }
    output.trim_matches('\n').to_owned()
}

fn line_break(output: &mut String, count: usize) {
    if !output.is_empty() {
        let existing = output.chars().rev().take_while(|c| *c == '\n').count();
        output.extend(std::iter::repeat_n('\n', count.saturating_sub(existing)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_becomes_readable_qq_text_without_losing_links_or_code() {
        let text = render("## 结果\n\n**成功**，见[报告](https://example.com/a_b?q=1&x=2)。\n\n```python\nif x < 2:\n    print('a_b **literal**')\n```\n\n普通文字，2 * 3 = 6，文件 report_v1.pdf。[CQ:face,id=14]");
        assert_eq!(text, "结果\n\n成功，见报告 (https://example.com/a_b?q=1&x=2)。\n\nif x < 2:\n    print('a_b **literal**')\n\n普通文字，2 * 3 = 6，文件 report_v1.pdf。[CQ:face,id=14]");
    }

    #[test]
    fn lists_tables_quotes_and_reference_links_keep_their_content() {
        let text = render("3. 第一项\n4. 第二项\n\n| 项目 | 状态 |\n| --- | --- |\n| A | **成功** |\n\n> 引用内容\n\n[资料][ref]\n\n[ref]: https://example.com\n");
        assert!(text.contains("3. 第一项\n4. 第二项"), "{text}");
        assert!(text.contains("项目　状态\nA　成功"), "{text}");
        assert!(text.contains("引用：\n引用内容"), "{text}");
        assert!(text.contains("资料 (https://example.com)"), "{text}");
        assert!(!text.contains("---"));
    }

    #[test]
    fn ordinary_text_escaped_symbols_and_urls_are_not_stripped() {
        for text in [
            "你好\n第二行😊",
            "https://example.com/a_b?q=1&x=2",
            "C:\\Users\\name\\file.txt",
            "2 * 3 = 6，foo_bar_baz，#123",
        ] {
            assert_eq!(render(text), text);
        }
        assert_eq!(render(r"\*保留星号\* 和 \# 号"), "*保留星号* 和 # 号");
    }
}
