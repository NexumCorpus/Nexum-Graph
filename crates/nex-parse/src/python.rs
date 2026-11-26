//! Python semantic extractor.
//!
//! Implements SemanticExtractor using tree-sitter-python.
//! Extracts: function_definition, class_definition, decorated methods/functions,
//! named lambda assignments, and calls made by module-level statements.

use crate::SemanticExtractor;
use nex_core::{CodexError, CodexResult, DepKind, SemanticId, SemanticUnit, UnitKind};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::Path;
use tree_sitter::{Node, Parser, Tree};

const EXTENSIONS: &[&str] = &["py"];

/// Python semantic extractor backed by tree-sitter.
#[derive(Debug, Default, Clone, Copy)]
pub struct PythonExtractor;

#[derive(Debug, Clone)]
struct UnitInfo {
    unit: SemanticUnit,
    body_range: Option<Range<usize>>,
    references: Vec<(String, DepKind)>,
    imported_calls: HashMap<String, String>,
}

#[derive(Debug, Clone)]
struct ImportSymbol {
    local_name: String,
    module_name: String,
}

impl PythonExtractor {
    /// Create a new Python extractor.
    pub fn new() -> Self {
        let mut parser = Parser::new();
        let language = tree_sitter_python::LANGUAGE.into();
        parser
            .set_language(&language)
            .expect("tree-sitter Python grammar must load");
        Self
    }

    fn parse(&self, path: &Path, source: &[u8]) -> CodexResult<Tree> {
        let mut parser = Parser::new();
        let language = tree_sitter_python::LANGUAGE.into();
        parser
            .set_language(&language)
            .map_err(|err| parse_error(path, format!("failed to load Python grammar: {err}")))?;

        parser
            .parse(source, None)
            .ok_or_else(|| parse_error(path, "tree-sitter returned no parse tree"))
    }

    fn analyze(&self, path: &Path, source: &[u8]) -> CodexResult<(Tree, Vec<UnitInfo>)> {
        let tree = self.parse(path, source)?;
        let root = tree.root_node();
        let mut units = Vec::new();
        self.collect_program_units(root, path, source, &mut units)?;
        let mut calls = Vec::new();
        collect_module_calls(root, &mut calls);
        let imports = top_level_from_imports(root, source);
        let bindings = direct_call_bindings(root, source, &imports);
        if !bindings.is_empty() {
            for info in &mut units {
                let Some(body) = info
                    .body_range
                    .as_ref()
                    .and_then(|range| find_node_by_range(root, range))
                else {
                    continue;
                };
                info.imported_calls = collect_called_names(body, source)
                    .into_iter()
                    .filter(|(_, is_bare)| *is_bare)
                    .filter_map(|(name, _)| {
                        let (end, target) = bindings.get(&name)?;
                        // The caller might run before a later import executes.
                        (*end <= info.unit.byte_range.start).then(|| (name, target.clone()))
                    })
                    .collect();
                if !info.imported_calls.is_empty() {
                    let mut targets: Vec<_> = info.imported_calls.iter().collect();
                    targets.sort_unstable();
                    // Changing an import can change what a caller does with the same body.
                    let binding_text: Vec<_> = targets
                        .iter()
                        .map(|(name, target)| format!("{name}={target}"))
                        .collect();
                    let unit = &mut info.unit;
                    unit.body_hash = hash_u64(
                        format!("{:016x}:{}", unit.body_hash, binding_text.join(",")).as_bytes(),
                    );
                    unit.id = crate::semantic_unit_id(
                        &unit.qualified_name,
                        &unit.file_path,
                        unit.body_hash,
                    );
                }
            }
        }
        let mut occurrences = HashMap::new();
        for call in calls {
            let Some(name) = call
                .child_by_field_name("function")
                .filter(|callee| callee.kind() == "identifier")
                .and_then(|callee| node_text(callee, source))
            else {
                continue;
            };
            let target = bindings
                .get(&name)
                .filter(|(end, _)| *end <= call.start_byte())
                .map_or(name.as_str(), |(_, imported)| imported.as_str());
            let body_hash = hash_u64(
                format!("{:016x}:{target}", normalized_body_hash(call, source)).as_bytes(),
            );
            let occurrence = occurrences.entry(body_hash).or_insert(0);
            *occurrence += 1;
            let label = format!("<call {name} {body_hash:016x}:{occurrence}>");
            units.push(UnitInfo {
                unit: build_unit(
                    UnitKind::Module,
                    label.clone(),
                    module_call_name(path, &label),
                    path,
                    call.byte_range(),
                    hash_u64(b""),
                    body_hash,
                ),
                body_range: None,
                references: vec![(target.to_owned(), DepKind::Calls)],
                imported_calls: HashMap::new(),
            });
        }
        Ok((tree, units))
    }

