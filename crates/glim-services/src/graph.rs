//! Local structured-data graph projection. No network, embedded scripts or source writes.
use crate::{
    json::JsonNode,
    limits,
    work::Cancellation,
    xmind::{MindMap, Sheet, Topic},
};
use anyhow::{Result, ensure};
use serde::Deserialize;
use std::{io::Read, path::Path};

pub fn supports(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "json" | "jsonc" | "json5" | "jsonl" | "ndjson" | "yaml" | "yml" | "csv" | "tsv" | "xml"
    )
}
pub fn read(path: &Path, text: Option<String>, cancel: &Cancellation) -> Result<MindMap> {
    cancel.check()?;
    let text = match text {
        Some(text) => text,
        None => {
            let input = crate::work::Reader {
                inner: std::fs::File::open(path)?,
                cancel: cancel.clone(),
            };
            let mut text = String::new();
            input
                .take(limits::JSON_INPUT_BYTES as u64 + 1)
                .read_to_string(&mut text)?;
            text
        }
    };
    ensure!(
        text.len() <= limits::JSON_INPUT_BYTES,
        "Graph input exceeds the preview budget; use Source."
    );
    let format = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let nodes = match format.as_str() {
        "yaml" | "yml" => {
            let mut documents = Vec::new();
            for document in serde_yaml_ng::Deserializer::from_str(text) {
                cancel.check()?;
                let value = serde_yaml_ng::Value::deserialize(document)?;
                documents.push(serde_json::to_value(value)?);
            }
            let value = if documents.len() == 1 {
                documents.remove(0)
            } else {
                serde_json::Value::Array(documents)
            };
            crate::json::nodes_for_value(value)?
        }
        "csv" | "tsv" => {
            // Keep every column and duplicate header by representing records as arrays.
            let mut reader = csv::ReaderBuilder::new()
                .has_headers(false)
                .delimiter(if format == "tsv" { b'\t' } else { b',' })
                .from_reader(text.as_bytes());
            let mut rows = Vec::new();
            let mut count = 1usize;
            for record in reader.records() {
                cancel.check()?;
                let record = record?;
                count = count.saturating_add(record.len() + 1);
                ensure!(
                    count <= limits::XMIND_TOPICS,
                    "Graph node budget exceeded; use Source or Table."
                );
                rows.push(serde_json::Value::Array(
                    record
                        .iter()
                        .map(|v| serde_json::Value::String(v.into()))
                        .collect(),
                ));
            }
            crate::json::nodes_for_value(serde_json::Value::Array(rows))?
        }
        "xml" => xml_nodes(text, cancel)?,
        _ => crate::json::parse_format(text, &format)?,
    };
    cancel.check()?;
    ensure!(
        nodes.len() <= limits::XMIND_TOPICS,
        "Graph node budget exceeded; use Source."
    );
    let mut pending = vec![(0usize, 0usize)];
    while let Some((index, depth)) = pending.pop() {
        cancel.check()?;
        ensure!(
            depth <= limits::XMIND_NESTING,
            "Graph nesting budget exceeded; use Source."
        );
        pending.extend(
            nodes[index]
                .children
                .iter()
                .map(|&child| (child, depth + 1)),
        );
    }
    // Keep scalar fields together in object cards; only containers branch out.
    let topics = nodes
        .iter()
        .map(|node| {
            let grouped = node.kind == "object";
            let fields: Vec<_> = node
                .children
                .iter()
                .copied()
                .filter(|&i| nodes[i].children.is_empty() && nodes[i].full_value.is_some())
                .collect();
            let mut title = format!("{}\n{}", node.key, node.value);
            let mut notes = format!(
                "{}\n{}",
                node.full_key,
                node.full_value.as_deref().unwrap_or(&node.value)
            );
            if grouped {
                for (position, &child) in fields.iter().enumerate() {
                    let field = &nodes[child];
                    if position < 2 {
                        title.push_str(&format!("\n{}: {}", field.key, field.value));
                    }
                    notes.push_str(&format!(
                        "\n{}: {}",
                        field.full_key,
                        field.full_value.as_deref().unwrap_or(&field.value)
                    ));
                }
            }
            Topic {
                title,
                notes,
                labels: vec![node.kind.into()],
                link: None,
                children: node
                    .children
                    .iter()
                    .copied()
                    .filter(|&i| {
                        !grouped || !nodes[i].children.is_empty() || nodes[i].full_value.is_none()
                    })
                    .collect(),
            }
        })
        .collect();
    Ok(MindMap {
        path: path.to_owned(),
        fingerprint: String::new(),
        sheets: vec![Sheet {
            title: "Graph".into(),
            topics,
            roots: vec![0],
            relationships: Vec::new(),
        }],
    })
}
fn xml_nodes(text: &str, cancel: &Cancellation) -> Result<Vec<JsonNode>> {
    let document = roxmltree::Document::parse(text)?;
    let mut nodes: Vec<JsonNode> = Vec::new();
    let mut pending = vec![(document.root_element(), None::<usize>, 0usize)];
    while let Some((element, parent, depth)) = pending.pop() {
        cancel.check()?;
        ensure!(
            depth <= limits::XMIND_NESTING,
            "Graph nesting budget exceeded; use Source."
        );
        ensure!(
            nodes.len() < limits::XMIND_TOPICS,
            "Graph node budget exceeded; use Source."
        );
        let index = nodes.len();
        let name = element.tag_name().name().to_owned();
        nodes.push(JsonNode {
            key: name.clone(),
            full_key: name,
            full_value: None,
            value: "element".into(),
            kind: "element",
            children: Vec::new(),
        });
        if let Some(parent) = parent {
            nodes[parent].children.push(index);
        }
        for (key, value) in element
            .attributes()
            .map(|a| (format!("@{}", a.name()), a.value()))
            .chain(
                element
                    .children()
                    .filter(|n| n.is_text())
                    .filter_map(|n| n.text())
                    .filter(|t| !t.trim().is_empty())
                    .map(|t| ("#text".into(), t)),
            )
        {
            ensure!(
                nodes.len() < limits::XMIND_TOPICS,
                "Graph node budget exceeded; use Source."
            );
            let child = nodes.len();
            nodes.push(JsonNode {
                key: key.clone(),
                full_key: key,
                full_value: Some(value.into()),
                value: value.chars().take(limits::JSON_FIELD_CHARS).collect(),
                kind: "string",
                children: Vec::new(),
            });
            nodes[index].children.push(child);
        }
        pending.extend(
            element
                .children()
                .rev()
                .filter(|n| n.is_element())
                .map(|n| (n, Some(index), depth + 1)),
        );
    }
    Ok(nodes)
}
