use std::sync::LazyLock;

use regex::Regex;

static RICH_CONTENT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"<(image)>(https?://[^<]+)</image>|<(voice)>(/[^<]+)</voice>")
        .expect("invalid regex")
});

pub fn has_rich_markers(text: &str) -> bool {
    RICH_CONTENT_RE.is_match(text)
}

pub fn strip_rich_markers(text: &str) -> String {
    RICH_CONTENT_RE.replace_all(text, "").trim().to_string()
}

pub enum RichSegment {
    Text(String),
    Image(String),
    Voice(String),
}

pub fn extract_rich_segments(message: &str) -> Vec<RichSegment> {
    let mut segments = Vec::new();
    let mut last_end = 0;

    for caps in RICH_CONTENT_RE.captures_iter(message) {
        let full = caps.get(0).unwrap();
        let before = message[last_end..full.start()].trim();
        if !before.is_empty() {
            segments.push(RichSegment::Text(before.to_string()));
        }

        if let Some(url) = caps.get(2) {
            segments.push(RichSegment::Image(url.as_str().trim().to_string()));
        } else if let Some(path) = caps.get(4) {
            segments.push(RichSegment::Voice(path.as_str().trim().to_string()));
        }

        last_end = full.end();
    }

    let trailing = message[last_end..].trim();
    if !trailing.is_empty() {
        segments.push(RichSegment::Text(trailing.to_string()));
    }

    segments
}

pub fn rich_content_markers(response: &str) -> Vec<String> {
    RICH_CONTENT_RE
        .find_iter(response)
        .map(|m| m.as_str().to_string())
        .collect()
}
