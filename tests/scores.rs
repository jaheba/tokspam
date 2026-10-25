use tokspam::{Checker, Limits, Reason, Storage, analyze, check};

#[test]
fn scores_measure_complete_input_and_ast_shapes() {
    let source = "x = f(1, [2, 3], other(a, b, c))\n";
    let report = analyze(source);
    assert!(report.is_complete(), "{report:?}");
    assert_eq!(report.scores.source_bytes, source.len());
    assert_eq!(report.scores.scanned_bytes, source.len());
    assert_eq!(
        report.scores.work_units,
        source.len() + report.scores.tokens
    );
    assert_eq!(report.scores.call_arguments, 3);
    assert_eq!(report.scores.display_items, 2);
    assert_eq!(report.scores.delimiter_depth, 2);
    assert_eq!(report.scores.parser_depth, 2);
    assert_eq!(analyze("a + b + c").scores.ast_depth, 2);
    assert_eq!(analyze("---1").scores.parser_depth, 3);
    assert_eq!(analyze("---1").scores.ast_depth, 3);
}

#[test]
fn independent_scores_and_python_width_roles() {
    let report = analyze("def f(a, /, b, *, c):\n x = (1, 2, 3)\n y = x[1, 2]\n z = f'{a}{b}'\n");
    assert!(report.is_complete(), "{report:?}");
    assert_eq!(report.scores.parameters, 3);
    assert_eq!(report.scores.display_items, 3);
    assert_eq!(report.scores.slice_items, 2);
    assert_eq!(report.scores.interpolation_fields, 2);
    assert_eq!(report.scores.indentation_depth, 1);
    assert_eq!(analyze("x = [i for i in xs]").scores.display_items, 0);
}

#[test]
fn annotations_share_work_and_preserve_parent_scores() {
    let report = analyze("x: \"list['int']\"");
    assert!(report.is_complete(), "{report:?}");
    assert_eq!(report.scores.annotation_depth, 2);
    assert!(report.scores.scanned_bytes > report.scores.source_bytes);
    assert!(report.scores.parser_depth >= 1);
    assert_eq!(analyze("x: Literal['[[[']").scores.annotation_depth, 0);
    assert_eq!(
        analyze("x: Annotated[int, '[[[']").scores.annotation_depth,
        0
    );
}

#[test]
fn scores_have_no_former_hard_ceilings() {
    let deep = format!("{}0{}", "[".repeat(2000), "]".repeat(2000));
    let report = analyze(&deep);
    assert!(report.is_complete(), "{:?}", report.stopped);
    assert_eq!(report.scores.delimiter_depth, 2000);
    assert_eq!(report.scores.parser_depth, 2000);
    let long_chain = std::iter::repeat_n("x", 5000)
        .collect::<Vec<_>>()
        .join(" + ");
    assert_eq!(analyze(&long_chain).scores.ast_depth, 4999);
    let high_limits = Limits {
        max_nesting_depth: Some(usize::MAX),
        max_tokens: Some(usize::MAX),
        max_ast_depth: Some(usize::MAX),
        max_source_bytes: Some(usize::MAX),
        max_work_units: Some(usize::MAX),
        max_indentation_depth: Some(usize::MAX),
        max_annotation_depth: Some(usize::MAX),
        ..Limits::default()
    };
    assert!(Checker::new(high_limits).check(&deep).is_ok());
}

#[test]
fn fixed_storage_exhaustion_is_incomplete_and_reusable() {
    for capacity in [0, 1, 2, 8] {
        let mut checker =
            Checker::with_storage(Limits::default(), Storage::Fixed { capacity }).unwrap();
        let report = checker.analyze(&format!("{}0{}", "[".repeat(50), "]".repeat(50)));
        assert!(!report.is_complete());
        assert_eq!(report.stopped.unwrap().reason, Reason::Storage);
        if capacity > 0 {
            assert!(checker.analyze("x = 1").is_complete());
        }
    }
    assert!(
        Checker::with_storage(
            Limits::default(),
            Storage::Fixed {
                capacity: usize::MAX
            }
        )
        .is_err()
    );
}

