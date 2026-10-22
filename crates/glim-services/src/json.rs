//! Bounded JSON structure for an on-demand, read-only tree.
use crate::limits as reader_limits;
use anyhow::{Result, ensure};
use serde_json::Value;

pub struct JsonNode {
    pub key: String,
    pub full_key: String,
    pub full_value: Option<String>,
    pub value: String,
    pub kind: &'static str,
    pub children: Vec<usize>,
}
pub fn parse(text: &str) -> Result<Vec<JsonNode>> {
    ensure!(
        text.len() <= reader_limits::JSON_INPUT_BYTES,
        "Tree input exceeds the preview size limit; use Source for larger JSON."
    );
    let value: Value = serde_json::from_str(text)?;
    nodes_for_value(value)
}
pub(crate) fn nodes_for_value(value: Value) -> Result<Vec<JsonNode>> {
    let mut nodes = Vec::new();
    fn add(key: String, value: Value, nodes: &mut Vec<JsonNode>) -> Result<usize> {
        ensure!(
            nodes.len() < reader_limits::JSON_NODES,
            "Tree node count exceeds the preview limit; use Source."
        );
        let index = nodes.len();
        let (kind, label) = match &value {
            Value::Object(v) => {
                let description = v
                    .get("cell_type")
                    .or_else(|| v.get("type"))
                    .and_then(Value::as_str);
                (
                    "object",
                    match description {
                        Some(description) => format!(
                            "{} · {{ {} keys }}",
                            description
                                .chars()
                                .take(reader_limits::JSON_DESCRIPTION_CHARS)
                                .collect::<String>(),
                            v.len()
                        ),
                        None => format!("{{ {} keys }}", v.len()),
                    },
                )
            }
            Value::Array(v) => {
                let label = if !v.is_empty() && v.iter().all(Value::is_string) {
                    let excerpt: String = v
                        .iter()
                        .filter_map(Value::as_str)
                        .flat_map(str::chars)
                        .take(reader_limits::JSON_ARRAY_SUMMARY_CHARS)
                        .collect();
                    format!(
                        "[ {} items ] {}",
                        v.len(),
                        excerpt.replace(['\n', '\r'], " ")
                    )
                } else {
                    format!("[ {} items ]", v.len())
                };
                ("array", label)
            }
            Value::String(v) => (
                "string",
                format!(
                    "{:?}{}",
                    v.chars()
                        .take(reader_limits::JSON_FIELD_CHARS)
                        .collect::<String>(),
                    if v.chars().count() > reader_limits::JSON_FIELD_CHARS {
                        "…"
                    } else {
                        ""
                    }
                ),
            ),
            Value::Number(v) => ("number", v.to_string()),
            Value::Bool(v) => ("boolean", v.to_string()),
            Value::Null => ("null", "null".into()),
        };
        let full_value = match &value {
            Value::Object(_) | Value::Array(_) => None,
            Value::String(text) => Some(text.clone()),
            _ => Some(value.to_string()),
        };
        nodes.push(JsonNode {
            full_key: key.clone(),
            full_value,
            key: key.chars().take(reader_limits::JSON_FIELD_CHARS).collect(),
            value: label,
            kind,
            children: Vec::new(),
        });
        match value {
            Value::Object(values) => {
                for (key, value) in values {
                    let child = add(key, value, nodes)?;
                    nodes[index].children.push(child);
                }
            }
            Value::Array(values) => {
                for (i, value) in values.into_iter().enumerate() {
                    let child = add(format!("[{i}]"), value, nodes)?;
                    nodes[index].children.push(child);
                }
            }
            _ => {}
        }
        Ok(index)
    }
    add("$".into(), value, &mut nodes)?;
    Ok(nodes)
}

