//! nex-graph: petgraph-based semantic code graph with diff capability.
//!
//! This crate owns:
//! - `CodeGraph` - a directed graph of semantic units with dependency edges
//! - Graph construction from `SemanticUnit` vectors
//! - Graph diff algorithm (match names at their file paths, compare hashes)
//! - Caller/dependency queries
//!
//! The `CodeGraph` API is specified in the Implementation Specification and
//! constitutes a contract. Codex implements the function bodies.
//!
//! Reviewing this file:
//! - `diff()` matches `qualified_name` at the same path first. An unmatched
//!   name can match across paths only if exactly one copy remains on each side.
//! - Dependency edges do not influence diff classification; they matter later
//!   for coordination and validation only.
//! - If you change diff semantics, update `CORE_INVARIANTS.md` and the
//!   property tests in `tests/diff_invariants.rs`.

use nex_core::{
    ChangeKind, DepKind, ModifiedUnit, MovedUnit, SemanticDiff, SemanticId, SemanticUnit,
};
use petgraph::Direction;
use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::HashMap;
use std::path::Path;

/// A dependency edge in the code graph.
#[derive(Debug, Clone)]
pub struct DepEdge {
    pub kind: DepKind,
}

/// A node wrapper in the code graph.
#[derive(Debug, Clone)]
pub struct SemanticNode {
    pub unit: SemanticUnit,
}

/// Semantic code graph backed by petgraph.
///
/// Provides O(1) lookups by `SemanticId`, directed dependency edges,
/// and a `diff()` method that compares two graphs to produce a `SemanticDiff`.
pub struct CodeGraph {
    graph: DiGraph<SemanticNode, DepEdge>,
    index: HashMap<SemanticId, NodeIndex>,
}

