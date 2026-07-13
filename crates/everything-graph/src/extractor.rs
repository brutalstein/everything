use crate::schema::{CodeEntityKind, CodeLanguage, CodeRelationKind, SourceSpan};
use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};
use tree_sitter::{Language, Node, Parser};

#[derive(Debug, Clone)]
pub(crate) struct ExtractedEntity {
    pub id: String,
    pub kind: CodeEntityKind,
    pub name: String,
    pub qualified_name: String,
    pub language: CodeLanguage,
    pub file_path: PathBuf,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub(crate) enum RelationTarget {
    Id(String),
    Name(String),
    TypedName(CodeEntityKind, String),
}

#[derive(Debug, Clone)]
pub(crate) struct ExtractedRelation {
    pub source_id: String,
    pub target: RelationTarget,
    pub kind: CodeRelationKind,
    pub confidence: f32,
    pub evidence_file: PathBuf,
    pub evidence_span: SourceSpan,
    pub extractor: &'static str,
}

#[derive(Debug, Clone)]
pub(crate) struct ExtractedFile {
    pub path: PathBuf,
    pub hash: String,
    pub language: CodeLanguage,
    pub size: u64,
    pub modified_millis: u64,
    pub entities: Vec<ExtractedEntity>,
    pub relations: Vec<ExtractedRelation>,
    pub parse_errors: usize,
}

#[derive(Clone)]
struct Owner {
    id: String,
    name: String,
    qualified_name: String,
    kind: CodeEntityKind,
}

pub(crate) fn project_id(workspace: &Path) -> String {
    stable_id("project", &[&workspace.to_string_lossy()])
}

pub(crate) fn extract_file(
    project_id: &str,
    relative_path: PathBuf,
    content: String,
    hash: String,
    language: CodeLanguage,
    modified_millis: u64,
) -> Result<ExtractedFile> {
    if matches!(language, CodeLanguage::Toml | CodeLanguage::Json) {
        return extract_manifest(
            project_id,
            relative_path,
            content,
            hash,
            language,
            modified_millis,
        );
    }

    let parser_language = parser_language(language)?;
    let mut parser = Parser::new();
    parser
        .set_language(&parser_language)
        .context("failed to configure tree-sitter language")?;
    let tree = parser
        .parse(&content, None)
        .ok_or_else(|| anyhow!("tree-sitter returned no syntax tree"))?;
    let file_id = stable_id("file", &[&relative_path.to_string_lossy()]);
    let file_name = relative_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_owned();
    let root_span = span(tree.root_node());
    let mut context = ExtractionContext {
        source: content.as_bytes(),
        language,
        file_path: relative_path.clone(),
        file_id: file_id.clone(),
        entities: vec![ExtractedEntity {
            id: file_id.clone(),
            kind: CodeEntityKind::File,
            name: file_name,
            qualified_name: relative_path.to_string_lossy().replace('\\', "/"),
            language,
            file_path: relative_path.clone(),
            span: root_span.clone(),
        }],
        relations: vec![ExtractedRelation {
            source_id: project_id.to_owned(),
            target: RelationTarget::Id(file_id),
            kind: CodeRelationKind::Contains,
            confidence: 1.0,
            evidence_file: relative_path.clone(),
            evidence_span: root_span,
            extractor: "tree-sitter",
        }],
        parse_errors: usize::from(tree.root_node().has_error()),
    };
    context.visit(tree.root_node(), &mut Vec::new());

    Ok(ExtractedFile {
        path: relative_path,
        hash,
        language,
        size: content.len() as u64,
        modified_millis,
        entities: context.entities,
        relations: context.relations,
        parse_errors: context.parse_errors,
    })
}

struct ExtractionContext<'a> {
    source: &'a [u8],
    language: CodeLanguage,
    file_path: PathBuf,
    file_id: String,
    entities: Vec<ExtractedEntity>,
    relations: Vec<ExtractedRelation>,
    parse_errors: usize,
}

