//! Factory graph validation and dependency resolution.

use crate::model::{EdgeKind, FactoryDef, NodeId, NodeKind};
use std::collections::{BTreeSet, HashMap, HashSet};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum GraphError {
    #[error("factory has no nodes")]
    Empty,
    #[error("duplicate node id `{0}`")]
    DuplicateNode(NodeId),
    #[error("edge references unknown node `{0}`")]
    UnknownNode(NodeId),
    #[error("dependency edges contain a cycle through `{0}`")]
    Cycle(NodeId),
    #[error("rework edge {from}->{to} must have max_traversals > 0")]
    UnboundedRework { from: NodeId, to: NodeId },
    #[error("rework edge must originate at a gate node, `{0}` is not a gate")]
    ReworkNotFromGate(NodeId),
    #[error("rework edge {from}->{to} must target an ancestor of the gate")]
    ReworkNotAncestor { from: NodeId, to: NodeId },
    #[error("max_concurrency must be at least 1")]
    ZeroConcurrency,
    #[error("worker `{0}` must have a timeout > 0 (execution must be bounded)")]
    UnboundedWorker(NodeId),
}

/// Validated, indexed view of a factory definition.
#[derive(Debug, Clone)]
pub struct Graph {
    pub def: FactoryDef,
    deps: HashMap<NodeId, Vec<NodeId>>,
}

impl Graph {
    pub fn new(def: FactoryDef) -> Result<Self, GraphError> {
        if def.nodes.is_empty() {
            return Err(GraphError::Empty);
        }
        if def.max_concurrency == 0 {
            return Err(GraphError::ZeroConcurrency);
        }
        let mut ids = HashSet::new();
        for n in &def.nodes {
            if !ids.insert(n.id.clone()) {
                return Err(GraphError::DuplicateNode(n.id.clone()));
            }
            if n.worker.timeout_secs == 0 {
                return Err(GraphError::UnboundedWorker(n.id.clone()));
            }
        }
        let mut deps: HashMap<NodeId, Vec<NodeId>> =
            def.nodes.iter().map(|n| (n.id.clone(), vec![])).collect();
        for e in &def.edges {
            for id in [&e.from, &e.to] {
                if !ids.contains(id) {
                    return Err(GraphError::UnknownNode(id.clone()));
                }
            }
            if e.kind == EdgeKind::Dependency {
                deps.get_mut(&e.to).unwrap().push(e.from.clone());
            }
        }
        let g = Graph { def, deps };
        g.check_acyclic()?;
        g.check_rework()?;
        Ok(g)
    }

    pub fn dependencies(&self, id: &str) -> &[NodeId] {
        self.deps.get(id).map(Vec::as_slice).unwrap_or(&[])
    }

    /// All transitive dependencies of `id`.
    pub fn ancestors(&self, id: &str) -> BTreeSet<NodeId> {
        let mut out = BTreeSet::new();
        let mut stack: Vec<&str> = vec![id];
        while let Some(n) = stack.pop() {
            for d in self.dependencies(n) {
                if out.insert(d.clone()) {
                    stack.push(d);
                }
            }
        }
        out
    }

    /// All nodes that transitively depend on `id`.
    pub fn descendants(&self, id: &str) -> BTreeSet<NodeId> {
        self.def
            .nodes
            .iter()
            .filter(|n| self.ancestors(&n.id).contains(id))
            .map(|n| n.id.clone())
            .collect()
    }

    fn check_acyclic(&self) -> Result<(), GraphError> {
        // Kahn's algorithm over dependency edges only.
        let mut indeg: HashMap<&str, usize> = self
            .deps
            .iter()
            .map(|(k, v)| (k.as_str(), v.len()))
            .collect();
        let mut queue: Vec<&str> = indeg
            .iter()
            .filter(|(_, d)| **d == 0)
            .map(|(k, _)| *k)
            .collect();
        let mut seen = 0;
        while let Some(n) = queue.pop() {
            seen += 1;
            for e in &self.def.edges {
                if e.kind == EdgeKind::Dependency && e.from == n {
                    let d = indeg.get_mut(e.to.as_str()).unwrap();
                    *d -= 1;
                    if *d == 0 {
                        queue.push(&e.to);
                    }
                }
            }
        }
        if seen != self.deps.len() {
            let stuck = indeg
                .iter()
                .filter(|(_, d)| **d > 0)
                .map(|(k, _)| k.to_string())
                .min()
                .unwrap();
            return Err(GraphError::Cycle(stuck));
        }
        Ok(())
    }

