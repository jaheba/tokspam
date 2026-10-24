use spam::{
    Checker, LimitKind, Limits, MAX_ANNOTATION_DEPTH, MAX_AST_DEPTH, MAX_INDENTATION_DEPTH,
    MAX_NESTING_DEPTH, MAX_SOURCE_BYTES, MAX_TOKEN_BYTES, MAX_TOKENS, Reason, check,
};

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
fn hard_configuration_ceilings_are_checked_before_scanning() {
    let cases = [
        (
            Limits {
                max_source_bytes: MAX_SOURCE_BYTES + 1,
                ..Limits::default()
            },
            LimitKind::SourceBytes,
        ),
        (
            Limits {
                max_tokens: MAX_TOKENS + 1,
                ..Limits::default()
            },
            LimitKind::Tokens,
        ),
        (
            Limits {
                max_token_bytes: Some(MAX_TOKEN_BYTES + 1),
                ..Limits::default()
            },
            LimitKind::TokenBytes,
        ),
        (
            Limits {
                max_nesting_depth: MAX_NESTING_DEPTH + 1,
                ..Limits::default()
            },
            LimitKind::NestingDepth,
        ),
        (
            Limits {
                max_indentation_depth: MAX_INDENTATION_DEPTH + 1,
                ..Limits::default()
            },
            LimitKind::IndentationDepth,
        ),
        (
            Limits {
                max_annotation_depth: MAX_ANNOTATION_DEPTH + 1,
                ..Limits::default()
            },
            LimitKind::AnnotationDepth,
        ),
        (
            Limits {
                max_ast_depth: Some(MAX_AST_DEPTH + 1),
                ..Limits::default()
            },
            LimitKind::AstDepth,
        ),
    ];

    for (limits, expected) in cases {
        let error = Checker::new(limits.clone()).err().unwrap();
        assert_eq!(error.limit, expected);
        let rejection = check("x = 1", &limits).unwrap_err();
        assert_eq!(rejection.reason, Reason::Configuration);
        assert_eq!(rejection.range, 0..0);
    }
}

