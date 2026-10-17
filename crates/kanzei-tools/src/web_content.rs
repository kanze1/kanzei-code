//! Readability and bounded previews for saved web evidence.

use std::sync::OnceLock;

pub(crate) fn content_issue(html: &str) -> Option<&'static str> {
    static NON_CONTENT: OnceLock<regex::Regex> = OnceLock::new();
    let non_content = NON_CONTENT.get_or_init(|| regex::Regex::new(
        r"(?is)<head\b[^>]*>.*?</head\s*>|<title\b[^>]*>.*?</title\s*>|<noscript\b[^>]*>.*?</noscript\s*>|<h[1-6]\b[^>]*>.*?</h[1-6]\s*>"
    ).expect("static non-content blocks"));
    let body = non_content.replace_all(html, "");
    let text = crate::webfetch::html_to_text(&body);
    static INTERSTITIAL: OnceLock<regex::Regex> = OnceLock::new();
    let interstitial = INTERSTITIAL.get_or_init(|| regex::Regex::new(r"(?is)^\s*(?:please\s+)?(?:you\s+(?:need|have)\s+to\s+)?enable\s+javascript(?:\s+and\s+cookies)?(?:\s+to\s+(?:continue|run\s+this\s+app))?[.!]?\s*$").expect("static JavaScript interstitial"));
    if text.trim().is_empty() || interstitial.is_match(&text) {
        Some("HTML contains no readable body outside its title, scripts and noscript fallback. Open this URL with the browser tool or use an original text/API source; this response is not article evidence.")
    } else {
        None
    }
}

pub(crate) struct Preview {
    pub(crate) text: String,
    pub(crate) truncated: bool,
    pub(crate) next_line: Option<usize>,
    pub(crate) last_line: Option<usize>,
}

pub(crate) fn preview(text: &str, cap: usize) -> Preview {
    let mut rendered = String::new();
    let mut used = 0;
    let mut last_line = None;
    let mut next_line = None;
    let mut truncated = false;
    for line in text.lines() {
        let line_no = line
            .split_once(" | ")
            .and_then(|(number, _)| number.parse::<usize>().ok());
        let cost = line.chars().count() + usize::from(!rendered.is_empty());
        if used + cost > cap {
            truncated = true;
            if rendered.is_empty() {
                rendered.extend(line.chars().take(cap));
                last_line = line_no;
            }
            next_line = line_no;
            break;
        }
        if !rendered.is_empty() {
            rendered.push('\n');
        }
        rendered.push_str(line);
        used += cost;
        if line_no.is_some() {
            last_line = line_no;
        }
    }
    Preview {
        text: rendered,
        truncated,
        next_line,
        last_line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_shells_are_not_articles_and_short_real_text_is_valid() {
        for html in [
            "<html><head><title>NVD - Home</title></head><body><div id='app'></div><script>load()</script></body></html>",
            "<title>CVE</title><noscript>Please enable JavaScript to continue</noscript><script>boot()</script>",
            "<html><body></body></html>",
            "<h1>NVD Home</h1>",
            "<body><p>You need to enable JavaScript to run this app.</p><script>boot()</script></body>",
        ] { assert!(content_issue(html).is_some(), "{html}"); }
        assert!(content_issue(
            "<title>Small</title><body><p>A short but real advisory.</p></body>"
        )
        .is_none());
        assert!(content_issue(
            "<p>Technical article mentions JavaScript and its security implications.</p>"
        )
        .is_none());
    }

    #[test]
    fn pagination_retains_whole_lines_and_points_to_the_first_unread_line() {
        let first = preview("0001 | alpha\n0002 | beta\n0003 | gamma", 25);
        assert_eq!(first.text, "0001 | alpha\n0002 | beta");
        assert_eq!(first.last_line, Some(2));
        assert_eq!(first.next_line, Some(3));
        assert!(first.truncated);
        let long = preview("0001 | 超长中文正文", 10);
        assert_eq!(long.text.chars().count(), 10);
        assert_eq!(long.next_line, Some(1));
    }
}
