# Experimental construct-width limits

This experiment tests independent source-width budgets without changing Spam's default behavior. The
`experimental-width-limits` feature adds optional limits for call arguments, function and lambda parameters, display
items, subscript items, and f-string or template-string interpolation fields. The implementation uses one iterative,
preallocated delimiter stack. It does not build an AST or recurse.

The trial profile is:

| Construct | Limit |
| --- | ---: |
| Call arguments | 4,096 |
| Function and lambda parameters | 4,096 |
| Display items | 8,192 |
| Subscript items | 4,096 |
| Interpolation fields | 4,096 |

The profile was replayed against all 2,778 unchanged files from calibration run 27: CPython 3.14's standard library
(1,865), Monty (808), and Pydantic (105). Spam accepted every file. The highest measured real widths remain 48 call
arguments, 66 parameters, 2,231 display items, 49 slice items, and 128 interpolation fields.

All 101 retained adversarial cases were replayed against Ruff 0.0.14 and both CPython 3.14.8 and 3.15.0. Spam had no
bypasses for parser failures in either run. The new limits rejected 3 call, 5 parameter, 4 display, 4 slice, and 4
interpolation cases. Overall parser failures were 25 on CPython 3.14 and 23 on CPython 3.15; Ruff's retained crash still
reproduced. The campaign also includes syntactically valid stress cases that Spam intentionally rejects, so its
`spam_rejections_of_parsed_source` count is a protection result rather than a calibration false positive.

Monty's generated source-nesting cases were replayed separately. Seven dangerous chains were rejected, while five long
flat controls were accepted, including a 3,000-item negative-number list and a 2,000-item power-expression list.

Release-mode medians compared a reused normal checker with the same checker using the trial profile:

| Input | Normal | Width limits | Ratio |
| --- | ---: | ---: | ---: |
| Tiny assignment | 30.6 ns | 31.1 ns | 1.02× |
| 60 KB flat statements | 132.8 µs | 130.1 µs | 0.98× |
| 3,000-item list | 115.7 µs | 116.8 µs | 1.01× |
| CPython `shutil.py` | 223.3 µs | 226.0 µs | 1.01× |

The first implementation disabled the simple-line proof and made the 60 KB input about five times slower. That proof
cannot consume construct-width budget because it excludes delimiters and strings, so it remains enabled. The broader
experimental region proof is disabled whenever any width limit is configured because it may skip relevant delimiters.

Next work should fuzz each new public setting at zero and its boundary, add malformed delimiter cases around every
counter, and measure larger projects before choosing defaults. The global source, token, indentation, parser-depth, and
AST-depth defaults remain unchanged.