    fn collect_program_units(
        &self,
        node: Node<'_>,
        path: &Path,
        source: &[u8],
        units: &mut Vec<UnitInfo>,
    ) -> CodexResult<()> {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                "function_definition" => {
                    if let Some(unit) = self.function_unit(child, path, source, None) {
                        units.push(unit);
                    }
                }
                "class_definition" => self.collect_class_units(child, path, source, units, None)?,
                "decorated_definition" => {
                    self.collect_decorated_definition(child, path, source, units)?
                }
                "assignment" => {
                    if let Some(unit) = self.lambda_assignment_unit(child, path, source) {
                        units.push(unit);
                    }
                }
                "expression_statement" => {
                    self.collect_expression_statement(child, path, source, units)
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn collect_expression_statement(
        &self,
        node: Node<'_>,
        path: &Path,
        source: &[u8],
        units: &mut Vec<UnitInfo>,
    ) {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() != "assignment" {
                continue;
            }
            if let Some(unit) = self.lambda_assignment_unit(child, path, source) {
                units.push(unit);
            }
        }
    }

    fn collect_decorated_definition(
        &self,
        node: Node<'_>,
        path: &Path,
        source: &[u8],
        units: &mut Vec<UnitInfo>,
    ) -> CodexResult<()> {
        let Some(definition) = node.child_by_field_name("definition") else {
            return Ok(());
        };

        match definition.kind() {
            "function_definition" => {
                if let Some(unit) =
                    self.function_unit(definition, path, source, Some(node.byte_range()))
                {
                    units.push(unit);
                }
            }
            "class_definition" => {
                self.collect_class_units(definition, path, source, units, Some(node.byte_range()))?
            }
            _ => {}
        }

        Ok(())
    }

    fn collect_class_units(
        &self,
        node: Node<'_>,
        path: &Path,
        source: &[u8],
        units: &mut Vec<UnitInfo>,
        range_override: Option<Range<usize>>,
    ) -> CodexResult<()> {
        let Some(name_node) = node.child_by_field_name("name") else {
            return Ok(());
        };
        let Some(class_name) = node_text(name_node, source) else {
            return Ok(());
        };

        let body = node.child_by_field_name("body");
        units.push(UnitInfo {
            unit: build_unit(
                UnitKind::Class,
                class_name.clone(),
                class_name.clone(),
                path,
                range_override.unwrap_or_else(|| node.byte_range()),
                hash_u64(b""),
                body.map_or_else(|| hash_u64(b""), |body| normalized_body_hash(body, source)),
            ),
            body_range: None,
            references: collect_class_heritage(node, source),
            imported_calls: HashMap::new(),
        });

        let Some(body) = body else {
            return Ok(());
        };

        let mut cursor = body.walk();
        for child in body.named_children(&mut cursor) {
            match child.kind() {
                "function_definition" => {
                    if let Some(unit) = self.method_unit(child, &class_name, path, source, None) {
                        units.push(unit);
                    }
                }
                "decorated_definition" => {
                    let Some(definition) = child.child_by_field_name("definition") else {
                        continue;
                    };
                    if definition.kind() != "function_definition" {
                        continue;
                    }
                    if let Some(unit) = self.method_unit(
                        definition,
                        &class_name,
                        path,
                        source,
                        Some(child.byte_range()),
                    ) {
                        units.push(unit);
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn function_unit(
        &self,
        node: Node<'_>,
        path: &Path,
        source: &[u8],
        range_override: Option<Range<usize>>,
    ) -> Option<UnitInfo> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(name_node, source)?;
        let body = node.child_by_field_name("body");

        Some(UnitInfo {
            unit: build_unit(
                UnitKind::Function,
                name.clone(),
                name,
                path,
                range_override.unwrap_or_else(|| node.byte_range()),
                signature_hash(node, source),
                body.map_or_else(|| hash_u64(b""), |body| normalized_body_hash(body, source)),
            ),
            body_range: body.map(|body| body.byte_range()),
            references: Vec::new(),
            imported_calls: HashMap::new(),
        })
    }

    fn method_unit(
        &self,
        node: Node<'_>,
        class_name: &str,
        path: &Path,
        source: &[u8],
        range_override: Option<Range<usize>>,
    ) -> Option<UnitInfo> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(name_node, source)?;
        let body = node.child_by_field_name("body");

        Some(UnitInfo {
            unit: build_unit(
                UnitKind::Method,
                name.clone(),
                format!("{class_name}::{name}"),
                path,
                range_override.unwrap_or_else(|| node.byte_range()),
                signature_hash(node, source),
                body.map_or_else(|| hash_u64(b""), |body| normalized_body_hash(body, source)),
            ),
            body_range: body.map(|body| body.byte_range()),
            references: Vec::new(),
            imported_calls: HashMap::new(),
        })
    }

    fn lambda_assignment_unit(
        &self,
        node: Node<'_>,
        path: &Path,
        source: &[u8],
    ) -> Option<UnitInfo> {
        let left = node.child_by_field_name("left")?;
        let right = node.child_by_field_name("right")?;
        if right.kind() != "lambda" {
            return None;
        }

        let name = assignment_name(left, source)?;
        let body = right.child_by_field_name("body");

        Some(UnitInfo {
            unit: build_unit(
                UnitKind::Function,
                name.clone(),
                name,
                path,
                node.byte_range(),
                signature_hash(right, source),
                body.map_or_else(|| hash_u64(b""), |body| normalized_body_hash(body, source)),
            ),
            body_range: body.map(|body| body.byte_range()),
            references: Vec::new(),
            imported_calls: HashMap::new(),
        })
    }
}

impl SemanticExtractor for PythonExtractor {
    fn extensions(&self) -> &[&str] {
        EXTENSIONS
    }

    fn extract(&self, path: &Path, source: &[u8]) -> CodexResult<Vec<SemanticUnit>> {
        let (_, units) = self.analyze(path, source)?;
        Ok(units.into_iter().map(|info| info.unit).collect())
    }

    fn dependencies(
        &self,
        units: &[SemanticUnit],
        source: &[u8],
    ) -> CodexResult<Vec<(SemanticId, SemanticId, DepKind)>> {
        if units.is_empty() {
            return Ok(Vec::new());
        }

        // `units` spans the whole snapshot, so the source's own path must be
        // resolved from content rather than assumed to be the first unit's.
        let (tree, analyzed_units) = self.analyze(Path::new("<memory.py>"), source)?;
        let root = tree.root_node();
        let source_paths = crate::resolve_source_paths_with_names(
            units,
            analyzed_units.iter().map(|info| &info.unit),
            python_name_at,
        );

        let unit_by_id: HashMap<SemanticId, &UnitInfo> = source_paths
            .iter()
            .flat_map(|path| {
                analyzed_units.iter().map(move |info| {
                    (
                        crate::semantic_unit_id(
                            &python_name_at(&info.unit, path),
                            path,
                            info.unit.body_hash,
                        ),
                        info,
                    )
                })
            })
            .collect();

        let mut units_by_name: HashMap<&str, Vec<&SemanticUnit>> = HashMap::new();
        for unit in units.iter().filter(|unit| is_python_unit(unit)) {
            units_by_name
                .entry(unit.name.as_str())
                .or_default()
                .push(unit);
        }

        let imports = collect_imports(root, source);
        let mut edges: HashSet<(SemanticId, SemanticId, DepKind)> = HashSet::new();

        for unit in units.iter().filter(|unit| is_python_unit(unit)) {
            let Some(info) = unit_by_id.get(&unit.id).copied() else {
                continue;
            };

            for (target_name, dep_kind) in &info.references {
                if unit.kind == UnitKind::Module {
                    if let Some(target) =
                        module_dependency(&units_by_name, &unit.file_path, target_name)
                        && target.id != unit.id
                    {
                        edges.insert((unit.id, target.id, *dep_kind));
                    }
                } else if let Some(targets) = units_by_name.get(target_name.as_str()) {
                    for target in targets {
                        if target.id != unit.id {
                            edges.insert((unit.id, target.id, *dep_kind));
                        }
                    }
                }
            }

            let Some(body_range) = &info.body_range else {
                continue;
            };
            let Some(body_node) = find_node_by_range(root, body_range) else {
                continue;
            };

            let called_names = collect_called_names(body_node, source);
            for (called_name, is_bare) in called_names {
                if is_bare
                    && let Some(target_name) = info.imported_calls.get(&called_name)
                    && let Some(target) =
                        module_dependency(&units_by_name, &unit.file_path, target_name)
                {
                    if target.id != unit.id {
                        edges.insert((unit.id, target.id, DepKind::Calls));
                    }
                    continue;
                }
                if let Some(targets) = units_by_name.get(called_name.as_str()) {
                    for target in targets {
                        if target.id != unit.id {
                            edges.insert((unit.id, target.id, DepKind::Calls));
                        }
                    }
                }
            }

            let used_identifiers = collect_identifier_names(body_node, source);
            for import in &imports {
                if used_identifiers.contains(import.local_name.as_str()) {
                    edges.insert((
                        unit.id,
                        external_unit_id(&import.module_name, &import.local_name),
                        DepKind::Imports,
                    ));
                }
            }
        }

        let mut edges: Vec<_> = edges.into_iter().collect();
        edges.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then(left.1.cmp(&right.1))
                .then(dep_kind_rank(left.2).cmp(&dep_kind_rank(right.2)))
        });
        Ok(edges)
    }
}

fn module_dependency<'a>(
    units_by_name: &HashMap<&str, Vec<&'a SemanticUnit>>,
    caller_path: &Path,
    target: &str,
) -> Option<&'a SemanticUnit> {
    let (module, name) = target
        .rsplit_once('.')
        .map_or((None, target), |(m, n)| (Some(m), n));
    let mut candidates = units_by_name.get(name)?.iter().copied().filter(|unit| {
        unit.qualified_name == name
            && module.map_or(unit.file_path == caller_path, |module| {
                is_imported_from(caller_path, module, &unit.file_path)
            })
    });
    let found = candidates.next()?;
    candidates.next().is_none().then_some(found)
}

