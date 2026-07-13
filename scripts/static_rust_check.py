#!/usr/bin/env python3
"""High-signal Rust syntax/duplicate contract checks for restricted build environments.

This is not a compiler replacement. It catches parser errors and common merge/edit
mistakes that tree-sitter alone accepts, such as duplicate function parameter names,
duplicate struct fields, duplicate struct literal initializers, and duplicate methods
inside one inherent impl block.
"""
from __future__ import annotations

from collections import defaultdict
from pathlib import Path
import sys

from tree_sitter import Language, Parser
import tree_sitter_rust

ROOT = Path(__file__).resolve().parents[1]
LANGUAGE = Language(tree_sitter_rust.language())


def text(source: bytes, node) -> str:
    return source[node.start_byte:node.end_byte].decode("utf-8", errors="replace")


def walk(node):
    stack = [node]
    while stack:
        current = stack.pop()
        yield current
        stack.extend(reversed(current.children))


def named_identifier(source: bytes, node) -> str | None:
    for child in walk(node):
        if child.type in {"identifier", "field_identifier", "type_identifier"}:
            return text(source, child)
    return None


def direct_named(node, kinds: set[str]):
    return [child for child in node.named_children if child.type in kinds]


def check_file(path: Path) -> list[str]:
    source = path.read_bytes()
    parser = Parser(LANGUAGE)
    tree = parser.parse(source)
    errors: list[str] = []
    rel = path.relative_to(ROOT)

    for node in walk(tree.root_node):
        if node.type == "ERROR" or node.is_missing:
            snippet = text(source, node).splitlines()[0][:100]
            errors.append(f"{rel}:{node.start_point[0]+1}: parser {node.type}: {snippet}")

        if node.type == "function_item":
            params = node.child_by_field_name("parameters")
            if params:
                names: list[str] = []
                for param in params.named_children:
                    if param.type == "self_parameter":
                        continue
                    pattern = param.child_by_field_name("pattern")
                    name = named_identifier(source, pattern) if pattern else None
                    if name:
                        names.append(name)
                duplicates = sorted({name for name in names if names.count(name) > 1})
                if duplicates:
                    fn_name = named_identifier(source, node.child_by_field_name("name")) or "<unknown>"
                    errors.append(f"{rel}:{node.start_point[0]+1}: duplicate parameters in {fn_name}: {duplicates}")

        if node.type in {"struct_item", "union_item"}:
            body = node.child_by_field_name("body")
            if body:
                names = [
                    named_identifier(source, field.child_by_field_name("name") or field)
                    for field in direct_named(body, {"field_declaration"})
                ]
                names = [name for name in names if name]
                duplicates = sorted({name for name in names if names.count(name) > 1})
                if duplicates:
                    item = named_identifier(source, node.child_by_field_name("name")) or "<unknown>"
                    errors.append(f"{rel}:{node.start_point[0]+1}: duplicate fields in {item}: {duplicates}")

        if node.type == "struct_expression":
            body = node.child_by_field_name("body")
            if body:
                names: list[str] = []
                for field in direct_named(body, {"field_initializer", "shorthand_field_initializer"}):
                    name_node = field.child_by_field_name("field") or field.child_by_field_name("name")
                    name = named_identifier(source, name_node or field)
                    if name:
                        names.append(name)
                duplicates = sorted({name for name in names if names.count(name) > 1})
                if duplicates:
                    errors.append(f"{rel}:{node.start_point[0]+1}: duplicate struct literal fields: {duplicates}")

        if node.type == "impl_item":
            body = node.child_by_field_name("body")
            if body:
                methods: list[str] = []
                for child in body.named_children:
                    if child.type == "function_item":
                        name_node = child.child_by_field_name("name")
                        if name_node:
                            methods.append(text(source, name_node))
                duplicates = sorted({name for name in methods if methods.count(name) > 1})
                if duplicates:
                    errors.append(f"{rel}:{node.start_point[0]+1}: duplicate methods in impl: {duplicates}")

    # Duplicate type/module definitions are high-signal merge errors. Top-level
    # functions are intentionally excluded because Rust commonly uses mutually
    # exclusive #[cfg(...)] implementations with the same name. The compiler is
    # the final authority for cfg expansion in a full build environment.
    names_by_kind: dict[str, list[str]] = defaultdict(list)
    for child in tree.root_node.named_children:
        if child.type in {"struct_item", "enum_item", "trait_item", "type_item", "mod_item"}:
            name_node = child.child_by_field_name("name")
            if name_node:
                names_by_kind[child.type].append(text(source, name_node))
    for kind, names in names_by_kind.items():
        for duplicate in sorted({name for name in names if names.count(name) > 1}):
            errors.append(f"{rel}: duplicate top-level {kind}: {duplicate}")
    return errors


def main() -> int:
    paths = sorted(
        path for path in ROOT.rglob("*.rs")
        if "target" not in path.parts and ".everything" not in path.parts
    )
    errors = [error for path in paths for error in check_file(path)]
    if errors:
        print("Rust static contract check failed:", file=sys.stderr)
        print("\n".join(f"- {error}" for error in errors), file=sys.stderr)
        return 1
    print(f"Rust static contract check passed: {len(paths)} files")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
