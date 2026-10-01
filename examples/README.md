# Runnable synthetic examples

All names, captured output, properties and exception text here are synthetic.
Run commands from the repository root. Build once:

```sh
cargo build --locked
```

For an installed executable, replace `target/debug/test-evidence` with
`test-evidence` (Windows Cargo builds use `target/debug/test-evidence.exe`).
These commands read files only. They do not execute a test suite or call a model.

## Clean comparison: exit 0

```sh
target/debug/test-evidence compare --baseline examples/baseline --candidate examples/clean
```

Three exact identities match. Coverage is explicitly not checked. Acceptance is
limited to these supplied reports; it does not establish equivalent test behavior.

## Migration findings: exit 1

```sh
target/debug/test-evidence compare --baseline examples/baseline --candidate examples/candidate --format json > migration-evidence.json
```

The command still writes a full report when it returns 1. The report contains a
missing `adds` identity, newly added `addsNumbers`, and newly skipped `subtracts`.
Read the report despite the nonzero status; do not chain a viewer with `&&`.
The shell redirection can overwrite an existing file: choose a new output name.

## Confirmed rename: exit 0; adding a skip still exits 1

The fixture author intentionally renamed `adds` to `addsNumbers` without changing
its intended identity. That synthetic intention is the basis for this mapping;
the CLI cannot independently establish it for a real migration.

```sh
target/debug/test-evidence compare --baseline examples/baseline --candidate examples/renamed --mapping examples/mapping.json
target/debug/test-evidence compare --baseline examples/baseline --candidate examples/candidate --mapping examples/mapping.json
```

The first command matches all three identities and exits 0. The second still
reports `newly_skipped` and exits 1. Mapping does not suppress status findings.

## Comparable coverage decrease: exit 1

```sh
target/debug/test-evidence compare --baseline examples/baseline --candidate examples/clean --baseline-jacoco examples/coverage/baseline.xml --candidate-jacoco examples/coverage/decreased.xml
```

Reported line coverage falls from 9/10 to 8/10, a decrease of 10 percentage points.
Nested counters are present deliberately; they must not double the denominator.
Use `changed-scope.xml` or `changed-denominator.xml` in place of `decreased.xml` to
see exit 2, inconclusive numerical comparability.

## Failures, retries, and timing: exit 1

```sh
target/debug/test-evidence compare --baseline examples/observations/before --candidate examples/observations/after
```

`priorFailure` fails in both runs; `newFailure` begins failing; `fixed` improves.
`retry` passes with a reported retry failure marker. It is retry evidence rather
than a diagnosed flakiness cause. Candidate all-case P95 falls from 0.4 s to 0.3 s
because that retry is excluded. The matched eligible population has P95 0.3 s on
both sides. Neither observation establishes a speedup. Failed tests with valid
durations remain in the eligible timing population; “eligible” does not mean
“passed.” Captured synthetic private text must not appear in either export.

## Invalid and partial reports: exit 2

```sh
target/debug/test-evidence compare --baseline examples/baseline --candidate examples/invalid/partial.xml
target/debug/test-evidence compare --baseline examples/baseline --candidate examples/invalid/malformed.xml
```

The partial example advertises three testcases but contains only one. This
particular inconsistency is detectable. A partial run with internally consistent
counts can remain invisible; run completeness must be checked separately.

## Repeat every acceptance check

With Python 3 installed:

```sh
python3 examples/verify.py target/debug/test-evidence
```

The runner checks 13 scenarios against the actual executable, each in JSON and
Markdown, twice for byte determinism and once with stdout written directly to a
temporary file. It also checks exact findings, duration populations, coverage
arithmetic, omitted private fields, Unicode/Markdown-looking identifiers and
rejection of an illegal XML control character. Expected policy/inconclusive
statuses are assertions, so the runner exits 0 only when all checks succeed.