fn is_imported_from(caller: &Path, module: &str, definition: &Path) -> bool {
    let levels = module.bytes().take_while(|ch| *ch == b'.').count();
    let module_path: std::path::PathBuf = module
        .trim_start_matches('.')
        .split('.')
        .filter(|part| !part.is_empty())
        .collect();
    let package_init = module_path.join("__init__.py");
    let module_file = module_path.with_extension("py");
    if levels == 0 {
        definition.ends_with(&module_file) || definition.ends_with(&package_init)
    } else if let Some(parent) = caller.ancestors().nth(levels) {
        definition == parent.join(module_file) || definition == parent.join(package_init)
    } else {
        false
    }
}

fn module_call_name(path: &Path, label: &str) -> String {
    format!("{}::{label}", path.display())
}

fn python_name_at<'a>(unit: &'a SemanticUnit, path: &Path) -> Cow<'a, str> {
    if unit.kind == UnitKind::Module {
        Cow::Owned(module_call_name(path, &unit.name))
    } else {
        Cow::Borrowed(&unit.qualified_name)
    }
}

fn collect_module_calls<'tree>(node: Node<'tree>, calls: &mut Vec<Node<'tree>>) {
    match node.kind() {
        // Existing named declarations own their own call edges.
        "function_definition" | "class_definition" | "decorated_definition" | "lambda" => return,
        "call" => calls.push(node),
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_module_calls(child, calls);
    }
}

