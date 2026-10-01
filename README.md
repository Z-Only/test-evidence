# Test Evidence

An offline Rust CLI for reviewing AI-assisted test migrations. Compare saved JUnit/Surefire XML snapshots and optional JaCoCo coverage, then give the evidence report to ChatGPT or review it in CI.

It accounts for reported test identities, skipped and failing cases, retry evidence, reported duration distributions, and coverage counters. It does not run tests, call an AI model, upload reports, prove semantic equivalence, or establish compatibility with a particular Java/testing stack.

## Install

Install Rust through [rustup](https://rustup.rs/). This version is tested with and requires Rust **1.98.1**.

```sh
cargo install --git https://github.com/Z-Only/test-evidence --locked
test-evidence --help
```

Or clone this repository and use `cargo run --locked -- compare ...` while developing. CI builds and runs the actual CLI tests on Linux, macOS, and Windows. Verification results are recorded separately; a configured matrix alone is not a claim that it passed.

## Workflow

1. Run the project's existing tests and save a fresh baseline report directory.
2. Make the migration or AI-assisted rewrite. Run the same intended test population and save a separate candidate report directory.
3. Compare the saved snapshots. Check missing identities and intentional renames before accepting the rewrite.
4. Review the generated report for private names and paths before pasting it into an AI conversation.

```sh
test-evidence compare --baseline before/ --candidate after/ --format markdown
test-evidence compare --baseline before/ --candidate after/ --format json
test-evidence compare --baseline before/ --candidate after/ \
  --baseline-jacoco before-jacoco.xml --candidate-jacoco after-jacoco.xml
```

Directories are recursively scanned for `TEST-*.xml` only. Pass an explicit file to read another filename. Use report-only snapshot directories rather than an entire source checkout. Preserve module-relative directories between snapshots. Do not compare stale reports from previous runs.

Output goes to stdout and can be redirected with your shell. Redirecting to an existing file may overwrite it; choose a new filename. The tool does not copy to your clipboard or automatically send anything to ChatGPT.

### Exit codes

- **0:** no configured policy findings or inconclusive evidence in these reports; not proof of complete or equivalent tests
- **1:** policy findings, such as missing candidate identities, newly skipped tests, candidate failures, or comparable coverage decreases
- **2:** invalid or inconclusive input, including ambiguous identities, malformed reports, count mismatches, or non-comparable coverage

Newly added tests and improvements are reported. Retry markers are preserved as evidence, not proof that a particular cause made a test flaky. Test names absent from both snapshots are invisible to this tool.

### Explicit identity mapping

An identity contains its relative directory scope, nested suite names, classname, and exact testcase name. JUnit migration, parameterized display names, or report suffixes can change these fields. Missing in the candidate means “not found under this identity,” not “proved deleted.”

Use `--mapping mapping.json` with a JSON array of explicit one-to-one mappings. Copy the `baseline` and `candidate` identity objects from the JSON findings. Unknown endpoints, duplicate targets, and ambiguous mappings fail closed. Never map identities merely to silence a finding; verify that they represent the same intended test.

```json
[
  {
    "baseline": {"scope": "", "id": {"suite": ["example"], "classname": "OldTest", "name": "adds"}},
    "candidate": {"scope": "", "id": {"suite": ["example"], "classname": "NewTest", "name": "adds"}}
  }
]
```

### Timing and coverage interpretation

P50/P95 describe reported testcase durations in a **single snapshot**, with sample counts and exclusions. They are not repeated-run latency, build wall time, or a causal speedup measurement. Matched-population summaries only include pairs eligible on both sides. Control hardware, JDK, parallelism, warmup and run completeness outside this tool.

JaCoCo comparison uses report-level LINE/BRANCH counters without adding nested counters again. Changed class scope or denominators makes the comparison inconclusive. Equal reported scope and denominators enables a numerical comparison, not proof of unchanged instrumentation, production code, exclusions, or assertion quality. When JaCoCo is absent, coverage is explicitly not checked.

## Privacy and input boundaries

- No network, model API, telemetry, or automatic upload
- Captured test stdout/stderr, properties, exception messages and stack traces are omitted from exports
- Names, relative paths and diagnostics can still be sensitive; review before sharing
- XML external resources are never fetched; unsupported declarations and internal entity subsets are rejected
- Read stable, trusted local files: symbolic-link handling is conservative, but the filesystem loader is not a security sandbox against concurrent hostile path changes
- Limits: 500 selected reports, 16 MiB per file, 128 MiB per snapshot, 100,000 cases, 64 directory levels, 100,000 discovered entries; XML nesting is bounded separately
- Markdown previews are bounded with explicit omission counts; JSON contains the complete structured comparison

See [format research and official references](docs/research.md), [development and coverage gates](docs/development.md), and [security considerations](SECURITY.md).

## Try the synthetic examples

```sh
cargo run --locked -- compare --baseline examples/baseline --candidate examples/candidate --format json
```

This intentionally returns exit **1** to demonstrate migration findings. Use `examples/clean` as the candidate for a clean comparison returning **0**. The [example walkthrough](examples/README.md) covers mappings, retries and coverage, and the [verification record](docs/verification.md) distinguishes actual checks from remaining platform evidence.

## 中文说明

这个工具适合在 ChatGPT 协助重写或迁移测试后，比较迁移前后的实际测试报告。它会指出候选报告中未找到的测试、跳过和失败变化，并生成可继续交给 ChatGPT 分析的证据材料。

请先确认两次运行完整、环境可比，保留独立报告目录。测试改名需要人工确认后显式映射；报告没有出现的测试、断言质量和代码行为等价性，不能仅凭 XML 比较得到证明。工具不会替你执行测试或调用 AI，也不会自动上传报告。

## License

MIT. All bundled fixtures are synthetic.
