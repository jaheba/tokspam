# Tokspam research outline

Working paper title: **Bounded resource screening for Python source before parsing**

Research question: Can a cheap, bounded preflight screen reduce resource exhaustion in Python parsing and AST
processing while accepting ordinary Python source?

The proposed paper studies Tokspam as a configurable resource guard for applications that process untrusted or
generated Python. Its central hypothesis is that token structure can expose useful signs of excessive parser
recursion and AST depth before constructing an AST. The contribution depends on demonstrating an advantage over
simple input limits, with measured compatibility and cost. Acceptance is not currently a verified guarantee that a
downstream parser can safely process the source.

## Problem and threat model

An adversary controls source submitted to a host's parser entry points, including dynamically generated source passed
to `eval()` or `exec()`. The target is availability during parsing, subsequent AST processing, or destruction.
Relevant source shapes include delimiter and indentation nesting, binary and postfix chains, broad constructs, and
quoted annotations interpreted as source by the downstream pipeline. Malformed input belongs in the threat model.

The host retains time and memory controls and must screen every applicable entry point. Tokspam does not execute
source, validate Python syntax, or restrict runtime capabilities. The evaluation must identify which parser and AST
operations actually consume each source form; quoted annotation contents should only be treated as a downstream
exhaustion case where that pipeline interprets them.

## Proposed contributions

1. **A structural screening design.** A streaming lexer retains source ranges without collecting tokens. Separate
   estimates track parser nesting and AST height, including operator precedence and postfix chains. Likely quoted
   annotations use an explicit work stack and share cumulative scan budgets.
2. **An auditable resource contract.** Score thresholds are optional. Growing scratch is fallible; fixed scratch has
   caller-selected capacity and performs no scanning allocations. Source structure does not increase native
   call-stack depth. Exhausting work fuel produces `Inconclusive`; exhausting scratch produces `Storage`,
   separately from a structural violation.
3. **A reproducible empirical characterization.** Measure failure interception, ordinary-source rejection, and
   overhead across pinned downstream implementations and configurations. Publish retained counterexamples and
   identify the conditions under which simpler checks perform equally well.

The first two contributions describe the current design documented in [configuration](CONFIGURATION.md) and
[safety requirements](SAFETY.md). A manuscript needs an implementation argument and supporting measurements for
the resource contract. The third contribution requires a consolidated evaluation in `tokspam-fuzzer`.

## Evaluation plan

**Failure interception.** Run generated families and retained fuzz cases against each downstream pipeline with and
without Tokspam. Include deep groups, indentation, operator precedence combinations, call/attribute/subscript chains,
wide constructs, f-strings, quoted annotations, malformed Unicode input, and combinations of these forms. Sweep sizes
around guard thresholds and observed downstream failures. Run downstream cases in isolated processes with fixed
deadlines and memory and stack limits. Distinguish clean syntax rejection, explicit resource rejection, timeout,
memory exhaustion, and crash; separate parsing from traversal, annotation interpretation, and destruction where possible.
Report failures among accepted inputs as prominently as intercepted failures.

**Baselines and ablations.** Compare no preflight guard, byte limits, byte plus token limits, delimiter/indentation
limits, and full Tokspam under the same host controls. Disable AST estimation, annotation screening, and optional
width limits separately. Compare tradeoffs across thresholds at similar ordinary-source acceptance rates, rather
than choosing a weak baseline threshold. This tests whether structural accounting contributes beyond input size.

**Compatibility.** Use a versioned corpus of real Python package sources, with generated and stub files reported
separately. Report file and package acceptance, rejected bytes, rejection reasons, and manually classified samples.
Call a rejection a false positive only when the target pipeline can process that input within the declared budget.
Separate calibration and held-out projects; freeze profiles before evaluating the held-out set. Compare default,
package-source, and calibrated structural profiles.

**Cost and bounds.** Measure checker construction separately from reuse, scan throughput, latency distributions,
allocation requests, and end-to-end guard-plus-parser cost. Include benign files, early rejections, near-limit inputs,
and repeated nested annotation scans. Plot scaling as input grows and vary configuration independently. Record
compiler flags, CPU, operating system, allocator, repetitions, and variability. Allocation instrumentation and
constrained-stack runs supplement the code argument; finite tests do not establish universal complexity bounds.

All experiments should pin Tokspam and tooling commits, parser versions, profiles, corpus hashes, seeds, and host
budgets. Preserve raw outcomes and scripts in the separate
[`tokspam-fuzzer`](https://github.com/jaheba/tokspam-fuzzer) project. Candidate targets include the Monty integration
and additional Python parser/AST pipelines whose relevant entry points can be exercised consistently.

## Evidence gaps and limits

The paper needs minimized examples demonstrating why byte and nesting limits are insufficient at useful acceptance
rates, a held-out compatibility study, and a common performance harness. Existing campaign reports must be audited
for reusable results and exact version/configuration coverage before importing any measurements into the manuscript.
The brief sanitizer campaign recorded in `SAFETY.md` supports only its tested conditions.

Lexical annotation detection does not resolve aliases or shadowing, and escaped or concatenated annotations are not
decoded. Conservative estimates can reject valid code; unmodeled syntax or downstream behavior may escape them.
Parser changes, platform stack sizes, and corpus selection limit generalization. Novelty also needs a related-work
review covering parser resource limits, bounded lexical filters, algorithmic complexity attacks, and process isolation.
The manuscript should distinguish experimentally observed coverage from any formally established property.

## Paper structure and decision point

A short paper can follow six sections: problem and motivating failures; resource contract and algorithm; implementation
and integration; evaluation; limitations and related work; implications for parser hosts. Its main figures should show
failure interception versus ordinary-source rejection and end-to-end overhead; a compact table should map source
families to downstream outcomes and the checks that detect them.

Proceed toward submission if structural screening offers a useful measured tradeoff over the baselines and the
artifact reproduces that result. If simple limits perform comparably, publish that finding with a technical report
and integration guidance. The immediate milestone is a reproducible pilot combining motivating failures, baseline
comparisons, held-out compatibility, and scan overhead before expanding the manuscript.
