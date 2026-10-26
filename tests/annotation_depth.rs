use tokspam::{Checker, Limits, Reason, Storage, analyze, check};

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

#[test]
fn each_quoted_type_adds_an_ast_layer() {
    let mut value = "T".to_owned();
    for depth in 1..=8 {
        value = quote(&value);
        let source = format!("x: {value}");
        let report = analyze(&source);
        assert!(report.is_complete(), "{report:?}");
        assert_eq!(report.scores.ast_depth, depth);
        assert_eq!(report.scores.annotation_depth, depth);
        for limit in [depth - 1, depth] {
            let limits = Limits {
                max_ast_depth: Some(limit),
                ..Limits::default()
            };
            let expected = (limit < depth).then_some(Reason::QuotedAnnotation);
            assert_eq!(check(&source, &limits).err().map(|e| e.reason), expected);
            assert_eq!(
                Checker::new(limits)
                    .analyze(&source)
                    .stopped
                    .map(|e| e.reason),
                expected
            );
        }
    }
}

#[test]
fn quoted_union_spends_its_wrapper_budget_before_operators() {
    let source = format!("x: '{}'", "T|".repeat(199) + "T");
    assert_eq!(analyze(&source).scores.ast_depth, 200);
    let limits = Limits {
        max_ast_depth: Some(199),
        ..Limits::default()
    };
    assert_eq!(
        check(&source, &limits).unwrap_err().reason,
        Reason::QuotedAnnotation
    );
    assert!(
        check(
            &source,
            &Limits {
                max_ast_depth: Some(200),
                ..limits
            }
        )
        .is_ok()
    );
}

#[test]
fn sibling_annotation_depths_combine_by_maximum_and_restore_parent() {
    let source = r#"x: tuple['T', 'U', 'V']"#;
    let depth = analyze(source).scores.ast_depth;
    let limits = Limits {
        max_ast_depth: Some(depth),
        ..Limits::default()
    };
    for storage in [Storage::Growing, Storage::Fixed { capacity: 256 }] {
        let mut checker = Checker::with_storage(limits.clone(), storage).unwrap();
        for _ in 0..3 {
            assert!(checker.check(source).is_ok());
            assert_eq!(checker.analyze(source).scores.ast_depth, depth);
            assert!(checker.check("x: T").is_ok());
        }
    }
}

#[test]
fn empty_annotation_checks_and_analysis_agree_at_zero_budget() {
    let source = "x: ''";
    let limits = Limits {
        max_ast_depth: Some(0),
        ..Limits::default()
    };
    assert_eq!(
        check(source, &limits).unwrap_err().reason,
        Reason::QuotedAnnotation
    );
    assert_eq!(
        Checker::new(limits).analyze(source).stopped.unwrap().reason,
        Reason::QuotedAnnotation
    );
}
