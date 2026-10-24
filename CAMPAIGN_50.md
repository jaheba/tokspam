# Fifty parser attack probes

This campaign tests source-processing availability only.
It does not execute generated programs or classify runtime behavior as malicious.
Each source is screened by Spam before CPython, Ruff, and Pyrefly process it in disposable processes.

A result is a **success** when Spam accepts the source and a downstream implementation exceeds the applicable 100 ms
source-processing target, reaches the 500 ms discovery timeout, exceeds the campaign memory target, crashes, or reports
another uncontrolled failure.
A **blocked** result means Spam rejects the source before the parser. **No issue** means the sampled accepted source
stayed within the target. **Direction** means the construction exposed unusual scaling or parser disagreement without
yet meeting the success criterion.

The plan below was written before running the probes.

| ID | Hypothesis and generated shape | Result |
| ---: | --- | --- |
| 1 | A long call argument list followed by invalid `if` forces CPython's second syntax-error pass. | **Success.** Spam accepted it; CPython 3.14 parsing took 143 ms. |
| 2 | Repeated literal arguments followed by invalid `if` remove identifier costs from probe 1. | **Success.** Spam accepted it; CPython 3.14 parsing took 128 ms without identifier costs. |
| 3 | A long function parameter list ending in a duplicate `*` creates late parameter recovery. | **Success.** Spam accepted it; CPython 3.14 parsing took 154 ms. |
| 4 | A long function parameter list ending in `/` at an invalid position delays failure. | **No issue.** Spam accepted it; CPython took 8 ms and Pyrefly took 93 ms. |
| 5 | A long type-parameter list ending in an invalid starred entry stresses new grammar recovery. | **Success.** Spam accepted it; Pyrefly grew from 94 ms at 4,000 entries to 710 ms at 24,000. |
| 6 | A long generic base list followed by an invalid keyword delays class-header failure. | **Success.** Spam accepted it; CPython 3.14 parsing took 136 ms and Pyrefly took 194 ms. |
| 7 | A long import-name list ending in `as` retains aliases before failure. | **Success.** Spam accepted it; Pyrefly grew from 115 ms at 1,000 names to a timeout at 24,000. |
| 8 | A long `from ... import` list ending in `as` exercises parenthesized import recovery. | **Success.** Spam accepted it; Pyrefly grew from 97 ms at 1,000 names to 725 ms at 24,000. |
| 9 | A long global-name list ending in a keyword delays simple-statement failure. | **Direction.** Spam accepted it; Pyrefly crossed 100 ms once, but scaling was not confirmed. |
| 10 | A long nonlocal-name list ending in a keyword delays scope-declaration failure. | **Direction.** Spam accepted it; Pyrefly crossed 100 ms once, but scaling was not confirmed. |
| 11 | A long call keyword list repeats one keyword and fails only at the final duplicate. | **No issue.** Parsing succeeded; CPython compilation rejected the duplicate in 55 ms. |
| 12 | A long dictionary display ends with an invalid double-star entry. | **Success.** Spam accepted it; CPython 3.14 parsing took 115 ms. |
| 13 | A long set display ends with an invalid double-star entry. | **Direction.** Spam accepted it; a seven-sample CPython 3.14 median was 102 ms, but 3.15 took 81 ms and nearby measurements were noisy. |
| 14 | A long list display ends with an invalid comprehension clause. | **Success.** Spam accepted it; CPython 3.14 parsing took 110 ms. |
| 15 | A long tuple display ends with a missing expression after a comma. | **Success.** Spam accepted it; CPython 3.14 parsing took 125 ms. |
| 16 | Thousands of semicolon-separated statements end in an invalid compound statement. | **Direction.** Spam accepted it; Pyrefly took 115 ms, but scaling was not confirmed. |
| 17 | Thousands of physical lines end in a missing suite after `if`. | **Direction.** Spam accepted it; Pyrefly took 111 ms, but scaling was not confirmed. |
| 18 | A long `elif` ladder ends in a missing colon. | **Blocked.** Spam rejected AST depth; CPython returned `MemoryError` quickly. |
| 19 | A long `match` body ends in an invalid case pattern. | **Success.** Spam accepted it; Pyrefly grew from 125 ms at 1,000 cases to 889 ms at 4,000 and timed out at 12,000. |
| 20 | A wide mapping pattern ends in an invalid key that invokes Ruff pattern recovery. | **No issue.** Spam accepted it; all sampled implementations stayed below 100 ms. |
| 21 | A wide class pattern ends in an invalid positional pattern after a keyword pattern. | **Direction.** Spam accepted it; Pyrefly took 111 ms, but scaling was not confirmed. |
| 22 | A wide sequence pattern ends with a second starred capture. | **Direction.** Spam accepted it; Pyrefly took 129 ms, while CPython rejected it during compilation in 41 ms. |
| 23 | A long OR pattern ends with a missing right-hand pattern. | **Blocked.** Spam rejected AST depth; downstream parsers failed normally. |
| 24 | A long comparison chain ends with an operator and no operand. | **Direction.** Spam accepted it; Pyrefly took 171 ms, but the parse-only implementations stayed below target. |
| 25 | A long Boolean chain ends with `and` and no operand. | **Direction.** Spam accepted it; Pyrefly took 183 ms, but the parse-only implementations stayed below target. |
| 26 | A long additive chain ends with `+` and no operand. | **Blocked.** Spam rejected AST depth; downstream parsers failed normally. |
| 27 | A long power chain ends with `**` and no operand. | **Blocked.** Spam rejected nesting; CPython returned `MemoryError` quickly. |
| 28 | A long postfix attribute chain ends with a bare dot. | **Blocked.** Spam rejected AST depth; downstream parsers failed normally. |
| 29 | A long subscript slice list ends with an incomplete slice. | **Success.** Spam accepted it; CPython parse, traversal, compilation, and disposal took 160–164 ms. |
| 30 | A long conditional-expression chain omits the final `else` expression. | **Blocked.** Spam rejected nesting; CPython returned `MemoryError` quickly. |
| 31 | A long lambda-parameter list omits the body after the colon. | **Success.** Spam accepted it; CPython parsing took 153 ms on 3.14 and 145 ms on 3.15. |
| 32 | Many comprehension filters end with an incomplete filter expression. | **Direction.** Spam accepted it; Pyrefly measured 100 ms once, too close to call without repetition. |
| 33 | Many comprehension generators end with an incomplete final iterable. | **Direction.** Spam accepted it; Pyrefly took 142 ms, but scaling was not confirmed. |
| 34 | Thousands of adjacent short strings stress concatenation and constant folding. | **No issue.** Spam accepted it; adjacent-string folding completed in about 6 ms in CPython. |
| 35 | Thousands of adjacent strings end in an unterminated final literal. | **Blocked.** Spam rejected the unterminated literal as a lexical-structure error. |
| 36 | One large escape-heavy bytes literal stresses decoding and diagnostic construction. | **No issue.** Spam accepted it; all parsers handled the 800 KB literal quickly. |
| 37 | One large invalid Unicode-escape string delays decoding failure. | **No issue.** Spam accepted it; CPython and Ruff returned controlled syntax errors in under 11 ms. |
| 38 | A flat f-string with many replacement fields stresses field parsing and AST allocation. | **Success.** Spam accepted it; Pyrefly scaled to 198 ms. CPython's seven-sample full-pipeline medians were a marginal 100 ms on 3.14 and 98 ms on 3.15. |
| 39 | A flat f-string with many fields leaves the final brace unmatched. | **Blocked.** Spam rejected the unmatched field as a lexical-structure error. |
| 40 | A flat template string with many replacement fields tests Python 3.14's new token states. | **Success.** Spam accepted it; CPython's full pipeline took 103–108 ms and Pyrefly scaled to 198 ms. |
| 41 | Alternating adjacent f-strings and template strings stress transitions between string modes. | **Blocked.** Spam rejected it at 140,001 tokens. |
| 42 | Many unique NFKC-normalized identifiers stress normalization and identifier retention. | **Success.** Spam accepted it; CPython's full pipeline took 181–188 ms. |
| 43 | Many identifiers that normalize to the same spelling stress collision and duplicate handling. | **Success.** Spam accepted it; CPython's full pipeline took 163–175 ms. |
| 44 | A near-limit underscored decimal integer stresses temporary underscore removal and bigint conversion. | **Direction.** Spam accepted it; Pyrefly took 210 ms at 800 KB, but smaller samples stayed near startup cost. |
| 45 | A near-limit hexadecimal integer stresses bigint conversion without CPython's decimal digit cap. | **No issue.** Spam accepted it; the 1 MB integer completed in about 10 ms in CPython and 12 ms in Ruff. |
| 46 | A very long hexadecimal float-like malformed token stresses numeric token recovery. | **No issue.** Spam accepted it; parser and compiler processing completed in about 11–12 ms. |
| 47 | Mixed bracket, lambda, conditional, and power nesting accumulates parser frames across grammar rules. | **Blocked.** Spam rejected nesting before the downstream parsers. |
| 48 | Nested classes containing decorators, generics, annotations, and defaults combine suite and expression frames. | **Blocked.** Spam rejected indentation depth before the downstream parsers. |
| 49 | Deep assignment and deletion targets exercise Ruff's recursive target validation after parsing. | **Blocked.** Spam rejected AST depth before Ruff target validation. |
| 50 | A shallow source producing a large AST is parsed, traversed, compiled, and destroyed as separate CPython phases. | **Blocked.** Spam rejected token count; CPython's full pipeline otherwise took 339 ms. |