#[test]
fn fixed_storage_exhaustion_precedes_a_simultaneous_score_stop() {
    let limits = Limits {
        max_tokens: Some(0),
        ..Limits::default()
    };
    let mut checker = Checker::with_storage(limits, Storage::Fixed { capacity: 1 }).unwrap();

    let report = checker.analyze("(");
    assert_eq!(report.scores.tokens, 1);
    assert_eq!(report.stopped.unwrap().reason, Reason::Storage);
}

#[test]
fn stops_explain_partial_scores() {
    let mut checker = Checker::new(Limits {
        max_tokens: Some(3),
        ..Limits::default()
    });
    let report = checker.analyze("a + b + c");
    assert_eq!(report.scores.tokens, 4);
    assert_eq!(report.stopped.unwrap().reason, Reason::TokenCount);
    let mut checker = Checker::new(Limits {
        max_work_units: Some(0),
        ..Limits::default()
    });
    assert_eq!(
        checker.analyze("x").stopped.unwrap().reason,
        Reason::Inconclusive
    );
    let malformed = analyze("([)]");
    assert_eq!(malformed.stopped.unwrap().reason, Reason::LexicalStructure);
}

#[test]
fn checking_and_analysis_agree_at_score_boundaries() {
    let sources = [
        "a + b + c",
        "---x",
        "[[x]]",
        "x: \"list['int']\"",
        "f(a,b,c)",
        "def f(a,b): pass",
        "if x:\n if y:\n  pass",
        "f'{a}{b}'",
        "x[1,2,3]",
        "[1,2,3]",
    ];
    for source in sources {
        for value in 0..5 {
            for score in 0..12 {
                let mut limits = Limits::default();
                match score {
                    0 => limits.max_tokens = Some(value),
                    1 => limits.max_token_bytes = Some(value),
                    2 => limits.max_nesting_depth = Some(value),
                    3 => limits.max_ast_depth = Some(value),
                    4 => limits.max_delimiter_depth = Some(value),
                    5 => limits.max_indentation_depth = Some(value),
                    6 => limits.max_annotation_depth = Some(value),
                    7 => limits.max_call_arguments = Some(value),
                    8 => limits.max_parameters = Some(value),
                    9 => limits.max_display_items = Some(value),
                    10 => limits.max_slice_items = Some(value),
                    11 => limits.max_interpolation_fields = Some(value),
                    _ => unreachable!(),
                }
                assert_eq!(
                    check(source, &limits).err(),
                    Checker::new(limits).analyze(source).stopped,
                    "{source:?}, score={score}, limit={value}"
                );
            }
        }
    }
}

#[test]
fn malformed_analysis_is_bounded_and_ranges_remain_utf8() {
    let pieces = [
        "x", "π", "🦀", "(", ")", "[", "]", "{", "}", "'", "f'", "t'", "lambda ", ":", ",", "=",
        "\n", "\t", "+", "**", "elif ", "case ", "for ",
    ];
    let mut random = 42u64;
    let mut checker = Checker::default();
    for _ in 0..3000 {
        let mut source = String::new();
        for _ in 0..100 {
            random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
            source.push_str(pieces[(random >> 32) as usize % pieces.len()]);
        }
        let report = checker.analyze(&source);
        if let Some(stop) = report.stopped {
            assert!(stop.range.start <= stop.range.end && stop.range.end <= source.len());
            assert!(
                source.is_char_boundary(stop.range.start)
                    && source.is_char_boundary(stop.range.end)
            );
        }
    }
}

#[test]
fn lexer_score_stops_include_the_exceeding_level() {
    let report = Checker::new(Limits {
        max_delimiter_depth: Some(0),
        ..Limits::default()
    })
    .analyze("[x]");
    assert_eq!(report.scores.delimiter_depth, 1);
    assert_eq!(report.stopped.unwrap().reason, Reason::DelimiterDepth);
    let report = Checker::new(Limits {
        max_indentation_depth: Some(0),
        ..Limits::default()
    })
    .analyze("if x:\n pass");
    assert_eq!(report.scores.indentation_depth, 1);
    assert_eq!(report.stopped.unwrap().reason, Reason::Indentation);
}