    fn check_rework(&self) -> Result<(), GraphError> {
        for e in self.def.edges.iter().filter(|e| e.kind == EdgeKind::Rework) {
            if e.max_traversals == 0 {
                return Err(GraphError::UnboundedRework {
                    from: e.from.clone(),
                    to: e.to.clone(),
                });
            }
            let from = self.def.nodes.iter().find(|n| n.id == e.from).unwrap();
            if from.kind != NodeKind::Gate {
                return Err(GraphError::ReworkNotFromGate(e.from.clone()));
            }
            if !self.ancestors(&e.from).contains(&e.to) {
                return Err(GraphError::ReworkNotAncestor {
                    from: e.from.clone(),
                    to: e.to.clone(),
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    pub fn node(id: &str, kind: NodeKind) -> Node {
        Node {
            id: id.into(),
            name: id.into(),
            kind,
            worker: WorkerDef {
                provider: "fake".into(),
                model: None,
                effort: None,
                role: id.into(),
                instructions: String::new(),
                permissions: ToolPermissions::default(),
                working_dir: None,
                timeout_secs: 60,
                max_retries: 0,
                provider_options: Default::default(),
            },
        }
    }

    pub fn dep(from: &str, to: &str) -> Edge {
        Edge {
            from: from.into(),
            to: to.into(),
            kind: EdgeKind::Dependency,
            max_traversals: 0,
        }
    }

    fn def(nodes: Vec<Node>, edges: Vec<Edge>) -> FactoryDef {
        FactoryDef {
            name: "t".into(),
            nodes,
            edges,
            max_concurrency: 2,
        }
    }

    #[test]
    fn valid_chain() {
        let g = Graph::new(def(
            vec![node("a", NodeKind::Worker), node("b", NodeKind::Worker)],
            vec![dep("a", "b")],
        ))
        .unwrap();
        assert_eq!(g.dependencies("b"), ["a".to_string()]);
    }

    #[test]
    fn rejects_zero_timeout() {
        let mut n = node("a", NodeKind::Worker);
        n.worker.timeout_secs = 0;
        assert_eq!(
            Graph::new(def(vec![n], vec![])).unwrap_err(),
            GraphError::UnboundedWorker("a".into())
        );
    }

    #[test]
    fn rejects_cycle() {
        let r = Graph::new(def(
            vec![node("a", NodeKind::Worker), node("b", NodeKind::Worker)],
            vec![dep("a", "b"), dep("b", "a")],
        ));
        assert!(matches!(r, Err(GraphError::Cycle(_))));
    }

    #[test]
    fn rejects_unknown_node_and_duplicates() {
        assert_eq!(
            Graph::new(def(vec![node("a", NodeKind::Worker)], vec![dep("a", "x")])).unwrap_err(),
            GraphError::UnknownNode("x".into())
        );
        assert_eq!(
            Graph::new(def(
                vec![node("a", NodeKind::Worker), node("a", NodeKind::Worker)],
                vec![]
            ))
            .unwrap_err(),
            GraphError::DuplicateNode("a".into())
        );
    }

    #[test]
    fn rework_must_be_bounded_from_gate_to_ancestor() {
        let nodes = vec![node("p", NodeKind::Worker), node("g", NodeKind::Gate)];
        let rework = |max| Edge {
            from: "g".into(),
            to: "p".into(),
            kind: EdgeKind::Rework,
            max_traversals: max,
        };
        assert!(matches!(
            Graph::new(def(nodes.clone(), vec![dep("p", "g"), rework(0)])),
            Err(GraphError::UnboundedRework { .. })
        ));
        assert!(Graph::new(def(nodes.clone(), vec![dep("p", "g"), rework(2)])).is_ok());
        // Not from a gate:
        let bad = Edge {
            from: "p".into(),
            to: "g".into(),
            kind: EdgeKind::Rework,
            max_traversals: 1,
        };
        assert!(matches!(
            Graph::new(def(nodes, vec![dep("p", "g"), bad])),
            Err(GraphError::ReworkNotFromGate(_))
        ));
    }
}