## Results

The campaign ran on macOS arm64 against Spam's release probe, Ruff parser 0.0.14, CPython 3.14 and 3.15, and Pyrefly
1.4.0.
Each process had a 500 ms discovery timeout in the broad pass.
Follow-up Pyrefly scaling used a one-second timeout.
Generated programs were never executed.

CPython timings in the table measure work inside `ast.parse`, or the explicitly stated parse, iterative AST traversal,
compilation, and disposal pipeline.
Ruff timings include parsing and AST disposal.
Pyrefly timings cover its CLI check pipeline with one thread, the `off` diagnostic preset, interpreter discovery
disabled, and no progress or error output.
They are not parse-only measurements.
A small Pyrefly input usually took 65–75 ms including process startup.

The 50 probes produced 17 confirmed accepted budget failures: 12 in CPython parsing or its later compiler pipeline and
five only in Pyrefly's broader check pipeline.
Another 12 are directions requiring more measurement, 13 were blocked by Spam, and eight had no observed issue.
Ruff had no crash, timeout, or 100 ms crossing in this sample.
CPython and Pyrefly returned controlled diagnostics for malformed inputs unless a row says otherwise.

The strongest new Pyrefly cases were:

- a malformed import list, which grew from 115 ms at 1,000 names to 265 ms at 4,000, 630 ms at 12,000, and a timeout at
  24,000;
