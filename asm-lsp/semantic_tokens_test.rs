#[cfg(test)]
mod tests {
    use crate::{TreeEntry, get_semantic_tokens_full};
    use lsp_textdocument::FullTextDocument;
    use lsp_types::SemanticTokensResult;

    // Token type IDs (must match SEMANTIC_TOKENS_LEGEND order)
    const TT_KEYWORD: u32 = 0;
    const TT_VARIABLE: u32 = 1;
    const TT_FUNCTION: u32 = 2;
    const TT_MACRO: u32 = 3;
    const TT_NUMBER: u32 = 5;
    const TT_STRING: u32 = 6;
    const TT_COMMENT: u32 = 7;

    // Modifier bit positions (must match SEMANTIC_TOKENS_LEGEND modifier order)
    const MOD_READONLY: u32 = 1 << 0;
    const MOD_DECL: u32 = 1 << 1;

    /// A decoded semantic token in absolute (line, col, len, type, mods) form.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Decoded {
        line: u32,
        col: u32,
        len: u32,
        tt: u32,
        mods: u32,
        text: String,
    }

    fn run(source: &str) -> Vec<Decoded> {
        let doc = FullTextDocument::new("asm".to_string(), 0, source.to_string());
        let mut tree_entry = TreeEntry {
            tree: None,
            parser: tree_sitter::Parser::new(),
        };
        tree_entry
            .parser
            .set_language(&tree_sitter_asm::language())
            .expect("loading asm grammar");

        let result = get_semantic_tokens_full(&doc, &mut tree_entry);
        let tokens = match result {
            SemanticTokensResult::Tokens(t) => t.data,
            _ => panic!("expected tokens"),
        };

        let lines: Vec<&str> = source.split('\n').collect();
        let mut out = Vec::new();
        let mut line = 0u32;
        let mut col = 0u32;
        for t in tokens {
            if t.delta_line == 0 {
                col += t.delta_start;
            } else {
                line += t.delta_line;
                col = t.delta_start;
            }
            let text = lines
                .get(line as usize)
                .and_then(|l| l.get(col as usize..(col + t.length) as usize))
                .unwrap_or("")
                .to_string();
            out.push(Decoded {
                line,
                col,
                len: t.length,
                tt: t.token_type,
                mods: t.token_modifiers_bitset,
                text,
            });
        }
        out
    }

    fn find<'a>(toks: &'a [Decoded], text: &str) -> Option<&'a Decoded> {
        toks.iter().find(|t| t.text == text)
    }

    #[test]
    fn instruction_is_keyword_operands_are_not() {
        let src = "    mov %rax, $123\n";
        let toks = run(src);
        let mov = find(&toks, "mov").expect("mov token");
        assert_eq!(mov.tt, TT_KEYWORD, "mov should be keyword: {toks:?}");
        // Number 123 must be a number, not keyword.
        let num = toks.iter().find(|t| t.text.contains("123"));
        assert!(num.is_some(), "expected number token, got {toks:?}");
        assert_eq!(num.unwrap().tt, TT_NUMBER);
    }

    #[test]
    fn directive_marker_is_keyword_but_args_are_not() {
        let src = ".section .text\n";
        let toks = run(src);
        let sec = find(&toks, ".section").expect(".section token");
        assert_eq!(sec.tt, TT_KEYWORD);
        // .text argument should NOT be keyword.
        for t in &toks {
            if t.text == ".text" {
                assert_ne!(t.tt, TT_KEYWORD, "directive arg must not be keyword");
            }
        }
    }

    #[test]
    fn label_definition_is_function_declaration() {
        let src = "main:\n    nop\n";
        let toks = run(src);
        let m = find(&toks, "main").expect("label def");
        assert_eq!(m.tt, TT_FUNCTION);
        assert!(m.mods & MOD_DECL != 0, "label def must have declaration mod");
    }

    #[test]
    fn known_label_reference_is_plain_function() {
        let src = "main:\n    bl main\n";
        let toks = run(src);
        let refs: Vec<&Decoded> = toks
            .iter()
            .filter(|t| t.text == "main" && t.mods == 0)
            .collect();
        assert_eq!(refs.len(), 1, "expected one main reference: {toks:?}");
        assert_eq!(refs[0].tt, TT_FUNCTION);
    }

    #[test]
    fn generic_identifier_is_not_function() {
        let src = "    mov %rax, other\n";
        let toks = run(src);
        let other = find(&toks, "other").expect("other ref");
        assert_eq!(other.tt, TT_VARIABLE);
    }

    #[test]
    fn register_wrapped_in_ident_is_variable() {
        let src = "    adr x0, main\n";
        let toks = run(src);
        let x0 = find(&toks, "x0").expect("x0 register");
        assert_eq!(x0.tt, TT_VARIABLE);
    }

    #[test]
    fn equ_constant_definition_and_reference_are_readonly_variable() {
        let src = ".equ BUFFER_SIZE, 1024\n    mov %rax, BUFFER_SIZE\n";
        let toks = run(src);
        // Both occurrences should be variable + readonly.
        let occurrences: Vec<&Decoded> = toks.iter().filter(|t| t.text == "BUFFER_SIZE").collect();
        assert_eq!(occurrences.len(), 2, "expected 2 BUFFER_SIZE tokens: {toks:?}");
        for o in occurrences {
            assert_eq!(o.tt, TT_VARIABLE, "BUFFER_SIZE must be variable");
            assert!(
                o.mods & MOD_READONLY != 0,
                "BUFFER_SIZE must have readonly modifier"
            );
        }
    }

    #[test]
    fn extern_argument_is_not_readonly() {
        let src = ".extern printf\n";
        let toks = run(src);
        let p = find(&toks, "printf").expect("printf ident");
        assert_eq!(p.tt, TT_VARIABLE, ".extern arg must not be function");
        assert!(
            p.mods & MOD_READONLY == 0,
            ".extern arg must not be readonly: {p:?}"
        );
    }

    #[test]
    fn macro_parameter_is_macro() {
        let src = ".macro PRINT value\n";
        let toks = run(src);
        let value = find(&toks, "value").expect("macro parameter");
        assert_eq!(value.tt, TT_MACRO);
    }

    #[test]
    fn line_comment_overrides_other_tokens() {
        // A line comment containing what would otherwise be an instruction +
        // number must produce only a single `comment` token covering the line.
        let src = "    # mov %rax, 123\n    nop\n";
        let toks = run(src);
        let comment_toks: Vec<&Decoded> = toks.iter().filter(|t| t.tt == TT_COMMENT).collect();
        assert!(!comment_toks.is_empty(), "expected a comment token: {toks:?}");
        // No keyword/number/variable inside the comment line (line 0).
        for t in &toks {
            if t.line == 0 && t.tt != TT_COMMENT {
                panic!("unexpected non-comment token on comment line: {t:?}");
            }
        }
        // nop on the next line should still be a keyword.
        let nop = find(&toks, "nop").expect("nop on next line");
        assert_eq!(nop.tt, TT_KEYWORD);
    }

    #[test]
    fn arm_at_comment_overrides_other_tokens() {
        // ARM uses `@` for line comments. Even if grammar doesn't expose it
        // as a comment node, our fallback must suppress other tokens.
        let src = "    mov r0, #1   @ comment with mov and 123\n";
        let toks = run(src);
        // Find the @ comment range.
        let c = toks.iter().find(|t| t.tt == TT_COMMENT);
        assert!(c.is_some(), "expected @ comment token: {toks:?}");
        let c = c.unwrap();
        // No tokens within the comment range on the same line.
        let comment_start = c.col;
        let comment_end = c.col + c.len;
        for t in &toks {
            if t.line == c.line && t.tt != TT_COMMENT {
                let s = t.col;
                let e = t.col + t.len;
                assert!(
                    e <= comment_start || s >= comment_end,
                    "token {t:?} overlaps @-comment range [{comment_start},{comment_end})"
                );
            }
        }
    }

    #[test]
    fn string_literal_is_string() {
        let src = ".string \"hello\"\n";
        let toks = run(src);
        let s = toks.iter().find(|t| t.text.starts_with('"'));
        assert!(s.is_some(), "expected a string token: {toks:?}");
        assert_eq!(s.unwrap().tt, TT_STRING);
    }

    #[test]
    fn realistic_arm_snippet() {
        let src = r#".extern printf
.equ CONST, 42

main:
    bl printf
    mov r0, #1
    @ this is a comment
"#;
        let toks = run(src);

        // .extern marker is keyword
        assert_eq!(find(&toks, ".extern").unwrap().tt, TT_KEYWORD);
        // printf is an unresolved external/generic identifier, not a label.
        let printf_refs: Vec<&Decoded> = toks.iter().filter(|t| t.text == "printf").collect();
        for p in &printf_refs {
            assert_eq!(p.tt, TT_VARIABLE, "printf should be generic variable");
            assert!(p.mods & MOD_READONLY == 0);
        }
        // .equ marker is keyword, CONST is variable + readonly
        assert_eq!(find(&toks, ".equ").unwrap().tt, TT_KEYWORD);
        let c = find(&toks, "CONST").unwrap();
        assert_eq!(c.tt, TT_VARIABLE);
        assert!(c.mods & MOD_READONLY != 0);
        // 42 is a number
        let n = toks.iter().find(|t| t.text == "42").unwrap();
        assert_eq!(n.tt, TT_NUMBER);
        // main is function + declaration
        let m = find(&toks, "main").unwrap();
        assert_eq!(m.tt, TT_FUNCTION);
        assert!(m.mods & MOD_DECL != 0);
        // bl/mov are keywords
        assert_eq!(find(&toks, "bl").unwrap().tt, TT_KEYWORD);
        assert_eq!(find(&toks, "mov").unwrap().tt, TT_KEYWORD);
        // #1 -> number "1"
        assert!(toks.iter().any(|t| t.text == "1" && t.tt == TT_NUMBER));
        // The @ comment line has only a comment token.
        let comment = toks.iter().find(|t| t.tt == TT_COMMENT).expect("comment");
        assert!(comment.text.starts_with('@'));
    }
}
