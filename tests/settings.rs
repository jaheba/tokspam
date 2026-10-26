use tokspam::{Checker, Limits, Reason, check};

#[test]
fn source_size_boundary() {
    let limits = Limits {
        max_source_bytes: Some(5),
        ..Limits::screening()
    };
    assert!(check("x = 1", &limits).is_ok());
    assert_eq!(
        check("x = 10", &limits).unwrap_err().reason,
        Reason::SourceSize
    );
}

#[test]
fn work_budget_is_distinct_from_structural_rejections() {
    let source = "x = 1\n";
    let limits = Limits {
        max_work_units: Some(source.len() + 2),
        ..Limits::screening()
    };
    assert_eq!(
        check(source, &limits).unwrap_err().reason,
        Reason::Inconclusive
    );

    let exact = Limits {
        max_work_units: Some(source.len() + 4),
        ..Limits::screening()
    };
    assert!(check(source, &exact).is_ok());
}

#[test]
fn token_count_bounds_flat_parser_work() {
    let limits = Limits {
        max_tokens: Some(10),
        ..Limits::screening()
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
        ..Limits::screening()
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
        max_indentation_depth: Some(1),
        max_ast_depth: None,
        ..Limits::screening()
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
        check(&source, &Limits::screening()).unwrap_err().reason,
        Reason::QuotedAnnotation
    );
    let limits = Limits {
        check_string_annotations: false,
        ..Limits::screening()
    };
    assert!(check(&source, &limits).is_ok());
}

#[test]
fn annotation_scan_depth_is_configurable() {
    let source = format!("x: '{}'", "word ".repeat(100));
    assert!(check(&source, &Limits::screening()).is_ok());
    let limits = Limits {
        max_annotation_depth: Some(0),
        ..Limits::screening()
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
        max_annotation_depth: Some(1),
        ..Limits::screening()
    };
    let rejection = check(&source, &shallow).unwrap_err();
    assert_eq!(rejection.reason, Reason::QuotedAnnotation);
    assert_eq!(rejection.range, 3..source.len());

    let deep_enough = Limits {
        max_annotation_depth: Some(2),
        ..Limits::screening()
    };
    assert!(check(&source, &deep_enough).is_ok());
}

#[test]
fn malformed_nested_annotation_does_not_invalidate_outer_source() {
    let source = format!("x: \"list['{}']\"", "([)]".repeat(30));
    assert!(check(&source, &Limits::screening()).is_ok());
}

#[test]
fn annotation_scans_share_source_and_token_work_budgets() {
    let source = "x: 'word word'";
    let byte_limited = Limits {
        max_source_bytes: Some(source.len()),
        max_nesting_depth: Some(2),
        ..Limits::screening()
    };
    // Source size measures the outer input; cumulative rescans use work fuel.
    assert!(check(source, &byte_limited).is_ok());
    let work_limited = Limits {
        max_work_units: Some(source.len() + 3),
        ..byte_limited.clone()
    };
    assert_eq!(
        check(source, &work_limited).unwrap_err().reason,
        Reason::Inconclusive
    );

    let token_limited = Limits {
        max_tokens: Some(3),
        max_nesting_depth: Some(2),
        ..Limits::screening()
    };
    assert_eq!(
        check(source, &token_limited).unwrap_err().reason,
        Reason::QuotedAnnotation
    );

    let annotations_disabled = Limits {
        check_string_annotations: false,
        max_source_bytes: Some(source.len()),
        max_tokens: Some(3),
        ..Limits::screening()
    };
    assert!(check(source, &annotations_disabled).is_ok());
}

#[test]
fn parser_and_ast_budgets_are_independent() {
    let limits = Limits {
        max_nesting_depth: Some(3),
        max_ast_depth: Some(100),
        ..Limits::screening()
    };
    assert_eq!(
        check("x = ----1", &limits).unwrap_err().reason,
        Reason::Nesting
    );
    let limits = Limits {
        max_nesting_depth: Some(100),
        max_ast_depth: Some(3),
        ..Limits::screening()
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
            check(&source, &Limits::screening()).unwrap_err().reason,
            Reason::AstDepth
        );
        let parser_only = Limits {
            max_ast_depth: None,
            ..Limits::screening()
        };
        assert!(check(&source, &parser_only).is_ok());
    }
}

