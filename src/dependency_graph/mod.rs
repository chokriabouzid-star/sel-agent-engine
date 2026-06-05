mod model;
pub mod parsers;

pub use model::{DependencyEdge, DependencyGraph, EdgeKind, FileNode, GraphLanguage, ImportRef};

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

impl DependencyGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self, node: FileNode) {
        self.nodes.insert(normalize(&node.path), node);
    }

    pub fn add_edge(&mut self, from: impl Into<PathBuf>, to: impl Into<PathBuf>, kind: EdgeKind) {
        let from = normalize(&from.into());
        let to = normalize(&to.into());

        if !self.nodes.contains_key(&from) {
            self.add_node(FileNode::new(from.clone(), GraphLanguage::Unknown));
        }
        if !self.nodes.contains_key(&to) {
            self.add_node(FileNode::new(to.clone(), GraphLanguage::Unknown));
        }

        let edge = DependencyEdge::new(from.clone(), to.clone(), kind);
        if !self
            .edges
            .iter()
            .any(|e| e.from == edge.from && e.to == edge.to && e.kind == edge.kind)
        {
            self.edges.push(edge);
        }
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    pub fn dependencies_of(&self, file: &Path) -> Vec<PathBuf> {
        let file = normalize(file);
        let mut out: Vec<PathBuf> = self
            .edges
            .iter()
            .filter(|e| e.from == file)
            .map(|e| e.to.clone())
            .collect();

        out.sort();
        out.dedup();
        out
    }

    pub fn impacted_by(&self, changed: &Path) -> Vec<PathBuf> {
        let changed = normalize(changed);
        let reverse = self.reverse_adjacency();

        let mut visited: HashSet<PathBuf> = HashSet::new();
        let mut queue: VecDeque<PathBuf> = VecDeque::new();

        if let Some(initial) = reverse.get(&changed) {
            for path in initial {
                if visited.insert(path.clone()) {
                    queue.push_back(path.clone());
                }
            }
        }

        while let Some(current) = queue.pop_front() {
            if let Some(next) = reverse.get(&current) {
                for path in next {
                    if visited.insert(path.clone()) {
                        queue.push_back(path.clone());
                    }
                }
            }
        }

        let mut out: Vec<PathBuf> = visited.into_iter().collect();
        out.sort();
        out
    }

    pub fn detect_cycles(&self) -> Vec<Vec<PathBuf>> {
        let mut tarjan = Tarjan::new(self);
        let mut cycles = tarjan.run();
        cycles.sort_by(|a, b| a.first().cmp(&b.first()));
        cycles
    }

    fn reverse_adjacency(&self) -> HashMap<PathBuf, Vec<PathBuf>> {
        let mut reverse: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
        for edge in &self.edges {
            reverse
                .entry(edge.to.clone())
                .or_default()
                .push(edge.from.clone());
        }

        for values in reverse.values_mut() {
            values.sort();
            values.dedup();
        }

        reverse
    }

    fn all_nodes(&self) -> Vec<PathBuf> {
        let mut nodes: Vec<PathBuf> = self.nodes.keys().cloned().collect();

        for edge in &self.edges {
            if !nodes.contains(&edge.from) {
                nodes.push(edge.from.clone());
            }
            if !nodes.contains(&edge.to) {
                nodes.push(edge.to.clone());
            }
        }

        nodes.sort();
        nodes.dedup();
        nodes
    }

    fn has_self_loop(&self, node: &Path) -> bool {
        self.edges.iter().any(|e| e.from == node && e.to == node)
    }
}

struct Tarjan<'a> {
    graph: &'a DependencyGraph,
    index: usize,
    indices: HashMap<PathBuf, usize>,
    lowlinks: HashMap<PathBuf, usize>,
    stack: Vec<PathBuf>,
    on_stack: HashSet<PathBuf>,
    sccs: Vec<Vec<PathBuf>>,
}

impl<'a> Tarjan<'a> {
    fn new(graph: &'a DependencyGraph) -> Self {
        Self {
            graph,
            index: 0,
            indices: HashMap::new(),
            lowlinks: HashMap::new(),
            stack: Vec::new(),
            on_stack: HashSet::new(),
            sccs: Vec::new(),
        }
    }

    fn run(&mut self) -> Vec<Vec<PathBuf>> {
        for node in self.graph.all_nodes() {
            if !self.indices.contains_key(&node) {
                self.visit(node);
            }
        }
        std::mem::take(&mut self.sccs)
    }

