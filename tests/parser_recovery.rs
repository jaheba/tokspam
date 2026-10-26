use tokspam::{Checker, Limits, Reason, Storage, analyze, check};

#[test]
fn invalid_annotation_text_preserves_valid_outer_source() {
    let sources = [
        r#"x: "it's""#,
        r"x: r'raw\d'",
        r#"x: "a" r"b\n""#,
        r#"x: tuple["it's", "list[int]"]"#,
        include_str!("fixtures/monty_class_annotations.py"),
    ];
    for storage in [Storage::Growing, Storage::Fixed { capacity: 256 }] {
        let mut checker = Checker::with_storage(Limits::screening(), storage).unwrap();
        for source in sources {
            assert!(checker.check(source).is_ok(), "{source}");
            assert!(checker.analyze(source).is_complete(), "{source}");
        }
    }
}

#[test]
fn invalid_annotation_only_finishes_its_child_scan() {
    let mut checker = Checker::new(Limits::screening());
    for _ in 0..3 {
        assert!(checker.check(r#"x: "it's""#).is_ok());
        let deep = format!(
            r#"x: tuple["it's", {}int{}]"#,
            "[".repeat(110),
            "]".repeat(110)
        );
        assert_eq!(checker.check(&deep).unwrap_err().reason, Reason::AstDepth);
        assert_eq!(
            checker.check("x: \"it's\"\ny = ([)]").unwrap_err().reason,
            Reason::LexicalStructure
        );
        assert!(checker.check("y: list[int]").is_ok());
    }
}

#[test]
fn invalid_annotation_does_not_suppress_resource_stops() {
    let source = r#"x: "[[[[[[[[it's""#;
    let limits = Limits {
        max_nesting_depth: Some(3),
        ..Limits::default()
    };
    assert_eq!(
        check(source, &limits).unwrap_err().reason,
        Reason::QuotedAnnotation
    );

    let limits = Limits {
        max_work_units: Some(source.len()),
        ..Limits::default()
    };
    assert_eq!(
        check(source, &limits).unwrap_err().reason,
        Reason::Inconclusive
    );

    let mut checker = Checker::with_storage(
        Limits {
            max_annotation_depth: Some(8),
            ..Limits::default()
        },
        Storage::Fixed { capacity: 3 },
    )
    .unwrap();
    assert_eq!(checker.check(source).unwrap_err().reason, Reason::Storage);
}

#[test]
fn recovered_if_chains_consume_ast_budget() {
    let limits = Limits {
        max_ast_depth: Some(200),
        ..Limits::default()
    };
    for storage in [Storage::Growing, Storage::Fixed { capacity: 64 }] {
        let mut checker = Checker::with_storage(limits.clone(), storage).unwrap();
        for suffix in ["", "\n"] {
            let source = format!("x = 1 {}{suffix}", "if ".repeat(300_000));
            assert_eq!(checker.check(&source).unwrap_err().reason, Reason::AstDepth);
            #[cfg(feature = "regions")]
            assert_eq!(
                checker.check_regions(&source).unwrap_err().reason,
                Reason::AstDepth
            );
            assert_eq!(
                checker.analyze(&source).stopped.unwrap().reason,
                Reason::AstDepth
            );
            assert!(checker.check("x = a if b else c").is_ok());
        }
    }
    assert!(analyze("x = 1 if if if if if if").scores.ast_depth > 0);
}

#[test]
fn recovery_depth_is_local_to_each_expression() {
    let limits = Limits {
        max_ast_depth: Some(3),
        ..Limits::default()
    };
    assert!(check(&"x = 1 if if if\n".repeat(1000), &limits).is_ok());
    assert!(check("[x for x in xs if p if q]", &limits).is_ok());
    assert!(check("if x:\n pass\nif y:\n pass\n", &limits).is_ok());
    assert!(check("[x if p else y, a if q else b]", &Limits::screening()).is_ok());
}