#[test]
fn prose_strings_are_not_forward_annotations() {
    let prose = format!("x = \"\"\"{}don't\"\"\"", "word ".repeat(20));
    assert!(check(&prose, &Limits::screening()).is_ok());
    let json = format!("{{\"items\": {}0{}}}", "[".repeat(300), "]".repeat(300));
    let json_source = format!("payload = r'''{json}'''\n");
    assert!(check(&json_source, &Limits::screening()).is_ok());
    assert!(check(&format!("x = '{}'", "(".repeat(101)), &Limits::screening()).is_ok());
    assert!(
        check(
            &format!("x = 'list[{}int{}]'", "list[".repeat(50), "]".repeat(50)),
            &Limits::screening()
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
            check(&source, &Limits::screening()).unwrap_err().reason,
            Reason::QuotedAnnotation
        );
    }
}

#[test]
fn annotation_metadata_and_literal_values_are_not_forward_references() {
    // Long prose can contain unmatched Python syntax without being type source.
    let prose = format!("{}don't parse this ([)]", "word ".repeat(40));
    let quoted_prose = prose.replace("don't", "do not");
    for source in [
        format!("x: Annotated[int, Doc(\"{prose}\")]"),
        format!("x: typing.Annotated[int, Field(description=\"{prose}\")]"),
        format!("def f(x: Annotated[int, Doc(\"{prose}\")]): pass"),
        format!("def f() -> Annotated[int, Doc(\"{prose}\")]: pass"),
        format!("x: Annotated[tuple[int, str], Doc(\"{prose}\"), \"{prose}\"]"),
        format!("x: Annotated[int, {{'doc': [\"{prose}\"]}}]"),
        format!("x: Literal[\"{prose}\", 42]"),
        format!("x: typing_extensions.Literal[\"{prose}\"]"),
        format!("x: list[Literal[\"{prose}\"]]"),
        format!("x: Annotated[int, Annotated[\"{prose}\", 'metadata']]"),
        format!("x: 'Annotated[int, Doc(\"{quoted_prose}\")]'"),
        format!("x: 'Literal[\"{quoted_prose}\"]'"),
    ] {
        let mut checker = Checker::new(Limits::screening());
        assert!(checker.check(&source).is_ok(), "{source}");
        assert!(check(&source, &Limits::screening()).is_ok(), "{source}");
    }
}

#[test]
fn metadata_exclusions_preserve_forward_reference_screening() {
    let deep = format!("{}int{}", "list[".repeat(100), "]".repeat(100));
    for source in [
        format!("x: Annotated['{deep}', Doc('metadata')]"),
        format!("x: typing.Annotated[tuple[int, '{deep}'], 'metadata']"),
        format!("x: tuple[Literal['data'], '{deep}']"),
        format!("x: Annotated[int, 'metadata'] | '{deep}'"),
        format!("def f(x: Annotated[int, 'metadata'], y: '{deep}'): pass"),
        format!("x: NotLiteral['{deep}']"),
        format!("x: Annotated[Annotated['{deep}', 'inner'], 'outer']"),
        format!("x: Callable[[Literal['data']], '{deep}']"),
        format!("x: \"Annotated['{deep}', 'metadata']\""),
        format!("x: \"tuple[Literal['data'], '{deep}']\""),
    ] {
        let mut checker = Checker::new(Limits::screening());
        for _ in 0..2 {
            assert_eq!(
                checker.check(&source).unwrap_err().reason,
                Reason::QuotedAnnotation,
                "{source}"
            );
            assert!(checker.check("x: Annotated[int, 'metadata']").is_ok());
        }
    }
}

#[test]
fn annotation_metadata_does_not_spend_nested_scan_budgets() {
    let source = format!("x: Annotated[int, Doc(\"{}([)]\")]", "word ".repeat(40));
    let limits = Limits {
        max_annotation_depth: Some(0),
        max_source_bytes: Some(source.len()),
        ..Limits::screening()
    };
    assert!(check(&source, &limits).is_ok());

    let quoted = format!("x: \"Annotated[int, Doc('{}([)]')]\"", "word ".repeat(40));
    let one_scan = Limits {
        max_annotation_depth: Some(1),
        ..Limits::screening()
    };
    assert!(check(&quoted, &one_scan).is_ok());

    // The annotation expression itself still consumes structural and token budgets.
    let deep = format!("x: Annotated[int, {}0{}]", "[".repeat(101), "]".repeat(101));
    assert!(check(&deep, &Limits::screening()).is_err());
    let tokens = Limits {
        max_tokens: Some(4),
        ..Limits::screening()
    };
    assert_eq!(
        check(&source, &tokens).unwrap_err().reason,
        Reason::TokenCount
    );
}