fn build_unit(
    kind: UnitKind,
    name: String,
    qualified_name: String,
    path: &Path,
    byte_range: Range<usize>,
    signature_hash: u64,
    body_hash: u64,
) -> SemanticUnit {
    SemanticUnit {
        id: crate::semantic_unit_id(&qualified_name, path, body_hash),
        kind,
        name,
        qualified_name,
        file_path: path.to_path_buf(),
        byte_range,
        signature_hash,
        body_hash,
        dependencies: Vec::new(),
    }
}

fn external_unit_id(module_name: &str, symbol_name: &str) -> SemanticId {
    let digest = blake3::hash(format!("external:{module_name}:{symbol_name}").as_bytes());
    *digest.as_bytes()
}

fn hash_u64(bytes: &[u8]) -> u64 {
    let digest = blake3::hash(bytes);
    let mut truncated = [0u8; 8];
    truncated.copy_from_slice(&digest.as_bytes()[..8]);
    u64::from_le_bytes(truncated)
}

fn signature_hash(node: Node<'_>, source: &[u8]) -> u64 {
    let params = node
        .child_by_field_name("parameters")
        .and_then(|params| node_text(params, source))
        .unwrap_or_default();
    let return_type = node
        .child_by_field_name("return_type")
        .and_then(|return_type| node_text(return_type, source))
        .unwrap_or_default();

    let mut signature = params;
    signature.push_str(&return_type);
    hash_u64(signature.as_bytes())
}