/// Strip JSONC comments and trailing commas outside strings without changing offsets.
pub fn parse_jsonc(text: &str) -> Result<Vec<JsonNode>> {
    ensure!(
        text.len() <= reader_limits::JSON_INPUT_BYTES,
        "Tree input exceeds the preview size limit; use Source."
    );
    let mut bytes = text.as_bytes().to_vec();
    let mut i = 0;
    let mut string = false;
    while i < bytes.len() {
        if string {
            if bytes[i] == b'\\' {
                i += 2;
                continue;
            }
            if bytes[i] == b'"' {
                string = false;
            }
            i += 1;
        } else if bytes[i] == b'"' {
            string = true;
            i += 1;
        } else if bytes.get(i..i + 2) == Some(b"//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                bytes[i] = b' ';
                i += 1;
            }
        } else if bytes.get(i..i + 2) == Some(b"/*") {
            bytes[i] = b' ';
            bytes[i + 1] = b' ';
            i += 2;
            while i < bytes.len() && bytes.get(i..i + 2) != Some(b"*/") {
                if !matches!(bytes[i], b'\n' | b'\r') {
                    bytes[i] = b' ';
                }
                i += 1;
            }
            ensure!(i + 1 < bytes.len(), "Unterminated JSONC comment");
            bytes[i] = b' ';
            bytes[i + 1] = b' ';
            i += 2;
        } else {
            i += 1;
        }
    }
    i = 0;
    string = false;
    while i < bytes.len() {
        if string && bytes[i] == b'\\' {
            i += 2;
            continue;
        }
        if bytes[i] == b'"' {
            string = !string;
        }
        if !string && bytes[i] == b',' {
            let mut next = i + 1;
            while next < bytes.len() && bytes[next].is_ascii_whitespace() {
                next += 1;
            }
            if bytes.get(next).is_some_and(|b| matches!(b, b'}' | b']')) {
                bytes[i] = b' ';
            }
        }
        i += 1;
    }
    parse(std::str::from_utf8(&bytes)?)
}

pub fn parse_format(text: &str, format: &str) -> Result<Vec<JsonNode>> {
    ensure!(
        text.len() <= reader_limits::JSON_INPUT_BYTES,
        "Tree input exceeds the preview size limit"
    );
    match format {
        "json5" => nodes_for_value(json5::from_str(text)?),
        "ndjson" | "jsonl" => {
            let values = text
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(serde_json::from_str)
                .collect::<std::result::Result<Vec<Value>, _>>()?;
            nodes_for_value(Value::Array(values))
        }
        "jsonc" => parse_jsonc(text),
        _ => parse(text),
    }
}

/// Whitespace-only formatting preserves key order, duplicate keys and numeric literals.
pub fn format_document(text: &str) -> anyhow::Result<String> {
    let _: serde::de::IgnoredAny = serde_json::from_str(text)?;
    let mut compact = Vec::with_capacity(text.len());
    let (mut quoted, mut escaped) = (false, false);
    for ch in text.chars() {
        if quoted {
            compact.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
        } else if ch == '"' {
            quoted = true;
            compact.push(ch);
        } else if !ch.is_ascii_whitespace() {
            compact.push(ch);
        }
    }
    let mut output = String::with_capacity(text.len());
    let (mut depth, mut quoted, mut escaped) = (0_usize, false, false);
    for (index, ch) in compact.iter().copied().enumerate() {
        if quoted {
            output.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
            continue;
        }
        match ch {
            '"' => {
                quoted = true;
                output.push(ch);
            }
            '{' | '[' => {
                output.push(ch);
                depth += 1;
                if !matches!(compact.get(index + 1), Some('}' | ']')) {
                    output.push('\n');
                    output.push_str(&"  ".repeat(depth));
                }
            }
            '}' | ']' => {
                depth = depth.saturating_sub(1);
                if index > 0 && !matches!(compact[index - 1], '{' | '[') {
                    output.push('\n');
                    output.push_str(&"  ".repeat(depth));
                }
                output.push(ch);
            }
            ',' => {
                output.push_str(",\n");
                output.push_str(&"  ".repeat(depth));
            }
            ':' => output.push_str(": "),
            _ => output.push(ch),
        }
    }
    output.push('\n');
    Ok(output)
}

#[cfg(test)]
mod jsonc_tests {
    #[test]
    fn comments_do_not_change_strings_and_trailing_commas_are_supported() {
        let nodes = super::parse_jsonc(
            r#"{
            // line comment
            "url": "https://example.invalid/a//b", /* block */
            "literal": "/* text */ \"quoted\"",
            "items": [1, 2,],
        }"#,
        )
        .unwrap();
        assert!(
            nodes
                .iter()
                .any(|node| node.key == "url" && node.value.contains("a//b"))
        );
        assert!(
            nodes
                .iter()
                .any(|node| node.key == "literal" && node.value.contains("/* text */"))
        );
        assert!(super::parse_jsonc("{ /* unfinished").is_err());
        assert!(super::parse("{\"a\":1,}").is_err());
    }
}
