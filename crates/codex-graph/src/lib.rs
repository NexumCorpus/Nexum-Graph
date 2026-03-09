// codex-graph: petgraph-based semantic code graph with diff capability
// Activated in Phase 0
//
// This crate owns:
// - CodeGraph (DiGraph<SemanticNode, DepEdge>)
// - Graph construction from SemanticUnits
// - Graph diff algorithm (match by qualified_name, compare hashes)
// - Caller/dependency queries