impl ExtractionContext<'_> {
    fn visit(&mut self, node: Node<'_>, owners: &mut Vec<Owner>) {
        if node.is_error() || node.is_missing() {
            self.parse_errors += 1;
        }

        let mut pushed_owner = false;
        if let Some((mut kind, name)) = self.definition(node, owners) {
            if kind == CodeEntityKind::Function && self.is_test_definition(node, &name) {
                kind = CodeEntityKind::Test;
            }
            if kind == CodeEntityKind::Function
                && owners.last().is_some_and(|owner| {
                    matches!(
                        owner.kind,
                        CodeEntityKind::Class
                            | CodeEntityKind::Trait
                            | CodeEntityKind::Interface
                            | CodeEntityKind::Implementation
                    )
                })
            {
                kind = CodeEntityKind::Method;
            }
            let node_span = span(node);
            let mut qualified_name = owners
                .last()
                .map(|owner| format!("{}::{name}", owner.qualified_name))
                .unwrap_or_else(|| name.clone());
            let mut id = stable_id(
                "entity",
                &[
                    self.language.as_str(),
                    &self.file_path.to_string_lossy(),
                    kind.as_str(),
                    &qualified_name,
                ],
            );
            if self.entities.iter().any(|entity| entity.id == id) {
                qualified_name = format!("{qualified_name}#L{}", node_span.start_line);
                id = stable_id(
                    "entity",
                    &[
                        self.language.as_str(),
                        &self.file_path.to_string_lossy(),
                        kind.as_str(),
                        &qualified_name,
                    ],
                );
            }
            let source_id = owners
                .last()
                .map(|owner| owner.id.clone())
                .unwrap_or_else(|| self.file_id.clone());
            self.entities.push(ExtractedEntity {
                id: id.clone(),
                kind,
                name: name.clone(),
                qualified_name: qualified_name.clone(),
                language: self.language,
                file_path: self.file_path.clone(),
                span: node_span.clone(),
            });
            self.relations.push(ExtractedRelation {
                source_id,
                target: RelationTarget::Id(id.clone()),
                kind: CodeRelationKind::Defines,
                confidence: 1.0,
                evidence_file: self.file_path.clone(),
                evidence_span: node_span.clone(),
                extractor: "tree-sitter",
            });

            if kind == CodeEntityKind::Implementation {
                self.add_implementation_relations(node, &id);
            } else if kind == CodeEntityKind::Class {
                self.add_inheritance_relation(node, &id);
            }

            owners.push(Owner {
                id,
                name,
                qualified_name,
                kind,
            });
            pushed_owner = true;
        }

        self.add_import_relation(node, owners);
        self.add_call_relation(node, owners);
        self.add_type_reference(node, owners);
        self.add_runtime_contract_relations(node, owners);

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit(child, owners);
        }

        if pushed_owner {
            owners.pop();
        }
    }

    fn definition(&self, node: Node<'_>, owners: &[Owner]) -> Option<(CodeEntityKind, String)> {
        let kind = match (self.language, node.kind()) {
            (CodeLanguage::Rust, "mod_item") => CodeEntityKind::Module,
            (CodeLanguage::Rust, "struct_item") => CodeEntityKind::Struct,
            (CodeLanguage::Rust, "enum_item") => CodeEntityKind::Enum,
            (CodeLanguage::Rust, "trait_item") => CodeEntityKind::Trait,
            (CodeLanguage::Rust, "type_item") => CodeEntityKind::TypeAlias,
            (CodeLanguage::Rust, "const_item") => CodeEntityKind::Constant,
            (CodeLanguage::Rust, "static_item") => CodeEntityKind::Variable,
            (CodeLanguage::Rust, "function_item" | "function_signature_item") => {
                CodeEntityKind::Function
            }
            (CodeLanguage::Rust, "impl_item") => CodeEntityKind::Implementation,
            (CodeLanguage::Python, "class_definition") => CodeEntityKind::Class,
            (CodeLanguage::Python, "function_definition") => CodeEntityKind::Function,
            (
                CodeLanguage::JavaScript | CodeLanguage::TypeScript | CodeLanguage::Tsx,
                "class_declaration",
            ) => CodeEntityKind::Class,
            (CodeLanguage::TypeScript | CodeLanguage::Tsx, "interface_declaration") => {
                CodeEntityKind::Interface
            }
            (CodeLanguage::TypeScript | CodeLanguage::Tsx, "type_alias_declaration") => {
                CodeEntityKind::TypeAlias
            }
            (
                CodeLanguage::JavaScript | CodeLanguage::TypeScript | CodeLanguage::Tsx,
                "function_declaration" | "generator_function_declaration",
            ) => CodeEntityKind::Function,
            (
                CodeLanguage::JavaScript | CodeLanguage::TypeScript | CodeLanguage::Tsx,
                "method_definition",
            ) => CodeEntityKind::Method,
            (CodeLanguage::TypeScript | CodeLanguage::Tsx, "internal_module" | "module") => {
                CodeEntityKind::Namespace
            }
            _ => return None,
        };

        let name = if kind == CodeEntityKind::Implementation {
            self.node_text(node)
                .split('{')
                .next()
                .unwrap_or("impl")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            node.child_by_field_name("name")
                .map(|name_node| self.node_text(name_node))
                .filter(|name| !name.is_empty())
                .or_else(|| {
                    (kind == CodeEntityKind::Method)
                        .then(|| self.first_identifier(node))
                        .flatten()
                })?
        };

        if owners.last().is_some_and(|owner| owner.name == name) {
            return None;
        }
        Some((kind, name))
    }

    fn add_import_relation(&mut self, node: Node<'_>, owners: &[Owner]) {
        let is_import = match self.language {
            CodeLanguage::Rust => node.kind() == "use_declaration",
            CodeLanguage::Python => {
                matches!(node.kind(), "import_statement" | "import_from_statement")
            }
            CodeLanguage::JavaScript | CodeLanguage::TypeScript | CodeLanguage::Tsx => {
                node.kind() == "import_statement"
            }
            _ => false,
        };
        if !is_import {
            return;
        }

        let raw = self.node_text(node);
        let target = match self.language {
            CodeLanguage::Rust => raw
                .trim_start_matches("use")
                .trim_end_matches(';')
                .split("::")
                .next()
                .unwrap_or_default()
                .to_owned(),
            CodeLanguage::Python => raw
                .trim_start_matches("from")
                .trim_start_matches("import")
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .trim_matches(',')
                .to_owned(),
            _ => node
                .child_by_field_name("source")
                .map(|source| self.node_text(source).trim_matches(['\'', '"']).to_owned())
                .unwrap_or_else(|| raw),
        };
        if target.is_empty() {
            return;
        }
        self.relations.push(ExtractedRelation {
            source_id: owners
                .last()
                .map(|owner| owner.id.clone())
                .unwrap_or_else(|| self.file_id.clone()),
            target: RelationTarget::Name(target),
            kind: CodeRelationKind::Imports,
            confidence: 1.0,
            evidence_file: self.file_path.clone(),
            evidence_span: span(node),
            extractor: "tree-sitter",
        });
    }

    fn add_call_relation(&mut self, node: Node<'_>, owners: &[Owner]) {
        if !matches!(node.kind(), "call_expression" | "macro_invocation") {
            return;
        }
        let Some(owner) = owners.last() else {
            return;
        };
        if !matches!(
            owner.kind,
            CodeEntityKind::Function | CodeEntityKind::Method | CodeEntityKind::Test
        ) {
            return;
        }
        let callee = node
            .child_by_field_name("function")
            .or_else(|| node.child_by_field_name("macro"))
            .map(|callee| self.node_text(callee))
            .unwrap_or_default();
        let target = normalize_reference(&callee);
        if target.is_empty() || target == owner.name {
            return;
        }
        self.relations.push(ExtractedRelation {
            source_id: owner.id.clone(),
            target: RelationTarget::Name(target),
            kind: if owner.kind == CodeEntityKind::Test {
                CodeRelationKind::Tests
            } else {
                CodeRelationKind::Calls
            },
            confidence: 0.95,
            evidence_file: self.file_path.clone(),
            evidence_span: span(node),
            extractor: "tree-sitter",
        });
    }

    fn add_type_reference(&mut self, node: Node<'_>, owners: &[Owner]) {
        let is_reference = match self.language {
            CodeLanguage::Rust => node.kind() == "type_identifier",
            CodeLanguage::Python => false,
            CodeLanguage::JavaScript | CodeLanguage::TypeScript | CodeLanguage::Tsx => {
                matches!(node.kind(), "type_identifier" | "predefined_type")
            }
            _ => false,
        };
        if !is_reference {
            return;
        }
        let Some(owner) = owners.last() else {
            return;
        };
        let target = normalize_reference(&self.node_text(node));
        if target.is_empty() || target == owner.name {
            return;
        }
        self.relations.push(ExtractedRelation {
            source_id: owner.id.clone(),
            target: RelationTarget::Name(target),
            kind: CodeRelationKind::References,
            confidence: 0.9,
            evidence_file: self.file_path.clone(),
            evidence_span: span(node),
            extractor: "tree-sitter",
        });
    }

    fn add_implementation_relations(&mut self, node: Node<'_>, implementation_id: &str) {
        if let Some(trait_node) = node.child_by_field_name("trait") {
            let target = normalize_reference(&self.node_text(trait_node));
            if !target.is_empty() {
                self.relations.push(ExtractedRelation {
                    source_id: implementation_id.to_owned(),
                    target: RelationTarget::Name(target),
                    kind: CodeRelationKind::Implements,
                    confidence: 1.0,
                    evidence_file: self.file_path.clone(),
                    evidence_span: span(node),
                    extractor: "tree-sitter",
                });
            }
        }
        if let Some(type_node) = node.child_by_field_name("type") {
            let target = normalize_reference(&self.node_text(type_node));
            if !target.is_empty() {
                self.relations.push(ExtractedRelation {
                    source_id: implementation_id.to_owned(),
                    target: RelationTarget::Name(target),
                    kind: CodeRelationKind::References,
                    confidence: 1.0,
                    evidence_file: self.file_path.clone(),
                    evidence_span: span(node),
                    extractor: "tree-sitter",
                });
            }
        }
    }

    fn add_inheritance_relation(&mut self, node: Node<'_>, class_id: &str) {
        let target = match self.language {
            CodeLanguage::Python => node
                .child_by_field_name("superclasses")
                .map(|value| normalize_reference(&self.node_text(value))),
            CodeLanguage::JavaScript | CodeLanguage::TypeScript | CodeLanguage::Tsx => node
                .child_by_field_name("class_heritage")
                .or_else(|| node.child_by_field_name("superclass"))
                .map(|value| normalize_reference(&self.node_text(value))),
            _ => None,
        };
        if let Some(target) = target.filter(|target| !target.is_empty()) {
            self.relations.push(ExtractedRelation {
                source_id: class_id.to_owned(),
                target: RelationTarget::Name(target),
                kind: CodeRelationKind::Extends,
                confidence: 0.95,
                evidence_file: self.file_path.clone(),
                evidence_span: span(node),
                extractor: "tree-sitter",
            });
        }
    }

    fn is_test_definition(&self, node: Node<'_>, name: &str) -> bool {
        if name.starts_with("test_") || name.ends_with("_test") || name.ends_with("Test") {
            return true;
        }
        let start = node.start_byte().saturating_sub(256);
        let prefix =
            std::str::from_utf8(&self.source[start..node.start_byte()]).unwrap_or_default();
        match self.language {
            CodeLanguage::Rust => {
                prefix.contains("#[test]")
                    || prefix.contains("#[tokio::test]")
                    || prefix.contains("#[async_std::test]")
            }
            CodeLanguage::Python => prefix.contains("@pytest.mark") || prefix.contains("@unittest"),
            CodeLanguage::JavaScript | CodeLanguage::TypeScript | CodeLanguage::Tsx => false,
            _ => false,
        }
    }

    fn add_runtime_contract_relations(&mut self, node: Node<'_>, owners: &[Owner]) {
        if !matches!(
            node.kind(),
            "call_expression"
                | "macro_invocation"
                | "member_expression"
                | "subscript"
                | "string"
                | "string_literal"
        ) {
            return;
        }
        let source_id = owners
            .last()
            .map(|owner| owner.id.clone())
            .unwrap_or_else(|| self.file_id.clone());
        let raw = self.node_text(node);
        if raw.is_empty() || raw.len() > 16_384 {
            return;
        }

        if let Some(environment_name) = extract_environment_name(&raw) {
            self.relations.push(ExtractedRelation {
                source_id: source_id.clone(),
                target: RelationTarget::TypedName(
                    CodeEntityKind::EnvironmentVariable,
                    environment_name,
                ),
                kind: CodeRelationKind::UsesEnvironment,
                confidence: 0.98,
                evidence_file: self.file_path.clone(),
                evidence_span: span(node),
                extractor: "runtime-contract",
            });
        }

        if let Some((route, handler)) = extract_route(&raw) {
            let route_id = stable_id(
                "route",
                &[
                    self.language.as_str(),
                    &self.file_path.to_string_lossy(),
                    &route,
                ],
            );
            if !self.entities.iter().any(|entity| entity.id == route_id) {
                self.entities.push(ExtractedEntity {
                    id: route_id.clone(),
                    kind: CodeEntityKind::Route,
                    name: route.clone(),
                    qualified_name: format!("{}::{route}", self.file_path.to_string_lossy()),
                    language: self.language,
                    file_path: self.file_path.clone(),
                    span: span(node),
                });
                self.relations.push(ExtractedRelation {
                    source_id: source_id.clone(),
                    target: RelationTarget::Id(route_id.clone()),
                    kind: CodeRelationKind::Defines,
                    confidence: 1.0,
                    evidence_file: self.file_path.clone(),
                    evidence_span: span(node),
                    extractor: "runtime-contract",
                });
            }
            if let Some(handler) = handler.filter(|value| !value.is_empty()) {
                self.relations.push(ExtractedRelation {
                    source_id: route_id,
                    target: RelationTarget::Name(handler),
                    kind: CodeRelationKind::RoutesTo,
                    confidence: 0.92,
                    evidence_file: self.file_path.clone(),
                    evidence_span: span(node),
                    extractor: "runtime-contract",
                });
            }
        }

        if let Some((event, emitted)) = extract_event(&raw) {
            self.relations.push(ExtractedRelation {
                source_id: source_id.clone(),
                target: RelationTarget::TypedName(CodeEntityKind::Event, event),
                kind: if emitted {
                    CodeRelationKind::Emits
                } else {
                    CodeRelationKind::Handles
                },
                confidence: 0.9,
                evidence_file: self.file_path.clone(),
                evidence_span: span(node),
                extractor: "runtime-contract",
            });
        }

        if let Some((resource, write)) = extract_file_resource(&raw) {
            self.relations.push(ExtractedRelation {
                source_id: source_id.clone(),
                target: RelationTarget::TypedName(
                    CodeEntityKind::External,
                    format!("file:{resource}"),
                ),
                kind: if write {
                    CodeRelationKind::Writes
                } else {
                    CodeRelationKind::Reads
                },
                confidence: 0.88,
                evidence_file: self.file_path.clone(),
                evidence_span: span(node),
                extractor: "runtime-contract",
            });
        }

        if let Some((database_object, mutation)) = extract_sql_object(&raw) {
            self.relations.push(ExtractedRelation {
                source_id,
                target: RelationTarget::TypedName(CodeEntityKind::DatabaseObject, database_object),
                kind: if mutation {
                    CodeRelationKind::Mutates
                } else {
                    CodeRelationKind::Queries
                },
                confidence: 0.86,
                evidence_file: self.file_path.clone(),
                evidence_span: span(node),
                extractor: "runtime-contract",
            });
        }
    }

    fn node_text(&self, node: Node<'_>) -> String {
        node.utf8_text(self.source)
            .unwrap_or_default()
            .trim()
            .to_owned()
    }

    fn first_identifier(&self, node: Node<'_>) -> Option<String> {
        let mut cursor = node.walk();
        node.children(&mut cursor)
            .find(|child| matches!(child.kind(), "identifier" | "property_identifier"))
            .map(|child| self.node_text(child))
    }
}

