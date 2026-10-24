//! Grammar-directed source generation shared by the campaign and libFuzzer.

pub const FAMILIES: &[&str] = &[
    "unary",
    "power",
    "lambda",
    "conditional",
    "brackets",
    "indentation",
    "format-spec",
    "calls",
    "subscripts",
    "attributes",
    "binary",
    "bool-or",
    "compare",
    "assignment-chain",
    "comprehension",
    "dict-unpack",
    "slice-chain",
    "match-or",
    "class-pattern",
    "mapping-pattern",
    "nested-class",
    "decorators",
    "with-items",
    "except-tuple",
    "postfix-mix",
    "fstring-chain",
    "comprehension-chain",
    "binary-mix",
    "string-parens",
    "string-fake-annotation",
    "prose-apostrophe",
    "case-name-add",
    "left-mul",
    "left-xor",
    "nested-target",
    "union",
    "annotation",
    "flat-list",
    "mixed",
];

/// Families are grammatical constructions; parser version and resource limits
/// can still reject their large instances. Depth is capped to bound generation.
pub fn generate(family: &str, depth: usize, mut seed: u64) -> String {
    let depth = depth.min(100_000);
    match family {
        "unary" => format!("x = {}1\n", "-".repeat(depth)),
        "power" => format!("x = {}2\n", "2 ** ".repeat(depth)),
        "lambda" => format!("x = {}1\n", "lambda: ".repeat(depth)),
        "conditional" => format!("x = {}1\n", "1 if 1 else ".repeat(depth)),
        "brackets" => format!("x = {}1{}\n", "(".repeat(depth), ")".repeat(depth)),
        "indentation" => {
            // Indentation grows quadratically in bytes; cap this family separately.
            let depth = depth.min(500);
            let mut source = String::new();
            for i in 0..depth {
                source.push_str(&" ".repeat(i));
                source.push_str("if True:\n");
            }
            source.push_str(&" ".repeat(depth));
            source.push_str("pass\n");
            source
        }
        "format-spec" => format!("x = f'{}{{x}}{}'\n", "{x:".repeat(depth), "}".repeat(depth)),
        "calls" => format!("x = f{}\n", "()".repeat(depth)),
        "subscripts" => format!("x = a{}\n", "[0]".repeat(depth)),
        "attributes" => format!("x = a{}\n", ".x".repeat(depth)),
        "binary" => format!("x = {}1\n", "1 + ".repeat(depth)),
        "bool-or" => format!("x = {}True\n", "True or ".repeat(depth)),
        "compare" => format!("x = {}0\n", "0 < ".repeat(depth)),
        "assignment-chain" => format!("{}x = 1\n", "x = ".repeat(depth)),
        "comprehension" => format!("x = [0 {}]\n", "for _ in (1,) ".repeat(depth)),
        "dict-unpack" => format!("x = {{{}}}\n", "**{}, ".repeat(depth)),
        "slice-chain" => format!("x = a{}\n", "[0:1]".repeat(depth)),
        "match-or" => format!("match x:\n    case {}0: pass\n", "0 | ".repeat(depth)),
        "class-pattern" => format!(
            "match x:\n    case {}0{}: pass\n",
            "C(".repeat(depth),
            ")".repeat(depth)
        ),
        "mapping-pattern" => format!(
            "match x:\n    case {}0{}: pass\n",
            "{'k': ".repeat(depth),
            "}".repeat(depth)
        ),
        "nested-class" => {
            let depth = depth.min(300);
            let mut source = String::new();
            for level in 0..depth {
                source.push_str(&" ".repeat(level));
                source.push_str(&format!("class C{level}:\n"));
            }
            source.push_str(&" ".repeat(depth));
            source.push_str("pass\n");
            source
        }
        "decorators" => format!("{}def f(): pass\n", "@dec\n".repeat(depth)),
        "with-items" => format!("with {}cm: pass\n", "cm, ".repeat(depth)),
        "except-tuple" => format!(
            "try:\n    pass\nexcept ({}Exception):\n    pass\n",
            "Exception, ".repeat(depth)
        ),
        "postfix-mix" => format!("x = a{}\n", "()[0].x".repeat(depth)),
        "fstring-chain" => format!("x = f'{{a{}}}'\n", ".x".repeat(depth)),
        "comprehension-chain" => format!("x = [a{} for _ in (1,)]\n", ".x".repeat(depth)),
        "binary-mix" => format!("x = {}1\n", "1 + 1 * ".repeat(depth)),
        "string-parens" => format!("x = '{}'\n", "(".repeat(depth)),
        "string-fake-annotation" => format!(
            "x = 'list[{}int{}]'\n",
            "list[".repeat(depth),
            "]".repeat(depth)
        ),
        "prose-apostrophe" => format!("x = \"\"\"{}don't\"\"\"\n", "word ".repeat(depth)),
        "case-name-add" => format!("case = 1\nx = case{}\n", " + 1".repeat(depth)),
        "left-mul" => format!("x = {}1\n", "1 * ".repeat(depth)),
        "left-xor" => format!("x = {}1\n", "1 ^ ".repeat(depth)),
        "nested-target" => {
            let depth = depth.min(500);
            format!("{}x{} = 1\n", "(".repeat(depth), ",)".repeat(depth))
        }
        "union" => format!("x: {}int\n", "int | ".repeat(depth)),
        "annotation" => format!("x: '{}int{}'\n", "list[".repeat(depth), "]".repeat(depth)),
        "flat-list" => format!("x = [{}]\n", "-1, ".repeat(depth)),
        "mixed" => {
            let mut opens = String::new();
            let mut closes = Vec::new();
            for _ in 0..depth {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                let (open, close) = match seed >> 61 {
                    0 => ("(", ")"),
                    1 => ("[", "]"),
                    2 => ("f(", ")"),
                    3 => ("-(", ")"),
                    4 => ("(lambda: ", ")"),
                    5 => ("(1 if True else ", ")"),
                    6 => ("{'key': ", "}"),
                    _ => ("a[", "]"),
                };
                opens.push_str(open);
                closes.push(close);
            }
            let mut source = format!("x = {opens}1");
            for close in closes.into_iter().rev() {
                source.push_str(close);
            }
            source.push('\n');
            source
        }
        _ => panic!("unknown source family"),
    }
}
