// codex-parse: Tree-sitter parsing, rowan CST bridge, semantic unit extraction
// Activated in Phase 0
//
// This crate owns:
// - SemanticExtractor trait (language-agnostic extraction interface)
// - Tree-sitter → Rowan bridge (first-of-its-kind)
// - Per-language extractors (TypeScript, Python, Rust, Go, Java)
// - KindMap generation from node-types.json
