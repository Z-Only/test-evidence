#!/usr/bin/env python3
"""Offline end-to-end acceptance checks against an actual compiled CLI.

Run: python3 examples/verify.py /absolute/path/to/test-evidence
Only synthetic inputs are used; generated exports live in a temporary directory.
"""
import json
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
EXE = str(Path(sys.argv[1]).resolve()) if len(sys.argv) == 2 else None
if EXE is None:
    raise SystemExit("usage: python3 examples/verify.py /path/to/test-evidence")


def check(label, baseline, candidate, expected, extra=()):
    args = [EXE, "compare", "--baseline", "examples/" + baseline,
            "--candidate", "examples/" + candidate, *extra]
    results = {}
    for fmt in ("json", "markdown"):
        command = args + ["--format", fmt]
        first = subprocess.run(command, cwd=ROOT, capture_output=True, check=False)
        second = subprocess.run(command, cwd=ROOT, capture_output=True, check=False)
        assert first.returncode == expected, (label, fmt, first.returncode, first.stderr)
        assert second.returncode == expected
        assert first.stdout == second.stdout, (label, "non-deterministic output")
        assert not first.stderr, (label, first.stderr)
        assert b"SYNTHETIC_PRIVATE_" not in first.stdout, (label, "private body leaked")
        assert b"\x1b" not in first.stdout, (label, "terminal escape emitted")
        results[fmt] = first.stdout.decode("utf-8")
        # Exercise stdout written directly into a file, rather than reconstructed JSON.
        with tempfile.TemporaryDirectory(prefix="test-evidence-export-") as directory:
            target = Path(directory) / ("report." + fmt)
            with target.open("wb") as out:
                redirected = subprocess.run(command, cwd=ROOT, stdout=out,
                                            stderr=subprocess.PIPE, check=False)
            assert redirected.returncode == expected
            assert target.read_bytes() == first.stdout
    parsed = json.loads(results["json"])
    assert parsed["schema_version"] == 1
    assert parsed["verdict"] == {0: "accepted", 1: "policy_findings", 2: "inconclusive"}[expected]
    print(f"PASS {label}: exit {expected}; deterministic JSON/Markdown; stdout export")
    return parsed, results["markdown"]


def kinds(report):
    return [f["kind"] for f in report["findings"]]


def jacoco(candidate):
    return ("--baseline-jacoco", "examples/coverage/baseline.xml",
            "--candidate-jacoco", "examples/coverage/" + candidate)


clean, _ = check("clean", "baseline", "clean", 0)
assert clean["matched_cases"] == 3 and not clean["findings"]
assert clean["coverage"] == []
assert clean["durations"]["baseline"]["p50_seconds"] == 0.02
rename, _ = check("rename without mapping", "baseline", "renamed", 1)
assert kinds(rename) == ["missing_in_candidate", "newly_added"]
mapped, _ = check("exact rename mapping", "baseline", "renamed", 0,
                  ("--mapping", "examples/mapping.json"))
assert mapped["matched_cases"] == 3 and mapped["mappings_applied"] == 1
skipped, _ = check("rename plus newly skipped", "baseline", "candidate", 1)
assert set(kinds(skipped)) == {"missing_in_candidate", "newly_added", "newly_skipped"}
skipped_mapped, _ = check("mapping cannot hide skip", "baseline", "candidate", 1,
                         ("--mapping", "examples/mapping.json"))
assert kinds(skipped_mapped) == ["newly_skipped"]
observed, _ = check("failures retries and durations", "observations/before", "observations/after", 1)
assert kinds(observed).count("candidate_failure") == 2
assert {"passed_to_failure", "improved", "retry_notice"}.issubset(kinds(observed))
d = observed["durations"]
assert d["baseline"]["sample_count"] == 4 and d["candidate"]["sample_count"] == 3
assert d["candidate"]["excluded_retried"] == 1 and d["candidate"]["excluded_missing"] == 1
assert d["matched_pairs"] == 6 and d["matched_eligible_pairs"] == 3
assert d["matched_baseline"]["p95_seconds"] == d["matched_candidate"]["p95_seconds"] == 0.3
coverage, _ = check("coverage decrease", "baseline", "clean", 1, jacoco("decreased.xml"))
assert kinds(coverage) == ["coverage_decrease"]
assert coverage["coverage"][0]["lines"]["percentage_point_delta"] == -10.0
assert coverage["coverage"][0]["lines"]["baseline"]["total"] == 10
scope, _ = check("changed coverage scope", "baseline", "clean", 2, jacoco("changed-scope.xml"))
assert not scope["coverage"][0]["numerically_comparable"]
denominator, _ = check("changed coverage denominator", "baseline", "clean", 2,
                       jacoco("changed-denominator.xml"))
assert not denominator["coverage"][0]["numerically_comparable"]
for label, file in [("malformed XML", "malformed.xml"), ("partial count mismatch", "partial.xml"),
                    ("illegal XML control", "control.xml")]:
    invalid, _ = check(label, "baseline", "invalid/" + file, 2)
    assert invalid["diagnostics"]
identifiers, markdown = check("Chinese and markup identifiers", "identifiers/before.xml", "identifiers/after.xml", 1)
assert "输入中文" in markdown and "````text" in markdown
assert "\\u202e" in markdown and "\u202e" not in markdown
assert kinds(identifiers) == ["newly_skipped"]
print("All 13 acceptance scenarios passed. No network or model calls were made by this runner.")
