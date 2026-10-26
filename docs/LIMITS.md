# Scores and optional thresholds

All scores are `usize`. All numeric `Limits` fields are `Option<usize>`; no
threshold is enabled by default. A threshold is exceeded only when its score is
strictly greater than the configured value.

| Score | Threshold | Meaning |
| --- | --- | --- |
| `source_bytes` | `max_source_bytes` | Original UTF-8 source length, known before scanning. |
| `tokens` | `max_tokens` | Cumulative emitted tokens, including annotation rescans and layout tokens. |
| `max_token_bytes` | `max_token_bytes` | Largest emitted token range in bytes. |
| `delimiter_depth` | `max_delimiter_depth` | Maximum active lexical context depth: brackets, string modes, and replacement fields; excludes each scan's root. |
| `indentation_depth` | `max_indentation_depth` | Maximum active indentation levels, not indentation columns. |
| `parser_depth` | `max_nesting_depth` | Estimated retained parser recursion from brackets, suites, operators, branches, and format fields. |
| `ast_depth` | `max_ast_depth` | Conservative structural estimate of tree height, including retained binary and postfix subtrees. |
| `annotation_depth` | `max_annotation_depth` | Number of nested quoted-type rescans, excluding the outer source. |
| `call_arguments` | `max_call_arguments` | Maximum immediate arguments of a lexically recognized call. |
| `parameters` | `max_parameters` | Maximum immediate parameters of a function or lambda. Bare `/` and `*` markers are excluded. |
| `display_items` | `max_display_items` | Maximum immediate list/set/dict or tuple-display items. Comprehension clauses are excluded. |
| `slice_items` | `max_slice_items` | Maximum comma-separated subscript items. |
| `interpolation_fields` | `max_interpolation_fields` | Maximum replacement fields at one f/t-string or format-spec level, carrying counts across adjacent strings. |
| `boolean_operands` | `max_boolean_operands` | Maximum operands in one lexical `and`/`or` chain. Commas, statements, and nested delimiters split chains. |
| `with_items` | `max_with_items` | Maximum comma-separated items in one `with` header, including parenthesized headers. |
| `comprehension_filters` | `max_comprehension_filters` | Maximum `if` clauses after one `for` clause in a comprehension. |
| `scanned_bytes` | — | Original bytes plus each quoted-type rescan and raw escape-decoding pass. |
| `work_units` | `max_work_units` | Cumulative scanned bytes plus emitted tokens; a resource budget, not a structural violation. |

Depths count structural additions to a leaf, so an isolated name has AST depth
zero and `a + b + c` has estimated depth two. Independent sibling heights combine
by maximum, not sum. While an annotation child is scanned, parser/AST measurements
include the inherited parent contribution. Each quoted-type rescan adds one AST
level for interpreting the string as another expression. Lexical and indentation depths describe
the active scan separately.

AST depth is a conservative structural estimate, not an exact node count. Grouping,
block nesting, and call structure can contribute more than they do to a parser's
resulting tree. Different parsers also construct different trees and recover from
syntax errors differently. The estimate is not a guaranteed upper bound for every
parser.

AST depth also charges operand-less `if` tokens inside expressions, conservatively
modeling nested conditionals a parser may retain during error recovery. This does
not validate syntax or reproduce a particular parser's complete recovery behavior.
Lexically invalid quoted annotation text ends its child scan without rejecting the
outer string; attempted work and observed prefix scores remain counted.

Quoted-type screening is limited to positions recognized from source tokens. A
type checker can use name resolution and binding analysis to interpret strings
elsewhere as types, such as `type A = "list[int]"` or
`from typing import cast; foo = cast; foo("list[int]", x)`.
Tokspam does not follow those bindings or scan arbitrary strings, because ordinary
data strings can contain Python-looking text. Its scores do not bound the work a
downstream type checker may do after resolving such a string.

`Limits::screening()` enables Boolean, `with`, and comprehension widths at 4096,
1024, and 8192 respectively. These thresholds were chosen below resource
failures observed in one type-checker build. Widths are lexical estimates:
malformed syntax and parenthesized `with` headers can be overcounted. Keep a
separate memory limit and deadline around downstream parsing and type checking.

Width recognition is lexical, without name resolution or a full grammar. Counts
are finalized by separators and closing delimiters. On malformed or unfinished
input, an incomplete report can omit a pending final item. A token budget may stop
before that token reaches the structural tracker. Partial scores therefore should
not be treated as complete measurements.

`scanned_bytes` charges the full outer input once and charges a type-string
interior when scheduled. Escaped literals additionally charge the raw decoding
pass, then the decoded source separately. It measures budgeted work, not the
precise cursor position at interruption. Repeated nested interpretation consumes shared work, not a fresh
per-child allowance.

No universal safe threshold exists for all Python parsers. Select limits using
your parser version, platform, and workload, and retain an independent deadline
where availability matters.