fn extract_manifest(
    project_id: &str,
    relative_path: PathBuf,
    content: String,
    hash: String,
    language: CodeLanguage,
    modified_millis: u64,
) -> Result<ExtractedFile> {
    let file_id = stable_id("file", &[&relative_path.to_string_lossy()]);
    let file_span = SourceSpan {
        start_byte: 0,
        end_byte: content.len(),
        start_line: 1,
        start_column: 1,
        end_line: content.lines().count().max(1),
        end_column: 1,
    };
    let mut entities = vec![ExtractedEntity {
        id: file_id.clone(),
        kind: CodeEntityKind::File,
        name: relative_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_owned(),
        qualified_name: relative_path.to_string_lossy().replace('\\', "/"),
        language,
        file_path: relative_path.clone(),
        span: file_span.clone(),
    }];
    let mut relations = vec![ExtractedRelation {
        source_id: project_id.to_owned(),
        target: RelationTarget::Id(file_id.clone()),
        kind: CodeRelationKind::Contains,
        confidence: 1.0,
        evidence_file: relative_path.clone(),
        evidence_span: file_span.clone(),
        extractor: "manifest",
    }];

    let (package_name, dependencies) = if language == CodeLanguage::Toml {
        let value: toml::Value = toml::from_str(&content)?;
        let name = value
            .get("package")
            .and_then(|package| package.get("name"))
            .or_else(|| value.get("project").and_then(|project| project.get("name")))
            .and_then(toml::Value::as_str)
            .map(str::to_owned);
        let dependencies = ["dependencies", "dev-dependencies", "build-dependencies"]
            .into_iter()
            .filter_map(|key| value.get(key).and_then(toml::Value::as_table))
            .flat_map(|table| table.keys().cloned())
            .collect::<Vec<_>>();
        (name, dependencies)
    } else {
        let value: serde_json::Value = serde_json::from_str(&content)?;
        let name = value
            .get("name")
            .and_then(|name| name.as_str())
            .map(str::to_owned);
        let dependencies = ["dependencies", "devDependencies", "peerDependencies"]
            .into_iter()
            .filter_map(|key| value.get(key).and_then(|value| value.as_object()))
            .flat_map(|table| table.keys().cloned())
            .collect::<Vec<_>>();
        (name, dependencies)
    };

    if let Some(package_name) = package_name {
        let package_id = stable_id("package", &[&package_name]);
        entities.push(ExtractedEntity {
            id: package_id.clone(),
            kind: CodeEntityKind::Package,
            name: package_name.clone(),
            qualified_name: package_name,
            language,
            file_path: relative_path.clone(),
            span: file_span.clone(),
        });
        relations.push(ExtractedRelation {
            source_id: project_id.to_owned(),
            target: RelationTarget::Id(package_id.clone()),
            kind: CodeRelationKind::Contains,
            confidence: 1.0,
            evidence_file: relative_path.clone(),
            evidence_span: file_span.clone(),
            extractor: "manifest",
        });
        relations.push(ExtractedRelation {
            source_id: package_id.clone(),
            target: RelationTarget::Id(file_id),
            kind: CodeRelationKind::Defines,
            confidence: 1.0,
            evidence_file: relative_path.clone(),
            evidence_span: file_span.clone(),
            extractor: "manifest",
        });
        for dependency in dependencies {
            relations.push(ExtractedRelation {
                source_id: package_id.clone(),
                target: RelationTarget::Name(dependency),
                kind: CodeRelationKind::DependsOn,
                confidence: 1.0,
                evidence_file: relative_path.clone(),
                evidence_span: file_span.clone(),
                extractor: "manifest",
            });
        }
    }

    Ok(ExtractedFile {
        path: relative_path,
        hash,
        language,
        size: content.len() as u64,
        modified_millis,
        entities,
        relations,
        parse_errors: 0,
    })
}

