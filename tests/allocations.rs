use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use spam::{Limits, check};

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

fn allocations(source: &str, limits: &Limits) -> Allocations {
    COUNTS.with(|counts| counts.set(Some(Allocations::default())));
    let result = check(source, limits);
    let counts = COUNTS.with(|counts| counts.replace(None).unwrap());
    std::hint::black_box(result).ok();
    counts
}

#[test]
fn source_cannot_change_allocation_count_or_size() {
    let sources = [
        String::new(),
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
        let expected = allocations("", &limits);
        assert_eq!(expected.reallocations, 0);

        for source in &sources {
            assert_eq!(allocations(source, &limits), expected, "{source:?}");
        }
    }
}