impl CodeGraph {
    /// Create an empty code graph.
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            index: HashMap::new(),
        }
    }

    /// Add a semantic unit as a node. Returns its NodeIndex.
    pub fn add_unit(&mut self, unit: SemanticUnit) -> NodeIndex {
        let id = unit.id;
        let index = self.graph.add_node(SemanticNode { unit });
        self.index.insert(id, index);
        index
    }

    /// Add a dependency edge between two units.
    ///
    /// Idempotent: re-adding an existing `(from, to, kind)` edge is a no-op, so
    /// files with byte-identical content (whose edges are reported once per
    /// file) do not produce parallel duplicate edges.
    pub fn add_dep(&mut self, from: SemanticId, to: SemanticId, kind: DepKind) {
        let Some(from_index) = self.index.get(&from).copied() else {
            return;
        };
        let Some(to_index) = self.index.get(&to).copied() else {
            return;
        };
        if self
            .graph
            .edges_connecting(from_index, to_index)
            .any(|edge| edge.weight().kind == kind)
        {
            return;
        }

        self.graph.add_edge(from_index, to_index, DepEdge { kind });
    }

    /// Look up a unit by its SemanticId.
    pub fn get(&self, id: &SemanticId) -> Option<&SemanticUnit> {
        let index = self.index.get(id)?;
        self.graph.node_weight(*index).map(|node| &node.unit)
    }

    /// Find all units that call/depend on the given unit (incoming edges).
    pub fn callers_of(&self, id: &SemanticId) -> Vec<&SemanticUnit> {
        let Some(index) = self.index.get(id).copied() else {
            return Vec::new();
        };

        self.graph
            .neighbors_directed(index, Direction::Incoming)
            .filter_map(|neighbor| self.graph.node_weight(neighbor).map(|node| &node.unit))
            .collect()
    }

    /// Find all units that the given unit depends on (outgoing edges).
    pub fn deps_of(&self, id: &SemanticId) -> Vec<&SemanticUnit> {
        let Some(index) = self.index.get(id).copied() else {
            return Vec::new();
        };

        self.graph
            .neighbors_directed(index, Direction::Outgoing)
            .filter_map(|neighbor| self.graph.node_weight(neighbor).map(|node| &node.unit))
            .collect()
    }

    /// Return all units in the graph.
    pub fn units(&self) -> Vec<&SemanticUnit> {
        self.graph.node_weights().map(|node| &node.unit).collect()
    }

    /// Return the number of units in the graph.
    pub fn unit_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Return the number of dependency edges in the graph.
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    /// Compute the semantic diff between `self` (before) and `other` (after).
    ///
    /// Algorithm (from spec):
    /// - Match units by `qualified_name` at the same path first, then compare
    ///   uniquely unmatched names across paths.
    /// - Same name + different `signature_hash` -> SignatureChanged
    /// - Same signature + different `body_hash` -> BodyChanged
    /// - Same `body_hash` + different `file_path` -> Moved
    /// - Unmatched in `other` -> Added
    /// - Unmatched in `self` -> Removed
    pub fn diff(&self, other: &CodeGraph) -> SemanticDiff {
        let mut before = self.units_by_location();
        let mut unmatched_after: HashMap<&str, Vec<&SemanticUnit>> = HashMap::new();
        let mut diff = SemanticDiff {
            added: Vec::new(),
            removed: Vec::new(),
            modified: Vec::new(),
            moved: Vec::new(),
        };
        for (location, after_unit) in other.units_by_location() {
            if let Some(before_unit) = before.remove(&location) {
                record_matched_unit(&mut diff, before_unit, after_unit);
            } else {
                unmatched_after
                    .entry(location.0)
                    .or_default()
                    .push(after_unit);
            }
        }
        let mut unmatched_before: HashMap<&str, Vec<&SemanticUnit>> = HashMap::new();
        for ((name, _), before_unit) in before {
            unmatched_before.entry(name).or_default().push(before_unit);
        }
        for (name, after_units) in unmatched_after {
            let before_units = unmatched_before.remove(name).unwrap_or_default();
            if let ([before_unit], [after_unit]) = (before_units.as_slice(), after_units.as_slice())
            {
                record_matched_unit(&mut diff, before_unit, after_unit);
            } else {
                diff.added.extend(after_units.into_iter().cloned());
                diff.removed.extend(before_units.into_iter().cloned());
            }
        }
        diff.removed
            .extend(unmatched_before.into_values().flatten().cloned());
        diff.added
            .sort_by(|a, b| unit_location(a).cmp(&unit_location(b)));
        diff.removed
            .sort_by(|a, b| unit_location(a).cmp(&unit_location(b)));
        diff.modified
            .sort_by(|a, b| unit_location(&a.after).cmp(&unit_location(&b.after)));
        diff.moved
            .sort_by(|a, b| unit_location(&a.unit).cmp(&unit_location(&b.unit)));
        diff
    }

    fn units_by_location(&self) -> HashMap<(&str, &Path), &SemanticUnit> {
        let mut units = HashMap::<_, &SemanticUnit>::new();
        for node in self.graph.node_weights() {
            let unit = &node.unit;
            let previous = units.entry(unit_location(unit)).or_insert(unit);
            // With repeated definitions of a name at one path, keep the later one.
            if unit.byte_range.start >= previous.byte_range.start {
                *previous = unit;
            }
        }
        units
    }
}

fn unit_location(unit: &SemanticUnit) -> (&str, &Path) {
    (&unit.qualified_name, &unit.file_path)
}

fn record_matched_unit(diff: &mut SemanticDiff, before: &SemanticUnit, after: &SemanticUnit) {
    let mut changes = Vec::new();
    if before.signature_hash != after.signature_hash {
        changes.push(ChangeKind::SignatureChanged);
    }
    if before.body_hash != after.body_hash {
        changes.push(ChangeKind::BodyChanged);
    }
    if !changes.is_empty() {
        diff.modified.push(ModifiedUnit {
            before: before.clone(),
            after: after.clone(),
            changes,
        });
    } else if before.file_path != after.file_path {
        diff.moved.push(MovedUnit {
            unit: after.clone(),
            old_path: before.file_path.clone(),
            new_path: after.file_path.clone(),
        });
    }
}

impl Default for CodeGraph {
    fn default() -> Self {
        Self::new()
    }
}