fn parser_language(language: CodeLanguage) -> Result<Language> {
    Ok(match language {
        CodeLanguage::Project => {
            return Err(anyhow!("project language does not use tree-sitter"));
        }
        CodeLanguage::Rust => tree_sitter_rust::LANGUAGE.into(),
        CodeLanguage::Python => tree_sitter_python::LANGUAGE.into(),
        CodeLanguage::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        CodeLanguage::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        CodeLanguage::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        CodeLanguage::Toml | CodeLanguage::Json => {
            return Err(anyhow!("manifest languages do not use tree-sitter"));
        }
    })
}

pub(crate) fn stable_id(prefix: &str, components: &[&str]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(prefix.as_bytes());
    for component in components {
        hasher.update(&[0]);
        hasher.update(component.as_bytes());
    }
    format!("cg_{prefix}_{}", &hasher.finalize().to_hex()[..24])
}

fn normalize_reference(value: &str) -> String {
    value
        .trim()
        .trim_matches(['(', ')', '[', ']', '<', '>', '\'', '"'])
        .split([':', '.', '/', '\\', ' ', ',', '<', '>', '(', ')'])
        .rfind(|part| !part.is_empty() && *part != "super" && *part != "self")
        .unwrap_or_default()
        .trim_matches(|character: char| !character.is_alphanumeric() && character != '_')
        .to_owned()
}

