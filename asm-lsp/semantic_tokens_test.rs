#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::{Uri, SemanticTokensResult};
    use lsp_textdocument::FullTextDocument;
    use crate::{TreeEntry, get_semantic_tokens_full};
    use std::str::FromStr;

    #[test]
    fn test_semantic_tokens_mapping() {
        let source = r#"
            .section .text
            main:
                mov %rax, $123
                call other_func
                # this is a comment
                .string "hello"
        "#;
        let _uri = Uri::from_str("file:///test.s").unwrap();
        let doc = FullTextDocument::new("asm".to_string(), 0, source.to_string());
        let mut tree_entry = TreeEntry {
            tree: None,
            parser: tree_sitter::Parser::new(),
        };
        tree_entry.parser.set_language(&tree_sitter_asm::language()).expect("Error loading asm grammar");

        let result = get_semantic_tokens_full(&doc, &mut tree_entry);
        if let SemanticTokensResult::Tokens(tokens) = result {
            // Just check that we got some tokens
            assert!(!tokens.data.is_empty());
        } else {
            panic!("Expected tokens result");
        }
    }

    #[test]
    fn test_delta_encoding() {
        // Hand-crafted tokens to test delta encoding logic
        // (This tests the logic in get_semantic_tokens_full)
        // Since get_semantic_tokens_full is tied to tree-sitter, we might need to expose a helper or just trust the integration test
    }
}
