//! Obsidian-like node mindmap backing the bookmark manager.
//!
//! Each node is a Markdown file in a "vault" directory. A node carries YAML
//! frontmatter (title, url, tags, links) plus a Markdown body. The links are
//! `[[wikilink]]`-style references to other nodes, which form the edges of the
//! mindmap. This mirrors how Obsidian stores a note graph, so the vault is
//! inspectable/editable outside serverbrowser and the future minimap can just
//! read node+edge data.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A single node (bookmark) in the mindmap.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Node {
    /// Node id, used as the Markdown filename (slugified title by default).
    pub id: String,
    /// Display title.
    pub title: String,
    /// The bookmark URL (empty for pure "folder"/note nodes).
    pub url: Option<String>,
    /// Tags (e.g. `#rust`, `#docs`).
    pub tags: Vec<String>,
    /// Outgoing edges to other node ids (`[[wikilinks]]`).
    pub links: Vec<String>,
    /// Optional free-form body (Markdown).
    pub body: String,
    /// Creation + modification timestamps (unix seconds).
    pub created: u64,
    pub updated: u64,
}

impl Node {
    pub fn new(title: &str, url: Option<String>) -> Self {
        let id = slugify(title);
        let now = now_secs();
        Node {
            id,
            title: title.to_string(),
            url,
            tags: vec![],
            links: vec![],
            body: String::new(),
            created: now,
            updated: now,
        }
    }

    /// Link this node to another, bidirectionally by default (undirected
    /// mindmap edges).
    pub fn link(&mut self, other_id: &str) {
        if !self.links.iter().any(|l| l == other_id) {
            self.links.push(other_id.to_string());
        }
        self.updated = now_secs();
    }

    pub fn unlink(&mut self, other_id: &str) {
        self.links.retain(|l| l != other_id);
        self.updated = now_secs();
    }

    fn to_markdown(&self) -> String {
        let mut s = String::new();
        s.push_str("---\n");
        s.push_str(&format!("title: \"{}\"\n", yaml_escape(&self.title)));
        if let Some(url) = &self.url {
            s.push_str(&format!("url: \"{}\"\n", yaml_escape(url)));
        }
        if !self.tags.is_empty() {
            s.push_str(&format!("tags: {}\n", self.tags.join(", ")));
        }
        s.push_str("---\n\n");
        if !self.body.is_empty() {
            s.push_str(&self.body);
            s.push_str("\n\n");
        }
        if !self.links.is_empty() {
            for l in &self.links {
                s.push_str(&format!("- [[{}]]\n", l));
            }
        }
        s
    }

    fn from_markdown(id: &str, text: &str, created: u64, updated: u64) -> Node {
        let mut title = id.to_string();
        let mut url = None;
        let mut tags = vec![];
        let mut links = vec![];
        let mut body = String::new();

        let mut in_frontmatter = false;
        let mut in_body_links = false;
        // Minimal YAML frontmatter parser (good enough for our own keys).
        for line in text.lines() {
            let t = line.trim();
            if t == "---" {
                if !in_frontmatter && body.is_empty() && url.is_none() && title == id {
                    in_frontmatter = true;
                } else if in_frontmatter {
                    in_frontmatter = false;
                    in_body_links = true;
                }
                continue;
            }
            if in_frontmatter {
                if let Some(rest) = t.strip_prefix("title:") {
                    title = unquote(rest.trim()).unwrap_or_else(|| id.to_string());
                } else if let Some(rest) = t.strip_prefix("url:") {
                    url = unquote(rest.trim()).map(|s| s.to_string());
                } else if let Some(rest) = t.strip_prefix("tags:") {
                    tags = rest
                        .trim()
                        .split(',')
                        .filter_map(|s| unquote(s.trim()))
                        .map(|s| s.to_string())
                        .collect();
                }
            } else if in_body_links || !in_frontmatter {
                // `[[wikilink]]` lines are edges.
                if let Some(link) = parse_wikilink_line(t) {
                    links.push(link);
                } else if in_body_links {
                    body.push_str(line);
                    body.push('\n');
                }
            }
        }
        let body = body.trim().to_string();

        Node {
            id: id.to_string(),
            title,
            url,
            tags,
            links,
            body,
            created,
            updated,
        }
    }
}

/// The whole graph: nodes keyed by id, plus persistence to a vault directory.
#[derive(Debug, Clone, Default)]
pub struct Mindmap {
    pub nodes: BTreeMap<String, Node>,
    root: PathBuf,
}

impl Mindmap {
    /// Open (or lazily create) a vault at `root`.
    pub fn open(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        std::fs::create_dir_all(&root).ok();
        let mut m = Mindmap {
            nodes: BTreeMap::new(),
            root,
        };
        m.reload();
        m
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Reload all Markdown nodes from disk.
    pub fn reload(&mut self) {
        self.nodes.clear();
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Ok(text) = std::fs::read_to_string(&p) else {
                continue;
            };
            // Use file mtime for the updated stamp; created falls back to it.
            let updated = mtime(&p).unwrap_or_else(now_secs);
            let created = updated;
            let node = Node::from_markdown(stem, &text, created, updated);
            self.nodes.insert(stem.to_string(), node);
        }
    }