fn normalized_body_hash(node: Node<'_>, source: &[u8]) -> u64 {
    let mut tokens = Vec::new();
    collect_leaf_tokens(node, source, &mut tokens);
    hash_u64(tokens.join(" ").as_bytes())
}

fn collect_leaf_tokens(node: Node<'_>, source: &[u8], tokens: &mut Vec<String>) {
    if node.is_extra() || node.kind() == "comment" {
        return;
    }

    if node.child_count() == 0 {
        if let Some(text) = node_text(node, source)
            && !text.trim().is_empty()
        {
            tokens.push(text);
        }
        return;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_leaf_tokens(child, source, tokens);
    }
}

fn collect_class_heritage(node: Node<'_>, source: &[u8]) -> Vec<(String, DepKind)> {
    let Some(superclasses) = node.child_by_field_name("superclasses") else {
        return Vec::new();
    };

    let mut heritage = Vec::new();
    let mut cursor = superclasses.walk();
    for child in superclasses.named_children(&mut cursor) {
        if let Some(name) = reference_name(child, source) {
            heritage.push((name, DepKind::Inherits));
        }
    }
    heritage
}

fn collect_imports(root: Node<'_>, source: &[u8]) -> Vec<ImportSymbol> {
    let mut imports = Vec::new();
    collect_imports_recursive(root, source, &mut imports);
    imports
}

fn top_level_from_imports(root: Node<'_>, source: &[u8]) -> Vec<(usize, ImportSymbol)> {
    let mut cursor = root.walk();
    root.named_children(&mut cursor)
        .filter(|node| node.kind() == "import_from_statement")
        .flat_map(|node| {
            import_symbols_from_statement(node, source)
                .into_iter()
                .map(move |symbol| (node.end_byte(), symbol))
        })
        .collect()
}

