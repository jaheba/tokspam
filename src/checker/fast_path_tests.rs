//! Compare simple-line proofs against the detailed scan on the same inputs.

use super::*;

#[test]
fn simple_prefix_matches_detailed_scanning() {
    let lines = [
        "x = 1\n",
        "value = left.attr + right * 2\r\n",
        "x = ~~--1 ** 2\n",
        "x != y <= z >> 2 // 3\n",
        "async await yield not in is and or if for\n",
        "case, x, 1.2e3, 0xdead\n",
        "x = 1 if if if if if if\n",
        "x: \"it's\"\n",
        "else x\n",
        "elif x\n",
        "lambda x\n",
        "x = '\n",
        "x: '[[[[int]]]]'\n",
        "x = f'{y:{z}}'\n",
        "(x\n + y)\n",
        "x = 1 + \\\n2\n",
        "π = '🦀'\n",
        "\t\x0cx = 1\n",
        "if x:\n pass\n",
        "# comment\n",
        "x = 1\r",
        "x = 1",
        "",
    ];
    let mut random = 123u64;

    for depth in [0, 1, 4, 30, 100] {
        for tokens in [0, 1, 4, 20, 1000] {
            for token_bytes in [None, Some(0), Some(1), Some(8)] {
                let limits = Limits {
                    max_nesting_depth: Some(depth),
                    max_ast_depth: Some(depth / 2),
                    max_tokens: Some(tokens),
                    max_token_bytes: token_bytes,
                    max_annotation_depth: Some(3),
                    ..Limits::screening()
                };
                let mut checker = Checker::new(limits.clone());

                for _ in 0..100 {
                    let mut source = String::new();
                    for _ in 0..10 {
                        random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                        source.push_str(lines[(random >> 32) as usize % lines.len()]);
                    }
                    let actual = checker.check(&source);
                    let buffers = checker.buffers.as_mut().expect("scan initialized buffers");
                    let expected = scan::<false, false>(
                        &source,
                        &limits,
                        &mut buffers.scratch,
                        &mut buffers.work,
                    );
                    assert_eq!(actual, expected, "{source:?}");
                }
            }
        }
    }
}