    /// Insert or update a node and persist it to a `<id>.md` file.
    pub fn save_node(&mut self, node: &Node) -> std::io::Result<()> {
        let path = self.root.join(format!("{}.md", node.id));
        std::fs::write(&path, node.to_markdown())?;
        self.nodes.insert(node.id.clone(), node.clone());
        Ok(())
    }

    /// Add a bookmark (with optional parent link) and return its id.
    pub fn add_bookmark(
        &mut self,
        title: &str,
        url: &str,
        parent: Option<&str>,
    ) -> std::io::Result<String> {
        let mut node = Node::new(title, Some(url.to_string()));
        if let Some(parent) = parent {
            node.link(parent);
            if let Some(p) = self.nodes.get_mut(parent) {
                p.link(&node.id);
                // persist the parent's new back-link too
                let parent_path = self.root.join(format!("{}.md", parent));
                std::fs::write(&parent_path, p.to_markdown())?;
            }
        }
        let id = node.id.clone();
        self.save_node(&node)?;
        Ok(id)
    }

    /// Find a node by title or id (case-insensitive).
    pub fn find(&self, needle: &str) -> Option<&Node> {
        if let Some(n) = self.nodes.get(needle) {
            return Some(n);
        }
        let needle = needle.to_lowercase();
        self.nodes.values().find(|n| {
            n.title.to_lowercase() == needle
                || n.id.to_lowercase() == needle
                || n.url.as_deref().map(|u| u.to_lowercase() == needle).unwrap_or(false)
        })
    }

    /// All edges (id, id) sorted, for the future minimap / graph view.
    pub fn edges(&self) -> Vec<(String, String)> {
        let mut out = BTreeSet::new();
        for n in self.nodes.values() {
            for l in &n.links {
                let (a, b) = if n.id < *l { (&n.id, l) } else { (l, &n.id) };
                out.insert((a.clone(), b.clone()));
            }
        }
        out.into_iter().collect()
    }

    /// Children (outgoing links) of a node.
    pub fn children(&self, id: &str) -> Vec<&Node> {
        let mut out = vec![];
        if let Some(n) = self.nodes.get(id) {
            for l in &n.links {
                if let Some(c) = self.nodes.get(l) {
                    out.push(c);
                }
            }
        }
        out
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn mtime(p: &Path) -> Option<u64> {
    std::fs::metadata(p)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

/// Slugify a title into an id: lowercase, non-alphanumeric -> '-'.
pub fn slugify(title: &str) -> String {
    let mut s = String::new();
    let mut last_dash = false;
    for c in title.chars() {
        if c.is_alphanumeric() {
            s.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            s.push('-');
            last_dash = true;
        }
    }
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "node".to_string()
    } else {
        s
    }
}

fn yaml_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn unquote(s: &str) -> Option<String> {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        Some(s[1..s.len() - 1].to_string())
    } else if s.len() >= 2 && s.starts_with('\'') && s.ends_with('\'') {
        Some(s[1..s.len() - 1].to_string())
    } else if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

fn parse_wikilink_line(line: &str) -> Option<String> {
    let t = line.trim().trim_start_matches("- ").trim();
    if let Some(rest) = t.strip_prefix("[[") {
        if let Some(id) = rest.strip_suffix("]]") {
            // Strip any alias "[[id|alias]]"
            let id = id.split('|').next().unwrap_or(id).trim();
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Rust Programming"), "rust-programming");
        assert_eq!(slugify("  Hello World!!  "), "hello-world");
        assert_eq!(slugify("..."), "node");
    }

    #[test]
    fn node_markdown_roundtrip() {
        let mut n = Node::new("Rust Docs", Some("https://doc.rust-lang.org".into()));
        n.tags = vec!["rust".into(), "docs".into()];
        n.link("cargo");
        n.body = "Some notes".to_string();
        let md = n.to_markdown();
        let back = Node::from_markdown(&n.id, &md, n.created, n.updated);
        assert_eq!(back.title, "Rust Docs");
        assert_eq!(back.url.as_deref(), Some("https://doc.rust-lang.org"));
        assert_eq!(back.tags, vec!["rust", "docs"]);
        assert_eq!(back.links, vec!["cargo"]);
    }

    #[test]
    fn edges_deduplicated() {
        let mut m = Mindmap::default();
        let mut a = Node::new("A", None);
        a.link("b");
        let mut b = Node::new("B", None);
        b.link("a");
        m.nodes.insert("a".into(), a);
        m.nodes.insert("b".into(), b);
        assert_eq!(m.edges(), vec![("a".to_string(), "b".to_string())]);
    }
}