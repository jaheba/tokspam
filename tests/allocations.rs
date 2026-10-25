use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use tokspam::{Checker, Limits, Rejection, Storage, check};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Allocations {
    calls: usize,
    bytes: usize,
    reallocations: usize,
}

thread_local! {
    // Count only this test's calls, excluding the test harness and other threads.
    static COUNTS: Cell<Option<Allocations>> = const { Cell::new(None) };
    static FAIL_AFTER: Cell<Option<usize>> = const { Cell::new(None) };
}

fn fail_allocation() -> bool {
    FAIL_AFTER
        .try_with(|remaining| match remaining.get() {
            Some(0) => true,
            Some(count) => {
                remaining.set(Some(count - 1));
                false
            }
            None => false,
        })
        .unwrap_or(false)
}

struct CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if fail_allocation() {
            return std::ptr::null_mut();
        }
        let _ = COUNTS.try_with(|counts| {
            if let Some(mut value) = counts.get() {
                value.calls += 1;
                value.bytes += layout.size();
                counts.set(Some(value));
            }
        });

        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if fail_allocation() {
            return std::ptr::null_mut();
        }
        let _ = COUNTS.try_with(|counts| {
            if let Some(mut value) = counts.get() {
                value.reallocations += 1;
                counts.set(Some(value));
            }
        });

        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn allocations(run: impl FnOnce() -> Result<(), Rejection>) -> Allocations {
    COUNTS.with(|counts| counts.set(Some(Allocations::default())));
    let result = run();
    let counts = COUNTS.with(|counts| counts.replace(None).unwrap());
    std::hint::black_box(result).ok();
    counts
}

#[test]
fn source_cannot_change_allocation_count_or_size() {
    let sources = [
        String::new(),
        format!("x = \"{}🦀\"", "literal ".repeat(1000)),
        format!("x = f\"{}{{value}}\"", "literal ".repeat(1000)),
        format!("x = {}", vec!["f(x)[0].value * a.b"; 24].join(" + ")),
        format!("x = (a + b + c){}", ".x".repeat(101)),
        format!("x: Annotated[int, Doc(\"{}([)]\")]", "word ".repeat(40)),
        format!("x: \"Literal['{}([)]']\"", "word ".repeat(40)),
        format!(
            "x: Annotated[tuple[Literal['data'], '{}int{}'], 'metadata']",
            "list[".repeat(101),
            "]".repeat(101)
        ),
        format!("x: '{}'\n", "word ".repeat(30)).repeat(100),
        format!("x: \"list['{}']\"", "word ".repeat(30)),
        "x = 1\n".repeat(1_000),
        format!("{}1{}", "[".repeat(101), "]".repeat(101)),
        format!("{}1", "1 | 1 ^ 1 & 1 << 1 + 1 * ".repeat(200)),
        format!(
            "{}pass",
            (0..102)
                .map(|i| format!("{}if x:\n", " ".repeat(i)))
                .collect::<String>()
        ),
        "x: \"list['int']\"\n".repeat(1_000),
        format!("x: \"list['{}int{}']\"", "[".repeat(101), "]".repeat(101)),
        format!("f'{}1{}'", "{x:".repeat(101), "}".repeat(101)),
        "([)]\n'broken\n🦀".to_owned(),
        "π = 1\n$".to_owned(),
        "x = f'{?}'".to_owned(),
        "x = \0".to_owned(),
        format!(
            "if x:\n y: tuple[int, \"list['{}']\"]\n z = [1, 2]",
            "word ".repeat(30)
        ),
        format!("x: tuple[int, \"list['{}']\"]", "([)]".repeat(30)),
    ];

    for limits in [
        Limits::screening(),
        Limits {
            max_nesting_depth: Some(0),
            max_indentation_depth: Some(0),
            max_ast_depth: Some(0),
            ..Limits::screening()
        },
        Limits {
            max_nesting_depth: Some(2),
            max_ast_depth: Some(1),
            ..Limits::screening()
        },
        Limits {
            max_nesting_depth: Some(1),
            max_ast_depth: Some(200),
            ..Limits::screening()
        },
        Limits {
            max_ast_depth: None,
            check_string_annotations: false,
            ..Limits::screening()
        },
    ] {
        let expected = allocations(|| {
            Checker::with_storage(limits.clone(), Storage::Fixed { capacity: 512 })
                .unwrap()
                .check("")
        });
        assert_eq!(expected.reallocations, 0);

        for source in &sources {
            assert_eq!(
                allocations(|| Checker::with_storage(
                    limits.clone(),
                    Storage::Fixed { capacity: 512 }
                )
                .unwrap()
                .check(source)),
                expected,
                "{source:?}"
            );
        }

        let mut checker = Checker::with_storage(limits.clone(), Storage::Fixed { capacity: 512 })
            .expect("valid limits");

        // Include the first call and reuse after failures: neither may allocate
        // or retain lexer/scanner state from an earlier source.
        for source in sources.iter().cycle().take(sources.len() * 3) {
            assert_eq!(
                allocations(|| checker.check(source)),
                Allocations::default(),
                "{source:?}",
            );
            assert_eq!(checker.check(source), check(source, &limits));
            #[cfg(feature = "regions")]
            {
                assert_eq!(
                    allocations(|| checker.check_regions(source)),
                    Allocations::default()
                );
                assert_eq!(checker.check_regions(source), checker.check(source));
            }
        }
    }
}

