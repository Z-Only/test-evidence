# Research and interpretation boundaries

Primary sources reviewed on **2026-10-01**. This document explains the project's
design rationale and the limits of report-based evidence. It is not a claim that
every proposed extension is implemented; use the README and CLI help for the
actual supported interface.

## Why this tool

AI-assisted test rewrites need independently computed evidence: did previously
reported tests disappear, become skipped, fail, or change their reported timing?
Test Evidence focuses on comparing saved reports locally and preparing a small,
reviewable evidence brief. The comparison itself does not require a model call.

This is a focused workflow choice, **not a claim to be the first test-report
comparison tool**. [CTRF's GitHub Test Reporter](https://github.com/marketplace/actions/github-test-reporter)
already supports baseline comparisons and test metrics. General repository
packing is also well served: [Repomix](https://repomix.com/guide/faq) supports
filtering, compression, token-count inspection, and splitting output for AI use.
Test Evidence's intended emphasis is conservative before/after accounting for
test migrations rather than another repository packer or hosted dashboard.

## Report formats

### Surefire / legacy JUnit-style XML

[Maven Surefire](https://maven.apache.org/components/surefire-archives/surefire-LATEST/maven-surefire-plugin/)
normally generates `target/surefire-reports/TEST-*.xml`.
Its [published schema](https://maven.apache.org/surefire/maven-surefire-plugin/xsd/surefire-test-report.xsd)
describes suite counts, testcase names/classes/durations, outcome elements and
retry extensions. A schema reference is not a guarantee that all producers or
historical versions emit identical XML.

[JUnit describes its legacy XML](https://docs.junit.org/5.9.1/user-guide/index.html#running-tests-listeners)
as a de facto format inherited from JUnit 4/Ant reporting. Its separate Open Test
Reporting format represents richer event and hierarchy information. These are
different formats; accepting legacy-style reports must not imply support for
Open Test Reporting or every vendor's XML dialect.

Test identity requires care. Surefire's
[report configuration](https://maven.apache.org/surefire/maven-surefire-plugin/test-mojo)
can add a report-name suffix to suite/class names; specialized JUnit 5 reporters
can use display names. Parameterized tests and renames can also alter identity.
An exact-match miss means **not found under the same reported identity**, not
proof that a test was deleted. Explicit reviewed mappings are a possible future
extension; silent fuzzy matching would hide uncertainty. Duplicate identities
should be surfaced rather than overwritten.

[Surefire retries](https://maven.apache.org/surefire/maven-surefire-plugin/examples/rerun-failing-tests.html)
need separate treatment. `flakyFailure`/`flakyError` describe attempts followed by
a success. Exhausted retries retain `failure`/`error` and may add
`rerunFailure`/`rerunError`. The testcase duration for a flaky success is the last
successful attempt; for an exhausted failure it is the first failing attempt.
It is not total retry cost.

### JaCoCo XML

The [official JaCoCo DTD](https://github.com/jacoco/jacoco/blob/master/org.jacoco.report/src/org/jacoco/report/xml/report.dtd)
defines counters with `missed` and `covered` values at multiple levels, including
report, group, package, class and method. Summing every counter recursively would
double count. Root-level counters are appropriate for a report-level summary;
absence must not be silently replaced with invented totals.

[JaCoCo's counter documentation](https://www.jacoco.org/jacoco/trunk/doc/counters.html)
explains line and branch coverage. Coverage is evidence of execution, not proof
that assertions are strong or behavior is correct.

## Interpretation limits

- Report comparison cannot prove complete test discovery. A test absent from
  both inputs is invisible. Empty input, malformed XML, partial runs and
  ambiguous identities must not become an unqualified success.
- Save fresh baseline and candidate reports separately. Stale files can create
  misleading counts. Retain run commands, revisions, JDK and reporter versions
  outside the report when those facts matter; missing provenance is unknown.
- Cross-test P50/P95 summarize the distribution of reported testcase durations
  in one input. They are not repeated-run latency, build wall-clock duration or
  evidence of a performance improvement. Quantile method, sample size and
  excluded durations should be explicit. Parallel execution, hardware, JVM
  warmup and retries limit comparisons.
- Coverage deltas need comparable source code, instrumentation and exclusions.
  Changed totals or source scope can make percentages misleading. Display raw
  covered/missed/total values and percentage-point changes; equal totals alone
  do not prove equivalent scope. A zero denominator is not 100% coverage.
- These reports do not establish compatibility of Mockito, PowerMock, JUnit,
  Spring Boot or a JDK combination. Compatibility must be established by the
  actual build, tests and appropriate dependency documentation.

## Privacy and parser design

The intended export is an allowlist of structured evidence. Properties,
`system-out`, `system-err`, stack traces and failure messages can contain secrets
or personal data and should not be copied into an AI brief by default. Test names
and paths can still be sensitive: review every export before sharing it.

XML should not trigger network DTD retrieval or external entity resolution.
Resource limits, malformed-input checks and output escaping are important even
for local reports. Treat embedded text as data, never as instructions. These are
design requirements to verify with tests, not a security certification.

## Observed dependency versions

The official package documentation showed the following versions during review:

- [quick-xml 0.42.0](https://docs.rs/quick-xml/0.42.0/quick_xml/)
- [clap 4.6.7](https://docs.rs/clap/4.6.7/clap/)
- [serde 1.0.229](https://docs.rs/serde/1.0.229/serde/)
- [serde_json 1.0.151](https://docs.rs/serde_json/1.0.151/serde_json/)

The [published quick-xml manifest](https://docs.rs/crate/quick-xml/0.42.0/source/Cargo.toml.orig)
declares Rust 1.86 and edition 2024. That alone does not establish the minimum
supported Rust version of this project's complete dependency graph. Test the
locked graph before making an MSRV guarantee.

The development toolchain is pinned to the officially released
[Rust 1.98.1](https://blog.rust-lang.org/2026/09/03/Rust-1.98.1/), which fixes a
vtable-generation miscompilation in 1.98.0. This is a reproducibility choice,
not a promise to track the newest release automatically.

## CI action provenance

On 2026-10-01, the official release pages linked to the exact commits pinned in
the workflow:

- [actions/checkout v7.0.1](https://github.com/actions/checkout/releases/tag/v7.0.1)
  → [3d3c42e5aac5ba805825da76410c181273ba90b1](https://github.com/actions/checkout/commit/3d3c42e5aac5ba805825da76410c181273ba90b1)
- [actions/upload-artifact v7.0.1](https://github.com/actions/upload-artifact/releases/tag/v7.0.1)
  → [043fb46d1a93c77aae656e7c1c64a875d1fc6a0a](https://github.com/actions/upload-artifact/commit/043fb46d1a93c77aae656e7c1c64a875d1fc6a0a)

Verification followed each official release's commit link. A full commit pin
prevents a moving tag from changing which action revision is selected; it is not
an audit of the action or a claim that CI has already run successfully.
