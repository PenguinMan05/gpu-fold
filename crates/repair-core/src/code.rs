use anyhow::{Context, Result};
use tree_sitter::{Node, Parser};

fn parser() -> Result<Parser> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_rust::LANGUAGE.into())?;
    Ok(parser)
}
/// Public symbols and their source ranges; bodies are fetched separately.
pub fn outline(source: &str) -> Result<Vec<String>> {
    let tree = parser()?
        .parse(source, None)
        .context("Could not parse Rust source")?;
    fn walk(node: Node<'_>, source: &[u8], out: &mut Vec<String>) {
        if matches!(
            node.kind(),
            "function_item" | "struct_item" | "enum_item" | "trait_item" | "impl_item" | "mod_item"
        ) {
            if let Some(name) = node
                .child_by_field_name("name")
                .or_else(|| node.child_by_field_name("type"))
            {
                if let Ok(name) = name.utf8_text(source) {
                    out.push(format!(
                        "{} {}:{}-{}",
                        node.kind(),
                        name,
                        node.start_position().row + 1,
                        node.end_position().row + 1
                    ));
                }
            }
        }
        if out.len() >= 80 {
            return;
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            walk(child, source, out);
        }
    }
    let mut out = Vec::new();
    walk(tree.root_node(), source.as_bytes(), &mut out);
    out.truncate(80);
    Ok(out)
}
/// Exact AST item text for existing test functions/modules, including attributes.
pub fn tests(source: &str) -> Result<Vec<String>> {
    let tree = parser()?
        .parse(source, None)
        .context("Could not parse Rust source")?;
    anyhow::ensure!(!tree.root_node().has_error(), "Rust syntax is invalid");
    fn walk(node: Node<'_>, source: &str, out: &mut Vec<String>) {
        let mut cursor = node.walk();
        let mut attributes = Vec::new();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "attribute_item" {
                attributes.push(&source[child.byte_range()]);
                continue;
            }
            let test = attributes.iter().any(|a| {
                a.split_whitespace().collect::<String>().contains("#[test]")
                    || a.contains("cfg") && a.contains("test")
            });
            if test {
                out.push(format!(
                    "{}{}",
                    attributes.join("\n"),
                    &source[child.byte_range()]
                ));
            } else {
                walk(child, source, out);
            }
            attributes.clear();
        }
    }
    let mut out = Vec::new();
    walk(tree.root_node(), source, &mut out);
    Ok(out)
}
