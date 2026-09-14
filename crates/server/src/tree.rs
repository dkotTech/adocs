use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::content::DocMeta;

/// A directory tree node. Directories are not stored anywhere, they are derived from document paths.
#[derive(Serialize, Debug, Clone)]
pub struct TreeNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
    pub children: Vec<TreeNode>,
}

impl TreeNode {
    fn dir(name: &str, path: &str) -> Self {
        TreeNode {
            name: name.to_string(),
            path: path.to_string(),
            is_dir: true,
            title: None,
            summary: None,
            content_type: None,
            size: None,
            updated_at: None,
            children: Vec::new(),
        }
    }

    fn doc(name: &str, meta: &DocMeta) -> Self {
        TreeNode {
            name: name.to_string(),
            path: meta.path.clone(),
            is_dir: false,
            title: Some(meta.title.clone()),
            summary: meta.summary.clone(),
            content_type: Some(meta.content_type.clone()),
            size: Some(meta.size),
            updated_at: meta.updated_at,
            children: Vec::new(),
        }
    }
}

pub fn build(docs: &[DocMeta]) -> TreeNode {
    let mut root = TreeNode::dir("", "");
    for meta in docs {
        insert(&mut root, meta);
    }
    sort(&mut root);
    root
}

fn insert(root: &mut TreeNode, meta: &DocMeta) {
    let parts: Vec<&str> = meta.path.split('/').collect();
    let mut node = root;
    for (i, part) in parts.iter().enumerate() {
        if i + 1 == parts.len() {
            node.children.push(TreeNode::doc(part, meta));
            return;
        }
        let dir_path = parts[..=i].join("/");
        let idx = match node
            .children
            .iter()
            .position(|c| c.is_dir && c.name == *part)
        {
            Some(idx) => idx,
            None => {
                node.children.push(TreeNode::dir(part, &dir_path));
                node.children.len() - 1
            }
        };
        node = &mut node.children[idx];
    }
}

/// Directories first, then documents; within a group, by name, case-insensitively.
fn sort(node: &mut TreeNode) {
    node.children.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    for child in &mut node.children {
        sort(child);
    }
}