#[test]
fn hard_depth_boundaries_do_not_grow_scratch() {
    let nesting = 1024usize;
    let limits = Limits {
        max_nesting_depth: Some(1024usize),
        max_indentation_depth: Some(1024usize),
        max_annotation_depth: Some(32usize),
        max_ast_depth: None,
        ..Limits::screening()
    };
    let accepted = format!("{}1{}", "[".repeat(nesting), "]".repeat(nesting));
    let rejected = format!("[{accepted}]");
    // Two suspended scans retain a delimiter each; the inner scan consumes
    // exactly the remaining shared budget, then one additional level rejects.
    let annotation = format!(
        "x: tuple[\"list['{}int{}']\"]",
        "[".repeat(nesting - 2),
        "]".repeat(nesting - 2)
    );
    let annotation_rejected = format!(
        "x: tuple[\"list['{}int{}']\"]",
        "[".repeat(nesting - 1),
        "]".repeat(nesting - 1)
    );
    let indented = format!(
        "{}pass",
        (0..nesting)
            .map(|i| format!("{}if x:\n", " ".repeat(i)))
            .collect::<String>()
            + &" ".repeat(nesting)
    );
    let mut growing = Checker::new(limits.clone());
    let mut checker = Checker::with_storage(limits, Storage::Fixed { capacity: 2048 }).unwrap();

    for _ in 0..2 {
        for source in [&accepted, &annotation, &indented] {
            assert_eq!(growing.check(source), checker.check(source));
            assert_eq!(
                allocations(|| checker.check(source)),
                Allocations::default()
            );
            assert!(checker.check(source).is_ok());
        }
        for source in [&rejected, &annotation_rejected] {
            assert_eq!(growing.check(source), checker.check(source));
            assert_eq!(
                allocations(|| checker.check(source)),
                Allocations::default()
            );
            assert!(checker.check(source).is_err());
        }
    }
}

