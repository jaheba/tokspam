# Parser calibration

This run measured CPython 3.14 source processing across 2,778 valid local files:

| Corpus | Files |
| --- | ---: |
| CPython 3.14 standard library | 1,865 |
| Monty primary checkout | 808 |
| Pydantic 2 installation | 105 |

Three files were not UTF-8, three failed `ast.parse`, and 46 parsed to an AST but intentionally failed compilation.
Those files were excluded.
Each retained file was parsed and compiled three times in-process.
Spam and Ruff ran in disposable processes with a 500 ms timeout.
No retained file was rejected by Spam.

## Distribution

| Measurement | Median | p95 | p99 | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Source bytes | 3,916 | 69,040 | 155,450 | 603,041 |
| Tokens | 716 | 11,623 | 28,972 | 91,004 |
| AST nodes | 469 | 9,340 | 23,829 | 79,687 |
| Statements | 54 | 1,015 | 2,470 | 7,629 |
| Unique identifiers | 29 | 279 | 564 | 1,655 |
| Identifier occurrences | 137 | 3,115 | 8,140 | 27,295 |
| Maximum arguments in one call | 3 | 8 | 12 | 48 |
| Maximum parameters | 2 | 8 | 12 | 66 |
| Maximum items in one display | 3 | 31 | 198 | 2,231 |
| Maximum items in one subscript | 0 | 2 | 4 | 49 |
| Maximum interpolation fields | 0 | 3 | 5 | 128 |
| CPython parse time | 0.31 ms | 7.30 ms | 19.31 ms | 96.61 ms |
| CPython compile time | 0.36 ms | 7.15 ms | 18.80 ms | 70.00 ms |
| Ruff process wall time | 4.72 ms | 6.42 ms | 8.02 ms | 17.12 ms |

Ruff values include process startup and parsing plus AST disposal.
The run did not retain Ruff's parser status, so its times do not establish that Ruff accepted every CPython-valid file.

The largest real constructs were:

- 48 call arguments in Pydantic's `_internal/_config.py`;
- 66 parameters in Monty's `function__arity_defaults.py` test case;
- 2,231 dictionary items in `html/entities.py`;
- 49 slice items in Pydantic's `errors.py`;
- 128 interpolation fields in `test/test_argparse.py`.

`test/test_typing.py` was the largest module by several measures: 91,004 tokens, 79,687 AST nodes, 7,629 statements,
27,295 identifier occurrences, and a 96.61 ms median parse time.

## Comparison with retained attacks

| Construct | Largest calibrated file | First sampled attack above 100 ms | Approximate margin |
| --- | ---: | ---: | ---: |
| Call arguments | 48 | 39,200 | 817× |
| Parameters | 66 | 14,000 | 212× |
| Dictionary items | 2,231 | 24,000 | 11× |
| Slice items | 49 | 16,800 | 343× |
| Template-string fields | 128 | 24,000 | 188× |

These dimensions have room for configurable width limits below the observed attack thresholds and well above the
calibrated maxima.
A first experimental range could be 2,048–4,096 for calls, parameters, slices, and interpolation fields, and 8,192 for
display items.
Those are candidate ranges for cross-platform validation, not defaults.

Module-wide token, statement, and identifier limits have much less margin.
The standard library reaches 91% of Spam's current 100,000-token default, and its slowest valid file sits just below the
100 ms campaign target.
The colliding-identifier attack crossed the target around 16,800 statements, only about 2.2 times the calibrated
maximum.
A module-wide statement limit derived from this sample would be prone to false positives in generated or test code.

The 100 ms target should remain a discovery threshold rather than a portable acceptance promise.
The slowest standard-library file leaves too little allowance for hardware, build, operating-system, and Python-version
variation.
Construct-specific limits have substantially more margin than a tighter global token limit.

Raw observations are in `cpython314.json.gz`, derived percentiles and maxima are in `summary.json`, and the same 2,778
rows are stored as calibration run 27 in `corpus/cases.sqlite`.
