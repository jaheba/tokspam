use tokspam::{Limits, Reason, analyze, check};

#[test]
fn boolean_operands_count_per_expression() {
    let limits = Limits {
        max_boolean_operands: Some(3),
        ..Limits::default()
    };
    assert!(check("x = a or b or c", &limits).is_ok());
    assert_eq!(
        check("x = a or b or c or d", &limits).unwrap_err().reason,
        Reason::BooleanOperands
    );
    assert_eq!(analyze("x = a or b or c or d").scores.boolean_operands, 4);
    assert_eq!(
        analyze("x = (a or b) and (c or d)").scores.boolean_operands,
        2
    );
    assert_eq!(analyze("x = a or b; y = c or d").scores.boolean_operands, 2);
    assert_eq!(analyze("x = a or b, c or d").scores.boolean_operands, 2);
    assert_eq!(analyze("x = 'a or b or c or d'").scores.boolean_operands, 0);
}

#[test]
fn with_items_count_only_header_separators() {
    let limits = Limits {
        max_with_items: Some(2),
        ..Limits::default()
    };
    assert!(check("with f(a, b), g(c, d): pass", &limits).is_ok());
    assert_eq!(
        check("with a, b, c: pass", &limits).unwrap_err().reason,
        Reason::WithItems
    );
    assert_eq!(
        check("with (a, b, c): pass", &limits).unwrap_err().reason,
        Reason::WithItems
    );
    assert_eq!(
        analyze("with a, b: pass\nwith c, d: pass")
            .scores
            .with_items,
        2
    );
    assert_eq!(analyze("x = 'with a, b, c'").scores.with_items, 0);
}

#[test]
fn comprehension_filters_count_per_generator() {
    let limits = Limits {
        max_comprehension_filters: Some(2),
        ..Limits::default()
    };
    for source in [
        "x = [0 for x in xs if a if b if c]",
        "x = (0 for x in xs if a if b if c)",
        "x = {x: x for x in xs if a if b if c}",
        "x = sum(0 for x in xs if a if b if c)",
    ] {
        assert_eq!(
            check(source, &limits).unwrap_err().reason,
            Reason::ComprehensionFilters
        );
    }
    assert_eq!(
        analyze("x = [x for x in a if p if q for y in b if r]")
            .scores
            .comprehension_filters,
        2
    );
    assert_eq!(
        analyze("x = [a if p else b for a in xs]")
            .scores
            .comprehension_filters,
        0
    );
    assert_eq!(
        analyze("x = 'if a if b if c'").scores.comprehension_filters,
        0
    );
}

#[test]
fn screening_rejects_the_reduced_type_checker_attacks() {
    let limits = Limits::screening();
    let bool_chain = format!("x = {}", vec!["True"; 4_097].join(" or "));
    assert_eq!(
        check(&bool_chain, &limits).unwrap_err().reason,
        Reason::BooleanOperands
    );

    let with_header = format!("with {}: pass", vec!["cm"; 1_025].join(", "));
    assert_eq!(
        check(&with_header, &limits).unwrap_err().reason,
        Reason::WithItems
    );

    let filters = format!("x = [0 for x in (0,){}]", " if x".repeat(8_193));
    assert_eq!(
        check(&filters, &limits).unwrap_err().reason,
        Reason::ComprehensionFilters
    );
}

#[test]
fn zero_width_limits_reach_the_detailed_scan() {
    let boolean = Limits {
        max_boolean_operands: Some(0),
        ..Limits::default()
    };
    assert_eq!(
        check("x = a or b\n", &boolean).unwrap_err().reason,
        Reason::BooleanOperands
    );

    let with_items = Limits {
        max_with_items: Some(0),
        ..Limits::default()
    };
    assert_eq!(
        check("with cm: pass\n", &with_items).unwrap_err().reason,
        Reason::WithItems
    );

    let filters = Limits {
        max_comprehension_filters: Some(0),
        ..Limits::default()
    };
    assert_eq!(
        check("x = [x for x in xs if x]", &filters)
            .unwrap_err()
            .reason,
        Reason::ComprehensionFilters
    );
}
