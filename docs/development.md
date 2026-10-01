# Development and quality gates

Install Rust with the official [rustup installer](https://rustup.rs/), plus
Python 3 and Git. The repository pins the toolchain in `rust-toolchain.toml` and
application dependencies in `Cargo.lock`. Run all commands from the repository
root. Test fixtures are synthetic. Tests require no API keys or external service.

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo build --locked --release
cargo test --locked --all-targets --all-features
cargo run --locked --release -- --help
python3 -m unittest discover -s scripts -p 'test_*.py' -v
```

## Coverage

```sh
cargo install cargo-llvm-cov --locked --version 0.9.1
mkdir -p coverage
cargo llvm-cov --locked --all-targets --all-features --lcov --output-path coverage/rust.lcov
python3 scripts/check_coverage.py --base origin/main --working-tree \
  --report coverage/rust.lcov . --json-output coverage/summary.json
```

The coverage command runs the test suite, including CLI integration tests. Tests
that spawn the instrumented CLI must inherit `LLVM_PROFILE_FILE`; do not clear the
environment or use a separately built uninstrumented executable. Use Cargo's
`CARGO_BIN_EXE_test-evidence` integration-test executable. No `src/**/*.rs` file is
excluded from the gate, including the CLI entry point. Keep dedicated integration
tests under `tests/`.

Total executable production lines and added/changed executable production lines
must **independently reach 95%**. Values are compared without percentage rounding.
LCOV `DA` records define executable physical lines; LLVM function-summary overhead
is conservatively retained in the total denominator. Changed lines come from
Git zero-context diffs against the base revision. A renamed unchanged line is not
new; deletion-only and documentation-only changes add no executable denominator.
A changed-line result with zero executable lines is N/A, never 100%.

Missing executable-source records, missing/empty/malformed coverage, invalid Git refs,
and an empty production denominator fail closed. An explicit zero-line record is
valid for a source file containing only declarations, but cannot make an empty
application pass. LLVM may omit files containing only blank lines, line comments,
and plain `mod name;` or `pub mod name;` declarations. The gate recognizes exactly
that narrow syntax from the tested revision (or local worktree in working-tree
mode), records these module hubs in its JSON summary, and adds no coverage lines
for them. Attributes, macros, functions, inline modules, block comments, and any
other syntax still require LCOV records. This is syntax-based, never a filename
exclusion. The gate is tested against real temporary Git repositories,
renames, unusual paths, staged/untracked sources, and threshold boundaries.

CI uses the PR base SHA, merge-group base SHA, or push's previous SHA. The first
push uses Git's empty tree so every application line is newly changed. CI checks
the tested commit; `--working-tree` is only for local uncommitted development.
Do not lower thresholds, remove production sources from coverage, or ignore
failures to get a green check. Add useful tests or fix dead/unreachable code.

## Required CI and updates

The `rust` matrix builds and runs tests on Linux, macOS, and Windows. Linux also
measures coverage and tests the coverage gate. The aggregate `ci-gate` requires
every job to succeed; failures, cancellations, and skipped jobs fail it. Configure
branch protection to require `ci-gate`, disallow direct pushes and force pushes,
and enforce protection for administrators when appropriate to repository policy.

Actions are pinned to immutable commit SHAs. Dependabot proposes weekly Cargo and
GitHub Actions updates. Review the actual dependency diff and re-run all gates.
CI uses a read-only token, no persisted Git credentials, and no secrets or deploy
steps. Contributor pull requests execute only in the normal `pull_request`
workflow; there is no privileged `pull_request_target` workflow.
