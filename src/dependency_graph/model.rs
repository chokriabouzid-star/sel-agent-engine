use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub struct DependencyGraph {
    pub nodes: HashMap<PathBuf, FileNode>,
    pub edges: Vec<DependencyEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileNode {
    pub path: PathBuf,
    pub language: GraphLanguage,
    pub imports: Vec<ImportRef>,
    pub exports: Vec<String>,
}

impl FileNode {
    pub fn new(path: impl Into<PathBuf>, language: GraphLanguage) -> Self {
        Self {
            path: path.into(),
            language,
            imports: Vec::new(),
            exports: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyEdge {
    pub from: PathBuf,
    pub to: PathBuf,
    pub kind: EdgeKind,
}

impl DependencyEdge {
    pub fn new(from: impl Into<PathBuf>, to: impl Into<PathBuf>, kind: EdgeKind) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            kind,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphLanguage {
    Python,
    TypeScript,
    Go,
    Rust,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    Import,
    ReExport,
    Module,
    TypeOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportRef {
    pub raw: String,
    pub symbols: Vec<String>,
}

impl ImportRef {
    pub fn new(raw: impl Into<String>, symbols: Vec<String>) -> Self {
        Self {
            raw: raw.into(),
            symbols,
        }
    }
}
