//! Local, read-only OPML outlines; subscription URLs are never fetched.
use crate::{limits, work::Cancellation};
use anyhow::{Result, ensure};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

pub struct Outline {
    pub title: String,
    pub description: String,
    pub feed: Option<String>,
    pub website: Option<String>,
    pub children: Vec<usize>,
}
pub struct Document {
    pub path: PathBuf,
    pub fingerprint: String,
    pub title: String,
    pub nodes: Vec<Outline>,
    pub roots: Vec<usize>,
}
pub fn supports(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("opml"))
}
fn web_url(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    let url = url::Url::parse(value).ok()?;
    matches!(url.scheme(), "http" | "https").then(|| value.to_string())
}
pub fn read(path: &Path, cancel: &Cancellation) -> Result<Document> {
    let mut bytes = Vec::new();
    crate::work::Reader {
        inner: std::fs::File::open(path)?,
        cancel: cancel.clone(),
    }
    .take(limits::OPML_XML_BYTES as u64 + 1)
    .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= limits::OPML_XML_BYTES,
        "OPML exceeds preview XML budget"
    );
    let text = std::str::from_utf8(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes))?;
    let document = roxmltree::Document::parse(text)?;
    let root = document.root_element();
    ensure!(root.has_tag_name("opml"), "Not an OPML document");
    let title = root
        .children()
        .find(|n| n.has_tag_name("head"))
        .and_then(|head| head.children().find(|n| n.has_tag_name("title")))
        .and_then(|n| n.text())
        .unwrap_or("")
        .to_string();
    let body = root
        .children()
        .find(|n| n.has_tag_name("body"))
        .ok_or_else(|| anyhow::anyhow!("OPML body is missing"))?;
    let mut nodes: Vec<Outline> = Vec::new();
    let mut roots = Vec::new();
    let mut stack = body
        .children()
        .filter(|n| n.has_tag_name("outline"))
        .rev()
        .map(|n| (n, None::<usize>, 0_usize))
        .collect::<Vec<_>>();
    while let Some((node, parent, depth)) = stack.pop() {
        cancel.check()?;
        ensure!(
            depth < limits::OPML_NESTING && nodes.len() < limits::OPML_NODES,
            "OPML exceeds outline preview budget"
        );
        let index = nodes.len();
        let feed = web_url(node.attribute("xmlUrl"));
        let website = web_url(node.attribute("htmlUrl").or_else(|| node.attribute("url")));
        let title = node
            .attribute("text")
            .filter(|s| !s.trim().is_empty())
            .or_else(|| node.attribute("title"))
            .unwrap_or("")
            .to_string();
        nodes.push(Outline {
            title,
            description: node.attribute("description").unwrap_or("").to_string(),
            feed,
            website,
            children: Vec::new(),
        });
        if let Some(parent) = parent {
            nodes[parent].children.push(index);
        } else {
            roots.push(index);
        }
        stack.extend(
            node.children()
                .filter(|n| n.has_tag_name("outline"))
                .rev()
                .map(|n| (n, Some(index), depth + 1)),
        );
    }
    let stamp = std::fs::metadata(path)?;
    Ok(Document {
        path: path.to_path_buf(),
        fingerprint: format!("opml:{}:{:?}", stamp.len(), stamp.modified().ok()),
        title,
        nodes,
        roots,
    })
}