#[test]
fn exact_hard_ceilings_and_zero_limits_validate() {
    let ceilings = Limits {
        max_source_bytes: MAX_SOURCE_BYTES,
        max_tokens: MAX_TOKENS,
        max_token_bytes: Some(MAX_TOKEN_BYTES),
        max_nesting_depth: MAX_NESTING_DEPTH,
        max_indentation_depth: MAX_INDENTATION_DEPTH,
        max_annotation_depth: MAX_ANNOTATION_DEPTH,
        max_ast_depth: Some(MAX_AST_DEPTH),
        ..Limits::default()
    };
    assert!(ceilings.validate().is_ok());

    let zero = Limits {
        max_source_bytes: 0,
        max_tokens: 0,
        max_token_bytes: Some(0),
        max_nesting_depth: 0,
        max_indentation_depth: 0,
        max_annotation_depth: 0,
        max_ast_depth: Some(0),
        ..Limits::default()
    };
    assert!(zero.validate().is_ok());
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
fn nested_annotation_work_stack_obeys_depth_and_reports_outer_string() {
    let source = format!("x: \"list['{}']\"", "word ".repeat(30));
    let shallow = Limits {
        max_annotation_depth: 1,
        ..Limits::default()
    };
    let rejection = check(&source, &shallow).unwrap_err();
    assert_eq!(rejection.reason, Reason::QuotedAnnotation);
    assert_eq!(rejection.range, 3..source.len());

    let deep_enough = Limits {
        max_annotation_depth: 2,
        ..Limits::default()
    };
    assert!(check(&source, &deep_enough).is_ok());
}

#[test]
fn malformed_nested_annotation_is_mapped_to_the_outer_string() {
    let source = format!("x: \"list['{}']\"", "([)]".repeat(30));
    let rejection = check(&source, &Limits::default()).unwrap_err();
    assert_eq!(rejection.reason, Reason::QuotedAnnotation);
    assert_eq!(rejection.range, 3..source.len());
}

#[test]
fn annotation_scans_share_source_and_token_work_budgets() {
    let source = "x: 'word word'";
    let byte_limited = Limits {
        max_source_bytes: source.len(),
        max_nesting_depth: 2,
        ..Limits::default()
    };
    assert_eq!(
        check(source, &byte_limited).unwrap_err().reason,
        Reason::QuotedAnnotation
    );

    let token_limited = Limits {
        max_tokens: 3,
        max_nesting_depth: 2,
        ..Limits::default()
    };
    assert_eq!(
        check(source, &token_limited).unwrap_err().reason,
        Reason::QuotedAnnotation
    );

    let annotations_disabled = Limits {
        check_string_annotations: false,
        max_source_bytes: source.len(),
        max_tokens: 3,
        ..Limits::default()
    };
    assert!(check(source, &annotations_disabled).is_ok());
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
fn complex_literal_patterns_count_parser_recursion() {
    let limits = Limits {
        max_ast_depth: None,
        ..Limits::default()
    };
    let pattern = format!("match x:\n    case {}1j:\n        pass\n", "1+".repeat(101));
    assert_eq!(
        check(&pattern, &limits).unwrap_err().reason,
        Reason::Nesting
    );

    let variable = format!("case{}\n", " + 1".repeat(101));
    assert!(check(&variable, &limits).is_ok());
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
    let mut checker = spam::Checker::new(limits.clone()).expect("valid limits");
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

#[test]
fn construct_width_limits_are_independent_and_disabled_by_default() {
    let cases = [
        ("x = f(0, 1, 2)", Reason::CallArguments, 0),
        ("def f(a, b, c): pass", Reason::Parameters, 1),
        ("x = lambda a, b, c: a", Reason::Parameters, 1),
        ("x = [0, 1, 2]", Reason::DisplayItems, 2),
        ("x = value[0, 1, 2]", Reason::SliceItems, 3),
        ("x = f'{a}{b}{c}'", Reason::InterpolationFields, 4),
    ];

    for (source, reason, selected) in cases {
        assert!(check(source, &Limits::default()).is_ok());
        let mut limits = Limits::default();
        match selected {
            0 => limits.max_call_arguments = Some(2),
            1 => limits.max_parameters = Some(2),
            2 => limits.max_display_items = Some(2),
            3 => limits.max_slice_items = Some(2),
            4 => limits.max_interpolation_fields = Some(2),
            _ => unreachable!(),
        }
        assert_eq!(check(source, &limits).unwrap_err().reason, reason);
    }
}

#[test]
fn construct_width_hard_ceilings_are_validated() {
    use spam::MAX_CONSTRUCT_WIDTH;

    let cases = [
        (0, LimitKind::CallArguments),
        (1, LimitKind::Parameters),
        (2, LimitKind::DisplayItems),
        (3, LimitKind::SliceItems),
        (4, LimitKind::InterpolationFields),
    ];
    for (selected, expected) in cases {
        let mut limits = Limits::default();
        match selected {
            0 => limits.max_call_arguments = Some(MAX_CONSTRUCT_WIDTH + 1),
            1 => limits.max_parameters = Some(MAX_CONSTRUCT_WIDTH + 1),
            2 => limits.max_display_items = Some(MAX_CONSTRUCT_WIDTH + 1),
            3 => limits.max_slice_items = Some(MAX_CONSTRUCT_WIDTH + 1),
            4 => limits.max_interpolation_fields = Some(MAX_CONSTRUCT_WIDTH + 1),
            _ => unreachable!(),
        }
        assert_eq!(limits.validate().unwrap_err().limit, expected);
    }
}

#[test]
fn construct_widths_count_only_the_immediate_level() {
    let limits = Limits {
        max_call_arguments: Some(2),
        max_parameters: Some(2),
        max_display_items: Some(2),
        max_slice_items: Some(2),
        max_interpolation_fields: Some(2),
        ..Limits::default()
    };

    for source in [
        "x = f(g(1, 2), h(3, 4))",
        "def f(a=(1, 2), b=(3, 4)): pass",
        "x = [[1, 2], [3, 4]]",
        "x = value[other[0, 1], final[2, 3]]",
        "x = f'{value:{width}}'",
        "x = (1, 2,)",
    ] {
        assert!(check(source, &limits).is_ok(), "{source}");
    }
}

#[test]
fn construct_widths_distinguish_python_grammar_roles() {
    let zero = Limits {
        max_call_arguments: Some(0),
        max_parameters: None,
        max_display_items: Some(0),
        max_slice_items: Some(0),
        max_interpolation_fields: None,
        ..Limits::default()
    };
    for source in [
        "class C(A, B, metaclass=M): pass",
        "from package import (a, b, c)",
        "value = (((item)))",
        "value = [x for x in values if x]",
        "value = items[1:2]",
    ] {
        assert!(check(source, &zero).is_ok(), "{source}");
    }

    for source in [
        "def f(a, /, b=1, *args, c, **kwargs): pass",
        "value = lambda a, /, b=1, *args, c, **kwargs: a",
    ] {
        let exact = Limits {
            max_parameters: Some(5),
            ..Limits::default()
        };
        assert!(check(source, &exact).is_ok(), "{source}");
        let below = Limits {
            max_parameters: Some(4),
            ..Limits::default()
        };
        assert_eq!(
            check(source, &below).unwrap_err().reason,
            Reason::Parameters
        );
    }

    let adjacent = "value = f'{a}' 'literal' f'{b}' f'{c}'";
    let exact = Limits {
        max_interpolation_fields: Some(3),
        ..Limits::default()
    };
    assert!(check(adjacent, &exact).is_ok());
    let below = Limits {
        max_interpolation_fields: Some(2),
        ..Limits::default()
    };
    assert_eq!(
        check(adjacent, &below).unwrap_err().reason,
        Reason::InterpolationFields
    );
}
