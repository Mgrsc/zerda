use anyhow::Result;
use regex::Regex;

#[derive(Debug, Clone)]
pub struct PtcRequest {
    pub purpose: String,
    pub kind: PtcRequestKind,
}

#[derive(Debug, Clone)]
pub enum PtcRequestKind {
    Python { python: String },
}

pub fn parse_ptc_requests(raw: &str) -> Result<Vec<PtcRequest>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let block_re = Regex::new(r"(?s)<(PTC_TOOL_CALLING)\b([^>]*)>(.*?)</(PTC_TOOL_CALLING)>")
        .expect("valid regex");
    let mut spans = Vec::new();

    for captures in block_re.captures_iter(trimmed) {
        let Some(full_match) = captures.get(0) else {
            continue;
        };
        let Some(open_tag) = captures.get(1).map(|m| m.as_str()) else {
            continue;
        };
        let Some(attrs) = captures.get(2).map(|m| m.as_str()) else {
            continue;
        };
        let Some(body) = captures.get(3).map(|m| m.as_str()) else {
            continue;
        };
        let Some(close_tag) = captures.get(4).map(|m| m.as_str()) else {
            continue;
        };
        if open_tag != close_tag {
            anyhow::bail!("Mismatched PTC protocol tags");
        }
        let kind = match open_tag {
            "PTC_TOOL_CALLING" => {
                if body.contains("<PYTHON>") || body.contains("</PYTHON>") {
                    anyhow::bail!("Nested <PYTHON> is not allowed inside <PTC_TOOL_CALLING>");
                }
                let python = normalize_ptc_body(body);
                if python.trim().is_empty() {
                    anyhow::bail!("Empty body inside <PTC_TOOL_CALLING>");
                }
                PtcRequestKind::Python { python }
            }
            _ => anyhow::bail!("Unsupported PTC block type"),
        };
        let purpose = extract_purpose(open_tag, attrs, body);
        spans.push((
            full_match.start(),
            full_match.end(),
            PtcRequest { purpose, kind },
        ));
    }

    spans.sort_by_key(|(start, _, _)| *start);
    let mut requests = Vec::new();
    let mut cursor = 0usize;
    for (start, end, request) in spans {
        if !trimmed[cursor..start].trim().is_empty() {
            anyhow::bail!(
                "Unexpected non-PTC content remained after parsing captured protocol payload"
            );
        }
        requests.push(request);
        cursor = end;
    }

    if requests.is_empty() {
        anyhow::bail!("No valid PTC blocks found in captured protocol payload");
    }

    if !trimmed[cursor..].trim().is_empty() {
        anyhow::bail!(
            "Unexpected non-PTC content remained after parsing captured protocol payload"
        );
    }

    Ok(requests)
}

fn extract_purpose(open_tag: &str, attrs: &str, _body: &str) -> String {
    if open_tag != "PTC_TOOL_CALLING" {
        return String::new();
    }
    extract_attr(attrs, "purpose")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
}

fn extract_attr(attrs: &str, attr_name: &str) -> Option<String> {
    let pattern = format!(
        r#"(?s)\b{attr_name}\s*=\s*(?:"([^"]*)"|'([^']*)')"#,
        attr_name = regex::escape(attr_name)
    );
    let re = Regex::new(&pattern).ok()?;
    let captures = re.captures(attrs)?;
    captures
        .get(1)
        .or_else(|| captures.get(2))
        .map(|m| m.as_str().to_string())
}

fn normalize_ptc_body(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(inner) = trimmed
        .strip_prefix("<![CDATA[")
        .and_then(|value| value.strip_suffix("]]>"))
    {
        inner.trim().to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_ptc_requests, PtcRequestKind};

    #[test]
    fn parses_ptc_tool_calling_body_code() {
        let raw = r#"
<PTC_TOOL_CALLING purpose="read one file"><![CDATA[
result = await fs_read(path="README.md")
]]></PTC_TOOL_CALLING>
        "#;
        let requests = parse_ptc_requests(raw).expect("requests");
        assert_eq!(requests.len(), 1);
        let PtcRequestKind::Python { python } = &requests[0].kind;
        assert_eq!(requests[0].purpose, "read one file");
        assert_eq!(python, "result = await fs_read(path=\"README.md\")");
    }

    #[test]
    fn rejects_provider_style_tool_call_wrapper() {
        let raw = r#"
<tool_call><function=PTC_TOOL_CALLING><parameter=python>result = await fs_read(path="README.md")</parameter></function></tool_call>
        "#;
        assert!(parse_ptc_requests(raw).is_err());
    }

    #[test]
    fn rejects_rust_call_block() {
        let raw = r#"
<UNKNOWN_CALL name="legacy_discovery">
  <NAME>firecrawl_search_web</NAME>
</UNKNOWN_CALL>
        "#;
        assert!(parse_ptc_requests(raw).is_err());
    }

    #[test]
    fn rejects_nested_python_block_inside_ptc_tool_calling() {
        let raw = r#"
<PTC_TOOL_CALLING><PYTHON><![CDATA[
result = await fs_read(path="README.md")
]]></PYTHON></PTC_TOOL_CALLING>
        "#;
        assert!(parse_ptc_requests(raw).is_err());
    }
}