fn direct_call_bindings(
    root: Node<'_>,
    source: &[u8],
    imports: &[(usize, ImportSymbol)],
) -> HashMap<String, (usize, String)> {
    let mut bindings = HashMap::new();
    for (end, import) in imports {
        bindings
            .entry(import.local_name.clone())
            .and_modify(|value| *value = None)
            .or_insert_with(|| Some((*end, import.module_name.clone())));
    }

    // A name used outside its top-level from-import or a bare call may be rebound.
    // In those cases keep the existing heuristic rather than claiming a precise binding.
    let mut nodes = vec![(root, false)];
    while let Some((node, in_top_level_import)) = nodes.pop() {
        if bindings.is_empty() || node.kind() == "wildcard_import" {
            return HashMap::new();
        }
        if node.kind() == "identifier"
            && !in_top_level_import
            && !node.parent().is_some_and(|parent| {
                parent.kind() == "call" && parent.child_by_field_name("function") == Some(node)
            })
            && let Some(name) = node_text(node, source)
        {
            bindings.remove(&name);
        }
        let in_top_level_import = in_top_level_import
            || (node.kind() == "import_from_statement" && node.parent() == Some(root));
        let mut cursor = node.walk();
        nodes.extend(
            node.named_children(&mut cursor)
                .map(|child| (child, in_top_level_import)),
        );
    }
    bindings
        .into_iter()
        .filter_map(|(name, target)| target.map(|target| (name, target)))
        .collect()
}

fn collect_imports_recursive(node: Node<'_>, source: &[u8], imports: &mut Vec<ImportSymbol>) {
    if matches!(
        node.kind(),
        "import_statement" | "import_from_statement" | "future_import_statement"
    ) {
        imports.extend(import_symbols_from_statement(node, source));
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_imports_recursive(child, source, imports);
    }
}

fn import_symbols_from_statement(node: Node<'_>, source: &[u8]) -> Vec<ImportSymbol> {
    match node.kind() {
        "import_statement" | "future_import_statement" => {
            let mut imports = Vec::new();
            let mut cursor = node.walk();
            for child in node.children_by_field_name("name", &mut cursor) {
                match child.kind() {
                    "aliased_import" => {
                        let alias = child
                            .child_by_field_name("alias")
                            .and_then(|alias| node_text(alias, source));
                        let module_name = child
                            .child_by_field_name("name")
                            .and_then(|name| node_text(name, source));
                        if let (Some(local_name), Some(module_name)) = (alias, module_name) {
                            imports.push(ImportSymbol {
                                local_name,
                                module_name,
                            });
                        }
                    }
                    "dotted_name" => {
                        if let Some(module_name) = node_text(child, source) {
                            imports.push(ImportSymbol {
                                local_name: root_name(&module_name).to_string(),
                                module_name,
                            });
                        }
                    }
                    _ => {}
                }
            }
            imports
        }
        "import_from_statement" => {
            let module_name = node
                .child_by_field_name("module_name")
                .and_then(|module_name| node_text(module_name, source))
                .unwrap_or_default();
            if module_name.is_empty() {
                return Vec::new();
            }

            let mut imports = Vec::new();
            let mut cursor = node.walk();
            for child in node.children_by_field_name("name", &mut cursor) {
                match child.kind() {
                    "aliased_import" => {
                        let alias = child
                            .child_by_field_name("alias")
                            .and_then(|alias| node_text(alias, source));
                        let imported_name = child
                            .child_by_field_name("name")
                            .and_then(|name| node_text(name, source));
                        if let (Some(local_name), Some(imported_name)) = (alias, imported_name) {
                            imports.push(ImportSymbol {
                                local_name,
                                module_name: format!("{module_name}.{imported_name}"),
                            });
                        }
                    }
                    "dotted_name" => {
                        if let Some(imported_name) = node_text(child, source) {
                            imports.push(ImportSymbol {
                                local_name: tail_name(&imported_name).to_string(),
                                module_name: format!("{module_name}.{imported_name}"),
                            });
                        }
                    }
                    _ => {}
                }
            }
            imports
        }
        _ => Vec::new(),
    }
}

