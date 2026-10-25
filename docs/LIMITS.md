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
| `ast_depth` | `max_ast_depth` | Estimated tree height, including retained binary and postfix subtrees. |
| `annotation_depth` | `max_annotation_depth` | Number of nested quoted-type rescans, excluding the outer source. |
| `call_arguments` | `max_call_arguments` | Maximum immediate arguments of a lexically recognized call. |
| `parameters` | `max_parameters` | Maximum immediate parameters of a function or lambda. Bare `/` and `*` markers are excluded. |
| `display_items` | `max_display_items` | Maximum immediate list/set/dict or tuple-display items. Comprehension clauses are excluded. |
| `slice_items` | `max_slice_items` | Maximum comma-separated subscript items. |
| `interpolation_fields` | `max_interpolation_fields` | Maximum replacement fields at one f/t-string or format-spec level, carrying counts across adjacent strings. |
| `scanned_bytes` | — | Original bytes plus each quoted-type rescan and raw escape-decoding pass. |
| `work_units` | `max_work_units` | Cumulative scanned bytes plus emitted tokens; a resource budget, not a structural violation. |

Depths count structural additions to a leaf, so an isolated name has AST depth
zero and `a + b + c` has estimated depth two. Independent sibling heights combine
by maximum, not sum. While an annotation child is scanned, parser/AST measurements
include the inherited parent contribution. Lexical and indentation depths describe
the active scan separately.

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
