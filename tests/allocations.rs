use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use tokspam::{Checker, Limits, Rejection, check};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Allocations {
    calls: usize,
    bytes: usize,
    reallocations: usize,
}

thread_local! {
    // Count only this test's calls, excluding the test harness and other threads.
    static COUNTS: Cell<Option<Allocations>> = const { Cell::new(None) };
}

struct CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
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
        "lambda ".repeat(usize::from(u16::MAX) + 1),
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
        Limits::default(),
        Limits {
            max_nesting_depth: 0,
            max_indentation_depth: 0,
            max_ast_depth: Some(0),
            ..Limits::default()
        },
        Limits {
            max_nesting_depth: 2,
            max_ast_depth: Some(1),
            ..Limits::default()
        },
        Limits {
            max_nesting_depth: 1,
            max_ast_depth: Some(200),
            ..Limits::default()
        },
        Limits {
            max_ast_depth: None,
            check_string_annotations: false,
            ..Limits::default()
        },
    ] {
        let expected = allocations(|| check("", &limits));
        assert_eq!(expected.reallocations, 0);

        for source in &sources {
            assert_eq!(
                allocations(|| check(source, &limits)),
                expected,
                "{source:?}"
            );
        }

        let mut checker = Checker::preallocated(limits.clone()).expect("valid limits");

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
fn annotation_depth_shares_bounded_backing_buffers() {
    println!("Checker inline bytes: {}", std::mem::size_of::<Checker>());
    println!(
        "Default scratch: {:?}",
        allocations(|| check("", &Limits::default()))
    );

    for (name, nesting, indentation) in [
        ("default", 100, 80),
        (
            "maximum",
            tokspam::MAX_NESTING_DEPTH,
            tokspam::MAX_INDENTATION_DEPTH,
        ),
    ] {
        let limits = Limits {
            max_nesting_depth: nesting,
            max_indentation_depth: indentation,
            check_string_annotations: false,
            ..Limits::default()
        };
        let outer_only = allocations(|| {
            std::hint::black_box(Checker::preallocated(limits.clone()).unwrap());
            Ok(())
        });
        let nested = allocations(|| {
            std::hint::black_box(
                Checker::preallocated(Limits {
                    check_string_annotations: true,
                    max_annotation_depth: tokspam::MAX_ANNOTATION_DEPTH,
                    ..limits.clone()
                })
                .unwrap(),
            );
            Ok(())
        });

        println!("{name}: outer-only {outer_only:?}; 32 annotation levels {nested:?}");
        assert_eq!(outer_only.calls, nested.calls);
        assert_eq!(nested.reallocations, 0);
        assert!(nested.bytes < outer_only.bytes * 3);
    }
}

#[test]
fn hard_depth_boundaries_do_not_grow_scratch() {
    let nesting = usize::from(tokspam::MAX_NESTING_DEPTH);
    let limits = Limits {
        max_nesting_depth: tokspam::MAX_NESTING_DEPTH,
        max_indentation_depth: tokspam::MAX_INDENTATION_DEPTH,
        max_annotation_depth: tokspam::MAX_ANNOTATION_DEPTH,
        max_ast_depth: None,
        ..Limits::default()
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
    let mut checker = Checker::preallocated(limits).unwrap();

    for _ in 0..2 {
        for source in [&accepted, &annotation, &indented] {
            assert_eq!(
                allocations(|| checker.check(source)),
                Allocations::default()
            );
            assert!(checker.check(source).is_ok());
        }
        for source in [&rejected, &annotation_rejected] {
            assert_eq!(
                allocations(|| checker.check(source)),
                Allocations::default()
            );
            assert!(checker.check(source).is_err());
        }
    }
}

#[test]
fn lazy_initialization_matches_preallocation_and_retains_capacity() {
    let limits = Limits {
        max_source_bytes: 4096,
        ..Limits::default()
    };
    let mut lazy = None;
    assert_eq!(
        allocations(|| {
            lazy = Some(Checker::new(limits.clone()).unwrap());
            Ok(())
        }),
        Allocations::default()
    );
    let lazy = lazy.as_mut().unwrap();

    let oversized = " ".repeat(limits.max_source_bytes + 1);
    assert_eq!(
        allocations(|| lazy.check(&oversized)),
        Allocations::default()
    );
    assert_eq!(
        lazy.check(&oversized).unwrap_err().reason,
        tokspam::Reason::SourceSize
    );

    let upfront = allocations(|| {
        std::hint::black_box(Checker::preallocated(limits.clone()).unwrap());
        Ok(())
    });
    assert!(upfront.calls > 0);
    assert_eq!(upfront.reallocations, 0);
    assert_eq!(allocations(|| lazy.check("")), upfront);

    let deep = format!("{}0{}", "[".repeat(80), "]".repeat(80));
    let annotation = format!("x: \"list['{}int{}']\"", "[".repeat(101), "]".repeat(101));
    let mut eager = Checker::preallocated(limits.clone()).unwrap();
    for source in [deep.as_str(), "([)]", annotation.as_str(), "x = 1\n"] {
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