#[test]
fn counts_and_source_sizes_can_exceed_former_ceilings() {
    let source = "x\n".repeat(500_001);
    let report = analyze(&source);
    assert!(report.is_complete());
    assert_eq!(report.scores.tokens, 1_000_002);
    let source = format!("#{}", "x".repeat(17 * 1024 * 1024));
    let report = analyze(&source);
    assert!(report.is_complete());
    assert_eq!(report.scores.source_bytes, source.len());
    assert_eq!(report.scores.tokens, 0);
}

#[test]
fn escaped_forward_annotation_matches_its_unescaped_value() {
    // CPython's test_typing.test_double_forward: each annotation level decodes
    // its own literal once; an ordinary string remains a borrowed source slice.
    let escaped = r#"def foo(a: 'List[\'int\']'): pass"#;
    let plain = r#"def foo(a: "List['int']"): pass"#;
    let actual = analyze(escaped);
    let expected = analyze(plain);
    assert!(actual.is_complete(), "{actual:?}");
    assert_eq!(actual.scores.annotation_depth, 2);
    assert_eq!(actual.scores.tokens, expected.scores.tokens);
    assert_eq!(actual.scores.parser_depth, expected.scores.parser_depth);
    assert_eq!(actual.scores.ast_depth, expected.scores.ast_depth);
    assert_eq!(actual.scores.slice_items, expected.scores.slice_items);
    assert!(check(escaped, &Limits::screening()).is_ok());
    assert!(analyze(r#"x: r"List['int']""#).is_complete());
    assert_eq!(
        analyze(r#"x: r"List[\'int\']""#).stopped.unwrap().reason,
        Reason::LexicalStructure
    );
}

#[test]
fn decoded_structure_and_decoding_work_obey_limits() {
    let source = r#"x: "\x5bint\x5d""#;
    let report = analyze(source);
    assert!(report.is_complete(), "{report:?}");
    assert_eq!(report.scores.parser_depth, 1);
    assert_eq!(
        report.scores.scanned_bytes,
        source.len() + r"\x5bint\x5d".len() + "[int]".len()
    );
    let mut checker = Checker::new(Limits {
        max_work_units: Some(report.scores.work_units - 1),
        ..Limits::default()
    });
    assert_eq!(
        checker.analyze(source).stopped.unwrap().reason,
        Reason::Inconclusive
    );
    let source = format!(r#"x: "{}int{}""#, r"\x5b".repeat(101), r"\x5d".repeat(101));
    let mut checker = Checker::new(Limits::screening());
    let rejection = checker.check(&source).unwrap_err();
    assert_eq!(rejection.reason, Reason::QuotedAnnotation);
    assert_eq!(rejection.range, 3..source.len());
}

#[test]
fn malformed_annotations_and_unsupported_escapes_remain_incomplete() {
    assert_eq!(
        analyze(r#"def foo(a: "Node[T"): pass"#)
            .stopped
            .unwrap()
            .reason,
        Reason::LexicalStructure
    );
    for source in [r#"x: "\N{LEFT SQUARE BRACKET}int]""#, r#"x: "\ud800""#] {
        assert_eq!(
            analyze(source).stopped.unwrap().reason,
            Reason::AnnotationEscape
        );
    }
    assert_eq!(
        analyze(r#"x: Literal['\N{LATIN CAPITAL LETTER A}']"#)
            .scores
            .annotation_depth,
        0
    );
}

#[test]
fn nested_decoding_restores_parent_text_and_original_error_ranges() {
    fn quote(value: &str) -> String {
        format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
    }
    let mut value = "int".to_owned();
    for _ in 0..5 {
        value = format!("list[{}]", quote(&value));
    }
    let source = format!("x: {}\ny: 'str'", quote(&value));
    let expected = analyze(&source);
    assert!(expected.is_complete(), "{expected:?}");
    assert_eq!(expected.scores.annotation_depth, 6);
    let mut fixed =
        Checker::with_storage(Limits::default(), Storage::Fixed { capacity: 4096 }).unwrap();
    assert_eq!(fixed.analyze(&source), expected);
    let limits = Limits {
        max_nesting_depth: Some(2),
        ..Limits::default()
    };
    let error = Checker::new(limits).analyze(&source).stopped.unwrap();
    assert_eq!(error.reason, Reason::QuotedAnnotation);
    assert_eq!(error.range, 3..source.find('\n').unwrap());
    assert!(fixed.analyze("x = 1").is_complete());
}
