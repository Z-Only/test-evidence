# Practical CLI verification

## Local evidence, 2026-10-01

Platform: Linux x86_64. Toolchain: Rust/Cargo 1.98.1. These are local results,
not a claim that GitHub Actions, macOS or Windows has passed. Check the actual
workflow run for platform status. The examples are entirely synthetic.

The compiled development CLI passed `python3 examples/verify.py
 target/debug/test-evidence`: 13 end-to-end scenarios. Each scenario checks the
exit status, byte-identical repeated JSON and Markdown, valid JSON, output
redirected directly to a file, and absence of captured private body markers.

| Scenario | Actual exit | Checked result |
|---|---:|---|
| Identical clean snapshots | 0 | Three matches, no findings; coverage absent |
| Rename without mapping | 1 | Missing old identity plus newly added identity |
| Exact rename mapping | 0 | Three matches, one mapping applied |
| Rename plus new skip | 1 | Missing, added and newly skipped findings |
| Mapping plus new skip | 1 | Rename accounted for; skip still blocks |
| Prior/current failures, improvement, retry | 1 | Two candidate failures; regression, improvement and retry evidence |
| Coverage decrease | 1 | LINE 90% to 80%, total 10, delta −10 percentage points |
| Changed reported coverage class scope | 2 | Numerically non-comparable |
| Changed coverage denominator | 2 | Numerically non-comparable |
| Malformed XML | 2 | Inconclusive diagnostic |
| Declared testcase count mismatch | 2 | Inconclusive diagnostic |
| Illegal XML escape character reference | 2 | Rejected instead of emitting a terminal escape |
| Chinese/Markdown/control-looking identifiers | 1 | Readable Chinese, safe dynamic fence, visible bidirectional control |

These tests inspect stdout from actual CLI processes rather than merely calling
library rendering functions. JSON and Markdown are deterministic for the same
inputs and relative paths on this tested platform. JSON is structured evidence,
not an inherently terminal-safe display format; Markdown visibly escapes the
bidirectional control used in the fixture. Privacy checks cover captured test
stdout/stderr, properties, failure messages/stacks and retry bodies. Identities
and diagnostic paths are deliberately retained, so review is still required.

## Interpreting the example evidence

- A prior failure remains a policy finding even without a new regression
- A new failure has both regression and candidate-failure findings. Finding
  count is not the number of distinct affected testcases
- Retry evidence is informational under the default policy; it is not a promise
  of determinism or a root-cause diagnosis
- Timing excludes skipped/retried/missing/invalid observations, not failures.
  In the observation fixture the all-case P95 drops from 0.4 s to 0.3 s solely
  through a changed eligible population; matched P95 stays 0.3 s on both sides
- An exact mapping accounts for a verified rename; it cannot suppress a skip
- Consistent reported scope and denominators permit arithmetic comparison,
  without establishing identical instrumentation or semantic equivalence
- A count mismatch catches one partial-input pattern, not every partial run

See [runnable examples](../examples/README.md) for the commands and expected
statuses. The runner writes reports only into temporary directories.

## Packaged installation: passed locally

A `.crate` archive was built with `cargo package --locked --offline --allow-dirty
--no-verify`, extracted into a fresh `/tmp` directory outside the checkout, then
installed with `cargo install --path . --locked --offline --root ...`. The release
executable passed all 13 acceptance scenarios from that extracted package. This
checks installation from the packaged source and inclusion of the example inputs.
No publishing, GitHub mutation or application network/model call was involved.

### Reproduction

The package can be created without publishing or contacting a registry. With
locked dependencies already cached locally:

```sh
cargo package --locked --offline --allow-dirty --no-verify
mkdir -p /tmp/test-evidence-package-check
tar -xzf target/package/test-evidence-0.1.0.crate -C /tmp/test-evidence-package-check
cd /tmp/test-evidence-package-check/test-evidence-0.1.0
cargo install --path . --locked --offline --root /tmp/test-evidence-install
python3 examples/verify.py /tmp/test-evidence-install/bin/test-evidence
```

Use fresh temporary paths rather than reusing a populated directory. `--no-verify`
only skips Cargo's packaging verification stage; the separate clean install
actually compiles the extracted archive, and the runner exercises that installed
executable outside the source checkout. Offline installation requires cached
Cargo dependencies and a locally available toolchain.

## macOS filename-test portability correction

The initial macOS 15 ARM64 CI run (job `110563680666`) rejected creation of
non-UTF-8 fixture filenames and directories with OS error 92, `Illegal byte
sequence`, before the loader ran. Those two raw-byte creation tests now run on
Linux, where the tested filesystem accepts such names. Unix socket/non-regular
input tests remain enabled on macOS. A separate Unix CLI test passes an invalid
byte sequence as an explicit input path without creating it and requires an
inconclusive JSON result and exit 2. Production validation and the macOS CI job
are unchanged.

After this correction, Linux verification passed `cargo test --locked
--all-targets` (43 unit tests and 27 CLI integration tests) and `cargo clippy
--locked --all-targets -- -D warnings`. These local results do not establish a
macOS pass; the corrected tests still require an actual macOS CI rerun.