    fn visit(&mut self, node: PathBuf) {
        self.indices.insert(node.clone(), self.index);
        self.lowlinks.insert(node.clone(), self.index);
        self.index += 1;

        self.stack.push(node.clone());
        self.on_stack.insert(node.clone());

        for neighbor in self.graph.dependencies_of(&node) {
            if !self.indices.contains_key(&neighbor) {
                self.visit(neighbor.clone());
                if let (Some(low_node), Some(low_neighbor)) = (
                    self.lowlinks.get(&node).copied(),
                    self.lowlinks.get(&neighbor).copied(),
                ) {
                    self.lowlinks
                        .insert(node.clone(), low_node.min(low_neighbor));
                }
            } else if self.on_stack.contains(&neighbor) {
                if let (Some(low_node), Some(idx_neighbor)) = (
                    self.lowlinks.get(&node).copied(),
                    self.indices.get(&neighbor).copied(),
                ) {
                    self.lowlinks
                        .insert(node.clone(), low_node.min(idx_neighbor));
                }
            }
        }

        if self.indices.get(&node) == self.lowlinks.get(&node) {
            let mut scc = Vec::new();

            while let Some(current) = self.stack.pop() {
                self.on_stack.remove(&current);
                scc.push(current.clone());

                if current == node {
                    break;
                }
            }

            if scc.len() > 1 || self.graph.has_self_loop(&node) {
                scc.sort();
                self.sccs.push(scc);
            }
        }
    }
}

fn normalize(path: &Path) -> PathBuf {
    path.components().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn direct_dependencies_are_reported() {
        let mut g = DependencyGraph::new();
        g.add_node(FileNode::new("a.py", GraphLanguage::Python));
        g.add_node(FileNode::new("b.py", GraphLanguage::Python));
        g.add_node(FileNode::new("c.py", GraphLanguage::Python));

        g.add_edge("a.py", "b.py", EdgeKind::Import);
        g.add_edge("a.py", "c.py", EdgeKind::Import);

        assert_eq!(
            g.dependencies_of(Path::new("a.py")),
            vec![p("b.py"), p("c.py")]
        );
    }

    #[test]
    fn impacted_by_is_transitive() {
        let mut g = DependencyGraph::new();
        g.add_edge("api.py", "service.py", EdgeKind::Import);
        g.add_edge("service.py", "db.py", EdgeKind::Import);
        g.add_edge("cli.py", "service.py", EdgeKind::Import);

        let impacted = g.impacted_by(Path::new("db.py"));
        assert_eq!(impacted, vec![p("api.py"), p("cli.py"), p("service.py")]);
    }

    #[test]
    fn add_edge_auto_inserts_unknown_nodes() {
        let mut g = DependencyGraph::new();
        g.add_edge("main.ts", "utils.ts", EdgeKind::Import);

        assert_eq!(g.node_count(), 2);
        assert_eq!(g.edge_count(), 1);
        assert!(g.nodes.contains_key(&p("main.ts")));
        assert!(g.nodes.contains_key(&p("utils.ts")));
    }

    #[test]
    fn detect_simple_cycle() {
        let mut g = DependencyGraph::new();
        g.add_edge("a.py", "b.py", EdgeKind::Import);
        g.add_edge("b.py", "c.py", EdgeKind::Import);
        g.add_edge("c.py", "a.py", EdgeKind::Import);

        let cycles = g.detect_cycles();
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0], vec![p("a.py"), p("b.py"), p("c.py")]);
    }

    #[test]
    fn detect_multiple_cycles() {
        let mut g = DependencyGraph::new();
        g.add_edge("a.py", "b.py", EdgeKind::Import);
        g.add_edge("b.py", "a.py", EdgeKind::Import);

        g.add_edge("x.ts", "y.ts", EdgeKind::Import);
        g.add_edge("y.ts", "z.ts", EdgeKind::Import);
        g.add_edge("z.ts", "x.ts", EdgeKind::Import);

        let cycles = g.detect_cycles();
        assert_eq!(cycles.len(), 2);
        assert!(cycles.contains(&vec![p("a.py"), p("b.py")]));
        assert!(cycles.contains(&vec![p("x.ts"), p("y.ts"), p("z.ts")]));
    }

    #[test]
    fn no_cycles_returns_empty() {
        let mut g = DependencyGraph::new();
        g.add_edge("main.rs", "lib.rs", EdgeKind::Module);
        g.add_edge("lib.rs", "utils.rs", EdgeKind::Import);

        let cycles = g.detect_cycles();
        assert!(cycles.is_empty());
    }

    #[test]
    fn self_loop_counts_as_cycle() {
        let mut g = DependencyGraph::new();
        g.add_edge("a.py", "a.py", EdgeKind::Import);

        let cycles = g.detect_cycles();
        assert_eq!(cycles, vec![vec![p("a.py")]]);
    }
}