fn collect_called_names(node: Node<'_>, source: &[u8]) -> HashSet<(String, bool)> {
    let mut names = HashSet::new();
    collect_called_names_recursive(node, source, &mut names);
    names
}

fn collect_called_names_recursive(
    node: Node<'_>,
    source: &[u8],
    names: &mut HashSet<(String, bool)>,
) {
    if node.kind() == "call"
        && let Some(function_node) = node.child_by_field_name("function")
        && let Some(name) = callee_name(function_node, source)
    {
        names.insert((name, function_node.kind() == "identifier"));
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_called_names_recursive(child, source, names);
    }
}

fn callee_name(node: Node<'_>, source: &[u8]) -> Option<String> {
    match node.kind() {
        "identifier" => node_text(node, source),
        "attribute" => node
            .child_by_field_name("attribute")
            .and_then(|attribute| node_text(attribute, source))
            .or_else(|| {
                node.child_by_field_name("object")
                    .and_then(|object| callee_name(object, source))
            }),
        "call" | "parenthesized_expression" | "type" | "subscript" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if let Some(name) = callee_name(child, source) {
                    return Some(name);
                }
            }
            None
        }
        _ => None,
    }
}

fn collect_identifier_names(node: Node<'_>, source: &[u8]) -> HashSet<String> {
    let mut names = HashSet::new();
    collect_identifier_names_recursive(node, source, &mut names);
    names
}

fn collect_identifier_names_recursive(node: Node<'_>, source: &[u8], names: &mut HashSet<String>) {
    if node.kind() == "identifier"
        && let Some(name) = node_text(node, source)
    {
        names.insert(name);
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_identifier_names_recursive(child, source, names);
    }
}

fn reference_name(node: Node<'_>, source: &[u8]) -> Option<String> {
    match node.kind() {
        "identifier" | "dotted_name" => {
            node_text(node, source).map(|name| tail_name(&name).to_string())
        }
        "attribute" => node
            .child_by_field_name("attribute")
            .and_then(|attribute| node_text(attribute, source))
            .or_else(|| {
                node.child_by_field_name("object")
                    .and_then(|object| reference_name(object, source))
            }),
        "type" | "subscript" | "generic_type" | "type_parameter" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if let Some(name) = reference_name(child, source) {
                    return Some(name);
                }
            }
            None
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if let Some(name) = reference_name(child, source) {
                    return Some(name);
                }
            }
            None
        }
    }
}

fn assignment_name(node: Node<'_>, source: &[u8]) -> Option<String> {
    match node.kind() {
        "identifier" => node_text(node, source),
        "pattern_list" | "tuple_pattern" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if let Some(name) = assignment_name(child, source) {
                    return Some(name);
                }
            }
            None
        }
        _ => None,
    }
}

fn node_text(node: Node<'_>, source: &[u8]) -> Option<String> {
    node.utf8_text(source).ok().map(ToOwned::to_owned)
}

fn find_node_by_range<'tree>(node: Node<'tree>, range: &Range<usize>) -> Option<Node<'tree>> {
    if node.start_byte() == range.start && node.end_byte() == range.end {
        return Some(node);
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.start_byte() <= range.start
            && child.end_byte() >= range.end
            && let Some(found) = find_node_by_range(child, range)
        {
            return Some(found);
        }
    }

    None
}

fn dep_kind_rank(kind: DepKind) -> u8 {
    match kind {
        DepKind::Calls => 0,
        DepKind::Imports => 1,
        DepKind::Inherits => 2,
        DepKind::Implements => 3,
        DepKind::Uses => 4,
    }
}

fn is_python_unit(unit: &SemanticUnit) -> bool {
    unit.file_path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| EXTENSIONS.contains(&ext))
}

fn root_name(name: &str) -> &str {
    name.split('.').next().unwrap_or(name)
}

fn tail_name(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

fn parse_error(path: &Path, message: impl Into<String>) -> CodexError {
    CodexError::Parse {
        path: path.display().to_string(),
        message: message.into(),
    }
}