#[test]
fn growing_buffers_start_small_and_retain_capacity() {
    let limits = Limits {
        max_source_bytes: Some(4096),
        ..Limits::screening()
    };
    let mut lazy = None;
    assert_eq!(
        allocations(|| {
            lazy = Some(Checker::new(limits.clone()));
            Ok(())
        }),
        Allocations::default()
    );
    let lazy = lazy.as_mut().unwrap();

    let oversized = " ".repeat(limits.max_source_bytes.unwrap() + 1);
    assert_eq!(
        allocations(|| lazy.check(&oversized)),
        Allocations::default()
    );
    assert_eq!(
        lazy.check(&oversized).unwrap_err().reason,
        tokspam::Reason::SourceSize
    );

    let upfront = allocations(|| {
        std::hint::black_box(
            Checker::with_storage(limits.clone(), Storage::Fixed { capacity: 512 }).unwrap(),
        );
        Ok(())
    });
    assert!(upfront.calls > 0);
    assert_eq!(upfront.reallocations, 0);
    let small = allocations(|| lazy.check(""));
    assert!(small.bytes < upfront.bytes);
    assert_eq!(allocations(|| lazy.check("")), Allocations::default());

    let deep = format!("{}0{}", "[".repeat(80), "]".repeat(80));
    let annotation = format!("x: \"list['{}int{}']\"", "[".repeat(101), "]".repeat(101));
    let mut eager =
        Checker::with_storage(limits.clone(), Storage::Fixed { capacity: 512 }).unwrap();
    let growth = allocations(|| lazy.check(&deep));
    assert!(growth.calls + growth.reallocations > 0);
    // Once a shape has been seen, clearing lengths preserves its capacity.
    for source in [deep.as_str(), "([)]", annotation.as_str(), "x = 1\n"] {
        assert_eq!(lazy.check(source), eager.check(source));
        assert_eq!(allocations(|| lazy.check(source)), Allocations::default());
        assert_eq!(lazy.check(source), eager.check(source));
        #[cfg(feature = "regions")]
        {
            assert_eq!(
                allocations(|| lazy.check_regions(source)),
                Allocations::default()
            );
            assert_eq!(lazy.check_regions(source), eager.check_regions(source));
        }
    }
}

#[test]
fn fixed_analysis_never_allocates_including_partial_results() {
    let mut checker =
        Checker::with_storage(Limits::default(), Storage::Fixed { capacity: 32 }).unwrap();
    for source in [
        "x = f(1, 2)",
        "x: \"list['int']\"",
        "([)]",
        &"[".repeat(100),
    ] {
        assert_eq!(
            allocations(|| {
                std::hint::black_box(checker.analyze(source));
                Ok(())
            }),
            Allocations::default()
        );
    }
}

#[test]
fn allocation_failure_stops_measurement_and_allows_reuse() {
    let source = format!("x: \"list['{}int{}']\"", "[".repeat(100), "]".repeat(100));
    for successful_allocations in 0..20 {
        let mut checker = Checker::default();
        FAIL_AFTER.with(|remaining| remaining.set(Some(successful_allocations)));
        let report = checker.analyze(&source);
        FAIL_AFTER.with(|remaining| remaining.set(None));
        assert_eq!(report.stopped.unwrap().reason, tokspam::Reason::Storage);
        assert!(checker.analyze(&source).is_complete());
    }
}

#[test]
fn annotation_decoding_respects_fixed_storage_and_retains_growing_capacity() {
    let source = r#"x: 'List[\'int\']'"#;
    let mut fixed =
        Checker::with_storage(Limits::default(), Storage::Fixed { capacity: 64 }).unwrap();
    let expected = tokspam::analyze(source);
    assert!(expected.is_complete());
    for _ in 0..3 {
        assert_eq!(
            allocations(|| {
                assert_eq!(fixed.analyze(source), expected);
                Ok(())
            }),
            Allocations::default()
        );
    }
    let mut small =
        Checker::with_storage(Limits::default(), Storage::Fixed { capacity: 8 }).unwrap();
    assert_eq!(
        allocations(|| {
            assert_eq!(
                small.analyze(source).stopped.unwrap().reason,
                tokspam::Reason::Storage
            );
            assert!(small.analyze("x: 'int'").is_complete());
            Ok(())
        }),
        Allocations::default()
    );
    let mut growing = Checker::default();
    assert!(growing.analyze(source).is_complete());
    assert_eq!(
        allocations(|| {
            assert_eq!(growing.analyze(source), expected);
            Ok(())
        }),
        Allocations::default()
    );
}

#[test]
fn annotation_decode_allocation_failures_are_incomplete_and_reusable() {
    let source = format!(r#"x: "{}int{}""#, r"\x5b".repeat(100), r"\x5d".repeat(100));
    for successful_allocations in 0..25 {
        let mut checker = Checker::default();
        FAIL_AFTER.with(|remaining| remaining.set(Some(successful_allocations)));
        let report = checker.analyze(&source);
        FAIL_AFTER.with(|remaining| remaining.set(None));
        assert_eq!(report.stopped.unwrap().reason, tokspam::Reason::Storage);
        assert!(checker.analyze(&source).is_complete());
    }
}
