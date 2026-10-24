use spam::{Limits, Reason, check};

#[test]
fn source_size_boundary() {
    let limits = Limits {
        max_source_bytes: 5,
        ..Limits::default()
    };
    assert!(check("x = 1", &limits).is_ok());
    assert_eq!(
        check("x = 10", &limits).unwrap_err().reason,
        Reason::SourceSize
    );
}

#[test]
fn token_count_bounds_flat_parser_work() {
    let limits = Limits {
        max_tokens: 10,
        ..Limits::default()
    };
    assert_eq!(
        check("x = [0, 1, 2, 3, 4]", &limits).unwrap_err().reason,
        Reason::TokenCount
    );
    assert!(check("x = [0, 1]", &limits).is_ok());
}

#[test]
fn token_size_is_configurable() {
    let limits = Limits {
        max_token_bytes: Some(8),
        ..Limits::default()
    };
    assert_eq!(
        check("value = '0123456789'", &limits).unwrap_err().reason,
        Reason::TokenSize
    );
    assert!(check("value = 'short'", &limits).is_ok());
}

#[test]
fn indentation_is_independent_of_expression_depth() {
    let limits = Limits {
        max_indentation_depth: 1,
        max_ast_depth: None,
        ..Limits::default()
    };
    assert!(check("if x:\n pass\ny = 1\n", &limits).is_ok());
    assert_eq!(
        check("if x:\n if y:\n  pass\n", &limits)
            .unwrap_err()
            .reason,
        Reason::Indentation
    );
}

#[test]
fn annotation_screening_can_be_disabled() {
    let source = format!("x: '{}int{}'", "list[".repeat(100), "]".repeat(100));
    assert_eq!(
        check(&source, &Limits::default()).unwrap_err().reason,
        Reason::QuotedAnnotation
    );
    let limits = Limits {
        check_string_annotations: false,
        ..Limits::default()
    };
    assert!(check(&source, &limits).is_ok());
}

#[test]
fn annotation_scan_depth_is_configurable() {
    let source = format!("x: '{}'", "word ".repeat(100));
    assert!(check(&source, &Limits::default()).is_ok());
    let limits = Limits {
        max_annotation_depth: 0,
        ..Limits::default()
    };
    assert_eq!(
        check(&source, &limits).unwrap_err().reason,
        Reason::QuotedAnnotation
    );
}

#[test]
fn parser_and_ast_budgets_are_independent() {
    let limits = Limits {
        max_nesting_depth: 3,
        max_ast_depth: Some(100),
        ..Limits::default()
    };
    assert_eq!(
        check("x = ----1", &limits).unwrap_err().reason,
        Reason::Nesting
    );
    let limits = Limits {
        max_nesting_depth: 100,
        max_ast_depth: Some(3),
        ..Limits::default()
    };
    assert_eq!(
        check("x = 1+1+1+1+1", &limits).unwrap_err().reason,
        Reason::AstDepth
    );
}

#[test]
fn chained_calls_and_subscripts_retain_ast_depth() {
    for suffix in ["()", "[0]"] {
        let source = format!("x = a{}", suffix.repeat(1000));
        assert_eq!(
            check(&source, &Limits::default()).unwrap_err().reason,
            Reason::AstDepth
        );
        let parser_only = Limits {
            max_ast_depth: None,
            ..Limits::default()
        };
        assert!(check(&source, &parser_only).is_ok());
    }
}

#[test]
fn prose_strings_are_not_forward_annotations() {
    let prose = format!("x = \"\"\"{}don't\"\"\"", "word ".repeat(20));
    assert!(check(&prose, &Limits::default()).is_ok());
    assert!(check(&format!("x = '{}'", "(".repeat(101)), &Limits::default()).is_ok());
    assert!(
        check(
            &format!("x = 'list[{}int{}]'", "list[".repeat(50), "]".repeat(50)),
            &Limits::default()
        )
        .is_ok()
    );
}

#[test]
fn forward_annotations_are_still_screened() {
    let nested = format!("{}int{}", "list[".repeat(100), "]".repeat(100));
    for source in [
        format!("x: '{nested}'"),
        format!("def f(x: '{nested}'): pass"),
        format!("def f() -> '{nested}': pass"),
        format!("x: list['{nested}']"),
    ] {
        assert_eq!(
            check(&source, &Limits::default()).unwrap_err().reason,
            Reason::QuotedAnnotation
        );
    }
}

#[test]
fn case_can_be_a_variable() {
    let source = format!("case = 1\nx = case{}", " + 1".repeat(51));
    assert!(check(&source, &Limits::default()).is_ok());
    let expression_statement = format!("case{}\n", " + 1".repeat(51));
    assert!(check(&expression_statement, &Limits::default()).is_ok());
}

#[test]
fn mixed_operator_precedence_does_not_accumulate_sibling_depth() {
    let source = format!("x = {}1", "1 + 1 * ".repeat(51));
    assert!(check(&source, &Limits::default()).is_ok());
    let dangerous = format!("x = {}1", "1 + 1 * ".repeat(5000));
    assert_eq!(
        check(&dangerous, &Limits::default()).unwrap_err().reason,
        Reason::AstDepth
    );
}

#[test]
fn elif_chains_retain_ast_depth() {
    let chain = format!("if False: pass\n{}", "elif False: pass\n".repeat(101));
    assert_eq!(
        check(&chain, &Limits::default()).unwrap_err().reason,
        Reason::AstDepth
    );

    let independent = "if False: pass\nelif False: pass\n".repeat(60);
    assert!(check(&independent, &Limits::default()).is_ok());
}

#[test]
fn checker_reuse_resets_state_after_every_rejection_kind() {
    let limits = Limits {
        max_source_bytes: 100,
        max_tokens: 40,
        max_token_bytes: Some(20),
        max_nesting_depth: 4,
        max_indentation_depth: 2,
        max_ast_depth: Some(3),
        ..Limits::default()
    };
    let mut checker = spam::Checker::new(limits.clone());
    let sources = [
        "x = 1\n",
        "((((((1))))))",
        "1+1+1+1+1",
        "if x:\n if y:\n  if z:\n   pass",
        "([)]",
        "x: '[[[[[[int]]]]]]'",
        "'a very long string token'",
        "a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a",
    ];

    for source in sources.iter().cycle().take(sources.len() * 3) {
        // Each owned source is dropped before the next check; scratch must not
        // borrow input or retain unfinished statements after returning an error.
        let owned = source.to_string();
        assert_eq!(checker.check(&owned), check(&owned, &limits));
        assert!(checker.check("x = 1\n").is_ok());
    }

    assert_eq!(
        checker.check(&"x".repeat(101)).unwrap_err().reason,
        Reason::SourceSize
    );
    assert!(checker.check("x = 1\n").is_ok());
}