fn quoted_values(raw: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut quote = None;
    let mut escaped = false;
    let mut current = String::new();
    for character in raw.chars() {
        if let Some(active) = quote {
            if escaped {
                current.push(character);
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == active {
                values.push(std::mem::take(&mut current));
                quote = None;
            } else {
                current.push(character);
            }
        } else if character == '\'' || character == '"' {
            quote = Some(character);
        }
    }
    values
}

fn extract_environment_name(raw: &str) -> Option<String> {
    if let Some(index) = raw.find("process.env.") {
        let name = raw[index + "process.env.".len()..]
            .chars()
            .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
            .collect::<String>();
        if !name.is_empty() {
            return Some(name);
        }
    }
    let lower = raw.to_ascii_lowercase();
    if [
        "env::var",
        "getenv",
        "environ.get",
        "std::env::var",
        "var_os",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        return quoted_values(raw)
            .into_iter()
            .find(|value| !value.is_empty() && value.len() <= 256);
    }
    None
}

fn extract_route(raw: &str) -> Option<(String, Option<String>)> {
    let lower = raw.to_ascii_lowercase();
    let route_like = [
        ".route(",
        ".get(",
        ".post(",
        ".put(",
        ".patch(",
        ".delete(",
        "@app.get(",
        "@router.",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    if !route_like {
        return None;
    }
    let route = quoted_values(raw)
        .into_iter()
        .find(|value| value.starts_with('/') && value.len() <= 512)?;
    let handler = raw
        .rsplit([',', '('])
        .next()
        .unwrap_or_default()
        .trim_matches(|character: char| {
            !character.is_ascii_alphanumeric() && character != '_' && character != ':'
        })
        .split("::")
        .last()
        .map(str::to_owned)
        .filter(|value| !value.is_empty() && !value.starts_with('/'));
    Some((route, handler))
}

fn extract_event(raw: &str) -> Option<(String, bool)> {
    let lower = raw.to_ascii_lowercase();
    let emitted =
        lower.contains(".emit(") || lower.contains(".publish(") || lower.contains("send_event(");
    let handled = lower.contains(".on(")
        || lower.contains(".subscribe(")
        || lower.contains("add_event_listener(");
    if !emitted && !handled {
        return None;
    }
    let event = quoted_values(raw)
        .into_iter()
        .find(|value| !value.is_empty() && value.len() <= 256)?;
    Some((event, emitted))
}

fn extract_file_resource(raw: &str) -> Option<(String, bool)> {
    let lower = raw.to_ascii_lowercase();
    let write = [
        "write_to_string",
        "std::fs::write",
        "write_text",
        "write_bytes",
        "openoptions",
        "create(",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    let read = [
        "read_to_string",
        "std::fs::read",
        "read_text",
        "read_bytes",
        "open(",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    if !write && !read {
        return None;
    }
    let path = quoted_values(raw)
        .into_iter()
        .find(|value| !value.is_empty() && value.len() <= 1024)?;
    Some((path, write))
}

fn extract_sql_object(raw: &str) -> Option<(String, bool)> {
    for value in quoted_values(raw) {
        let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
        let upper = normalized.to_ascii_uppercase();
        let (keyword, mutation) = if upper.starts_with("SELECT ") {
            (" FROM ", false)
        } else if upper.starts_with("INSERT ") {
            (" INTO ", true)
        } else if upper.starts_with("UPDATE ") {
            ("UPDATE ", true)
        } else if upper.starts_with("DELETE ") {
            (" FROM ", true)
        } else {
            continue;
        };
        let start = upper.find(keyword)? + keyword.len();
        let object = normalized[start..]
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .trim_matches(|character: char| {
                !character.is_ascii_alphanumeric() && character != '_' && character != '.'
            })
            .to_owned();
        if !object.is_empty() {
            return Some((object, mutation));
        }
    }
    None
}

fn span(node: Node<'_>) -> SourceSpan {
    let start = node.start_position();
    let end = node.end_position();
    SourceSpan {
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
        start_line: start.row + 1,
        start_column: start.column + 1,
        end_line: end.row + 1,
        end_column: end.column + 1,
    }
}