- a malformed parenthesized `from` import, which grew from 97 ms at 1,000 names to 725 ms at 24,000;
- repeated match cases with a final invalid case, which grew from 125 ms at 1,000 cases to 889 ms at 4,000 and timed out
  at 12,000;
- a malformed type-parameter list, which reached 246 ms at 12,000 parameters and 710 ms at 24,000;
- 24,000 f-string or template-string fields, which took about 198 ms after a 69–73 ms small-input baseline.

The Pyrefly results establish expensive accepted check inputs, not parser attribution.
Pyrefly's timing report attributes the 4,000-case match result to later checking work: the run took 0.89 seconds and
reported 799 MiB of physical memory, while the target module's `Ast` step took 0.6 ms, `Answers` took 181 ms, and
`Solutions` took 522 ms.
This is a Pyrefly checker availability finding rather than a parser finding, so it does not by itself justify a Spam
rule.

A bounded memory refinement avoided repeating the 4,000-case allocation:

| Repeated cases | Source | Wall time | Pyrefly physical memory |
| ---: | ---: | ---: | ---: |
| 250 | 3.5 KB | 74 ms | 59 MiB |
| 500 | 7.0 KB | 88 ms | 69 MiB |
| 750 | 10.5 KB | 105 ms | 88 MiB |
| 1,000 | 14.0 KB | 122 ms | 111 MiB |
| 1,500 | 21.0 KB | 188 ms | 174 MiB |
| 2,000 | 28.0 KB | 342 ms | 257 MiB |