#[test]
fn case_can_be_a_variable() {
    let source = format!("case = 1\nx = case{}", " + 1".repeat(51));
    assert!(check(&source, &Limits::screening()).is_ok());
    let expression_statement = format!("case{}\n", " + 1".repeat(51));
    assert!(check(&expression_statement, &Limits::screening()).is_ok());
}

#[test]
fn complex_literal_patterns_count_parser_recursion() {
    let limits = Limits {
        max_ast_depth: None,
        ..Limits::screening()
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
    assert!(check(&source, &Limits::screening()).is_ok());
    let dangerous = format!("x = {}1", "1 + 1 * ".repeat(5000));
    assert_eq!(
        check(&dangerous, &Limits::screening()).unwrap_err().reason,
        Reason::AstDepth
    );
}

#[test]
fn elif_chains_retain_ast_depth() {
    let chain = format!("if False: pass\n{}", "elif False: pass\n".repeat(101));
    assert_eq!(
        check(&chain, &Limits::screening()).unwrap_err().reason,
        Reason::AstDepth
    );

    let independent = "if False: pass\nelif False: pass\n".repeat(60);
    assert!(check(&independent, &Limits::screening()).is_ok());
}

#[test]
fn checker_reuse_resets_state_after_every_rejection_kind() {
    let limits = Limits {
        max_source_bytes: Some(100),
        max_tokens: Some(40),
        max_token_bytes: Some(20),
        max_nesting_depth: Some(4),
        max_indentation_depth: Some(2),
        max_ast_depth: Some(3),
        ..Limits::screening()
    };
    let mut checker = tokspam::Checker::new(limits.clone());
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
        assert!(check(source, &Limits::screening()).is_ok());
        let mut limits = Limits::screening();
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
fn construct_widths_count_only_the_immediate_level() {
    let limits = Limits {
        max_call_arguments: Some(2),
        max_parameters: Some(2),
        max_display_items: Some(2),
        max_slice_items: Some(2),
        max_interpolation_fields: Some(2),
        ..Limits::screening()
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
        ..Limits::screening()
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
            ..Limits::screening()
        };
        assert!(check(source, &exact).is_ok(), "{source}");
        let below = Limits {
            max_parameters: Some(4),
            ..Limits::screening()
        };
        assert_eq!(
            check(source, &below).unwrap_err().reason,
            Reason::Parameters
        );
    }

    let adjacent = "value = f'{a}' 'literal' f'{b}' f'{c}'";
    let exact = Limits {
        max_interpolation_fields: Some(3),
        ..Limits::screening()
    };
    assert!(check(adjacent, &exact).is_ok());
    let below = Limits {
        max_interpolation_fields: Some(2),
        ..Limits::screening()
    };
    assert_eq!(
        check(adjacent, &below).unwrap_err().reason,
        Reason::InterpolationFields
    );
}

#[test]
fn annotation_segments_preserve_parent_state_and_recover_after_child_errors() {
    let limits = Limits {
        max_nesting_depth: Some(8),
        max_ast_depth: Some(8),
        max_display_items: Some(2),
        max_slice_items: Some(2),
        ..Limits::screening()
    };
    let padding = "word ".repeat(10);
    let accepted = format!("if flag:\n x: tuple[int, \"list['{padding}']\"]\n y = [1, 2]\n");
    let parent_width = format!("x: tuple[\"list['{padding}']\", int, str]");
    let child_error = format!("x: tuple[int, \"list['{}']\"]", "([)]".repeat(10));
    let parent_depth = format!(
        "x: tuple[\"list['{padding}']\", {}int{}]",
        "[".repeat(8),
        "]".repeat(8)
    );
    let mut checker = tokspam::Checker::new(limits);

    for _ in 0..3 {
        assert!(checker.check(&accepted).is_ok());
        assert_eq!(
            checker.check(&parent_width).unwrap_err().reason,
            Reason::SliceItems
        );
        assert!(checker.check(&child_error).is_ok());
        assert_eq!(
            checker.check(&parent_depth).unwrap_err().reason,
            Reason::AstDepth
        );
    }
}

#[test]
fn compact_counters_preserve_large_widths_and_indentation_columns() {
    let count = usize::from(u16::MAX) + 1;
    let source = format!("[{}0]", "0,".repeat(count));
    let mut limits = Limits {
        max_tokens: Some(1_000_000),
        max_ast_depth: None,
        max_display_items: Some(count + 1),
        ..Limits::screening()
    };
    assert!(check(&source, &limits).is_ok());
    limits.max_display_items = Some(count);
    assert_eq!(
        check(&source, &limits).unwrap_err().reason,
        Reason::DisplayItems
    );

    // Indentation columns count tab expansion, not bytes or nesting levels.
    // Both the initial indent and its subsequent dedent must retain >16 bits.
    let source = format!("{}x = 1\ny = 2\n", "\t".repeat(count / 8 + 1));
    assert!(check(&source, &limits).is_ok());

    // Unfinished lambda parameter lists do not consume parser depth yet.
    // Growing storage has no former width-stack ceiling for pending lambdas.
    let source = "lambda ".repeat(count);
    assert!(check(&source, &limits).is_ok());
}

#[test]
fn binary_depths_preserve_configured_ast_boundary() {
    let count = 4_096usize;
    let limits = Limits {
        max_ast_depth: Some(4_096),
        ..Limits::screening()
    };
    let accepted = format!("{}1", "1 + ".repeat(count));
    assert!(check(&accepted, &limits).is_ok());
    assert_eq!(
        check(&format!("{accepted} + 1"), &limits)
            .unwrap_err()
            .reason,
        Reason::AstDepth
    );

    // Fill every precedence slot, then fold them into a lower-precedence
    // operator near the ceiling. Depth arithmetic must still happen in u32.
    let mixed = format!("{}1 | 1 ^ 1 & 1 << 1 + 1 * 1 | 1", "1 + ".repeat(count));
    assert_eq!(check(&mixed, &limits).unwrap_err().reason, Reason::AstDepth);
}

#[test]
fn sibling_postfix_operands_do_not_accumulate_ast_depth() {
    for operand in ["A[0, 0]", "f(x)", "a.x", "f(x)[0].value"] {
        let term = [operand; 4].join(" * ");
        let source = format!("x = {}", vec![term; 24].join(" + "));
        let mut checker = Checker::new(Limits::screening());
        assert!(checker.check(&source).is_ok(), "{source}");

        let deep = format!("x = a{}", ".x".repeat(101));
        assert_eq!(checker.check(&deep).unwrap_err().reason, Reason::AstDepth);
        assert!(checker.check(&source).is_ok());
    }
}

#[test]
fn postfix_heights_remain_in_left_and_right_binary_operands() {
    let limits = Limits {
        max_ast_depth: Some(10),
        ..Limits::screening()
    };
    let chain = format!("a{}", ".x".repeat(8));
    for source in [
        format!("x = {chain} + b + c + d"),
        format!("x = a + (b + (c + {chain}))"),
        format!("x = {chain} + b * c + d + e"),
        format!("x = (a + b + c){}", ".x".repeat(8)),
        format!("x = f(a + b + c){}", ".x".repeat(8)),
        format!("x = a[b + c + d]{}", ".x".repeat(8)),
    ] {
        assert_eq!(
            check(&source, &limits).unwrap_err().reason,
            Reason::AstDepth,
            "{source}"
        );
    }

    for suffix in [".x", "()", "[0]"] {
        let source = format!("x = a{}", suffix.repeat(101));
        assert_eq!(
            check(&source, &Limits::screening()).unwrap_err().reason,
            Reason::AstDepth
        );
    }
    let arithmetic = format!("x = {}a", "a + ".repeat(101));
    assert_eq!(
        check(&arithmetic, &Limits::screening()).unwrap_err().reason,
        Reason::AstDepth
    );
}

#[test]
fn grouped_sibling_subtrees_use_maximum_depth() {
    let left = vec!["a * b * c * d"; 55].join(" + ");
    let right = vec!["a * b * c * d * e"; 87].join(" + ");
    // The division's path enters one operand at a time, not both sums.
    let source = format!("x = -(({left}) * r) / ({right})");
    assert!(check(&source, &Limits::screening()).is_ok());

    let deep = vec!["a * b * c * d * e"; 105].join(" + ");
    for source in [
        format!("x = ({deep}) / ({left})"),
        format!("x = ({left}) / ({deep})"),
    ] {
        assert_eq!(
            check(&source, &Limits::screening()).unwrap_err().reason,
            Reason::AstDepth
        );
    }
}