The 100-case process used 55 MiB.
Using that as the observed process baseline, incremental memory crosses 64 MiB between 1,000 and 1,500 cases.
Doubling from 500 to 1,000 cases raised incremental memory by roughly four times, which is consistent with quadratic
growth over this small sample but does not prove asymptotic complexity.

The 4,000-name malformed import run took 0.18 seconds and 62 MiB, while the target module's reported `Ast` step took 0.2
ms.
Its remaining cost was outside the target module's timed AST step and needs a lower-level probe before attribution.
The import curves remain useful Pyrefly availability findings, but they are not evidence of parser recursion or parser
complexity.

The CPython findings, and the CPython full-pipeline findings for slices, interpolation fields, and identifiers, remain
directly relevant to a parser preflight.

## CPython threshold refinement

Single measurements around 100 ms varied enough to make element-by-element binary minimization misleading.
The refinement pass therefore used five-sample medians at broad size intervals and seven samples at the original size.
These brackets are local measurements, not portable constants:

| Probe | Phase | Last sampled size below 100 ms | First sampled size above 100 ms |
| ---: | --- | ---: | ---: |
| 1 | parse | 34,300 arguments, 90 ms | 39,200 arguments, 105 ms |
| 2 | parse | 44,100 arguments, 98 ms | 49,000 arguments, 118 ms |
| 3 | parse | 12,000 parameters, 87 ms | 14,000 parameters, 103 ms |
| 6 | parse | 34,300 bases, 84 ms | 39,200 bases, 104 ms |
| 12 | parse | 21,600 entries, 97 ms | 24,000 entries, 110 ms |
| 14 | parse | 36,000 entries, 93 ms | 40,000 entries, 111 ms |
| 15 | parse | 44,100 entries, approximately 100 ms | 49,000 entries, 112 ms |
| 29 | parse, traverse, compile, dispose | 14,400 slices, 93 ms | 16,800 slices, 108 ms |
| 31 | parse | 12,000 parameters, 84 ms | 14,000 parameters, 101 ms |
| 40 | parse, traverse, compile, dispose | 21,600 fields, 94 ms | 24,000 fields, 106 ms |
| 42 | parse, traverse, compile, dispose | 12,000 identifiers, 86 ms | 16,800 identifiers, 122 ms |
| 43 | parse, traverse, compile, dispose | 14,400 identifiers, approximately 100 ms | 16,800 identifiers, 112 ms |

All bracket endpoints were accepted by Spam.
CPython 3.15 broadly confirmed the original-size results.
Probe 13 was downgraded because its seven-sample medians were 102 ms on CPython 3.14 and 81 ms on CPython 3.15.
Probe 38 remains a Pyrefly pipeline success, while its CPython medians sit on either side of the target.

Raw observations are stored beside this ledger in `campaigns/2026-10-24-50-ideas/`.
Eight CPython boundary/control pairs and one Pyrefly memory pair are retained in
`campaigns/2026-10-24-resource-boundaries/` and imported into `corpus/cases.sqlite` with generator recipes and
phase-specific resource observations.
