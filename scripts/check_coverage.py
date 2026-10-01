#!/usr/bin/env python3
"""Fail-closed LCOV total/diff line coverage gate; Python standard library only.

Example (run at the repository root):
  python3 scripts/check_coverage.py --base origin/main \
      --report coverage/rust.lcov .

LCOV DA records define executable lines. Every current production source file must
have a report, including files untouched by the diff, unless narrowly recognized
as a declaration-only module hub. Uninstrumented executable source is an error,
not a zero-line success. Dedicated integration tests outside src are not
production code. No CLI entry point or runtime adapter is exempted.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import json
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
from typing import Iterable, Mapping


class CoverageError(ValueError):
    """Invalid/incomplete input: never interpreted as a passing coverage result."""


class LineCoverage(dict[int, int]):
    """Physical line hits plus conservative provider summary overhead.

    LLVM emits DA from file-level coverage, but LF/LH from function summaries.
    Several functions/macros can occupy a physical line, so these counts need
    not be equal. Preserve that extra denominator and uncovered-instance count
    instead of incorrectly rejecting LLVM or inflating total coverage.
    """

    def __init__(self, lines: Mapping[int, int] | None = None, *, extra_lines: int = 0, extra_uncovered: int = 0):
        super().__init__(lines or {})
        self.extra_lines = extra_lines
        self.extra_uncovered = extra_uncovered

    def absorb(self, other: Mapping[int, int]) -> None:
        for line, hits in other.items():
            self[line] = max(self.get(line, 0), hits)
        # Physical hits can be OR-merged. Summary-only instance identities cannot,
        # so retain the largest uncovered overhead rather than invent coverage.
        self.extra_lines = max(self.extra_lines, getattr(other, "extra_lines", 0))
        self.extra_uncovered = max(self.extra_uncovered, getattr(other, "extra_uncovered", 0))

    def totals(self) -> tuple[int, int]:
        total = len(self) + self.extra_lines
        covered = sum(hits > 0 for hits in self.values()) + self.extra_lines - self.extra_uncovered
        return covered, total


@dataclass(frozen=True)
class CoverageResult:
    total_covered: int
    total_lines: int
    changed_covered: int
    changed_lines: int
    missing_files: tuple[str, ...]
    uncovered_changed: dict[str, list[int]]

    def failures(self, total_minimum: float, changed_minimum: float) -> list[str]:
        problems = []
        if self.missing_files:
            problems.append("Missing LCOV source records: " + ", ".join(self.missing_files))
        if not self.total_lines:
            problems.append("No executable production lines found in coverage reports")
        elif self.total_covered * 100 < total_minimum * self.total_lines:
            problems.append(f"Total line coverage is below {total_minimum:g}%")
        if self.changed_covered * 100 < changed_minimum * self.changed_lines:
            problems.append(f"Changed-line coverage is below {changed_minimum:g}%")
        return problems


def production_source(name: str) -> bool:
    """All application Rust sources count, including the CLI entry point.

    Dedicated tests live outside src and are not application executable lines.
    No application file, including files named tests.rs, is exempted.
    """
    path = PurePosixPath(name)
    return path.parts[0:1] == ("src",) and path.suffix == ".rs"


def git(root: Path, *args: str) -> str:
    try:
        completed = subprocess.run(
            ["git", "-C", str(root), *args], check=True,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, encoding="utf-8",
        )
    except (OSError, subprocess.CalledProcessError, UnicodeError) as exc:
        detail = getattr(exc, "stderr", None) or str(exc)
        raise CoverageError(f"Git command failed: {detail.strip()}") from exc
    return completed.stdout


def revision(root: Path, value: str, *, tree: bool = False) -> str:
    suffix = "^{tree}" if tree else "^{commit}"
    return git(root, "rev-parse", "--verify", "--end-of-options", value + suffix).strip()


def source_inventory(root: Path, head: str | None) -> set[str]:
    if head is not None:
        names = git(root, "ls-tree", "-r", "--name-only", "-z", head).split("\0")
    else:
        names = git(root, "ls-files", "--cached", "--others", "--exclude-standard", "-z").split("\0")
        names = [name for name in names if (root / name).is_file()]
    return {name for name in names if production_source(name)}


def declaration_only_module(text: str) -> bool:
    """Recognize only blank/comment lines and plain external module declarations.

    LLVM legitimately omits these files. This is intentionally not a Rust parser:
    attributes, block comments, inline modules, macros and all other syntax fail
    closed and still require an instrumentation record.
    """
    return all(
        re.fullmatch(r"[ \t]*(?:(?://[^\r\n]*)|(?:(?:pub[ \t]+)?mod[ \t]+[A-Za-z_][A-Za-z_0-9]*[ \t]*;[ \t]*(?://[^\r\n]*)?))?[ \t]*", line)
        is not None
        for line in text.splitlines()
    )


def missing_declaration_modules(
    root: Path, head: str | None, missing: set[str]
) -> set[str]:
    """Read the tested revision, not unchecked worktree contents, in commit mode."""
    recognized = set()
    for name in sorted(missing):
        text = git(root, "show", f"{head}:{name}") if head is not None else (root / name).read_text(encoding="utf-8")
        if declaration_only_module(text):
            recognized.add(name)
    return recognized


def changed_lines(root: Path, base: str, head: str | None) -> dict[str, set[int]]:
    """Use NUL-delimited names, then parse only zero-context hunk line ranges.

    A rename is diffed with both paths so unchanged renamed lines are not new.
    Deleted files/lines add no denominator. --working-tree includes untracked
    source; committed CI runs intentionally use only the tested commit.
    """
    revisions = [base] + ([head] if head is not None else [])
    common = ["--no-ext-diff", "--no-textconv", "--find-renames"]
    entries = git(root, "diff", *common, "--name-status", "-z", *revisions, "--").split("\0")
    changes: dict[str, set[int]] = {}
    cursor = 0
    while cursor < len(entries) and entries[cursor]:
        status = entries[cursor]
        cursor += 1
        if cursor >= len(entries):
            raise CoverageError("Truncated git name-status output")
        paths = [entries[cursor]]
        cursor += 1
        if status.startswith(("R", "C")):
            if cursor >= len(entries):
                raise CoverageError("Truncated git rename output")
            paths.append(entries[cursor])
            cursor += 1
        name = paths[-1]
        if status.startswith("D") or not production_source(name):
            continue
        patch = git(
            root, "diff", *common, "--unified=0", "--no-color", *revisions,
            "--", *(f":(literal){path}" for path in paths),
        )
        if "GIT binary patch" in patch or re.search(r"^Binary files .* differ$", patch, re.MULTILINE):
            raise CoverageError(f"Production source is binary: {name}")
        lines = set()
        for start, count in re.findall(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@", patch, re.MULTILINE):
            first, length = int(start), int(count) if count else 1
            lines.update(range(first, first + length))
        changes[name] = lines
    if head is None:
        for name in git(root, "ls-files", "--others", "--exclude-standard", "-z").split("\0"):
            if production_source(name):
                try:
                    content = (root / name).read_text(encoding="utf-8")
                except (OSError, UnicodeError) as exc:
                    raise CoverageError(f"Cannot read untracked source: {name}") from exc
                if "\0" in content:
                    raise CoverageError(f"Production source is binary: {name}")
                changes[name] = set(range(1, len(content.splitlines()) + 1))
    return changes


def parse_lcov(text: str, source_root: Path, root: Path) -> dict[str, LineCoverage]:
    reports: dict[str, LineCoverage] = {}
    current: str | None = None
    record_lines: dict[int, int] = {}
    line_count: int | None = None
    hit_count: int | None = None
    for number, raw in enumerate(text.splitlines(), 1):
        if raw.startswith("SF:"):
            if current is not None:
                raise CoverageError(f"LCOV record missing end_of_record at line {number}")
            name = raw[3:]
            if not name:
                raise CoverageError(f"Empty LCOV source path at line {number}")
            candidate = Path(name)
            candidate = candidate if candidate.is_absolute() else source_root / candidate
            try:
                current = candidate.resolve().relative_to(root.resolve()).as_posix()
            except ValueError as exc:
                raise CoverageError(f"LCOV source is outside repository: {name}") from exc
            record_lines, line_count, hit_count = {}, None, None
        elif raw.startswith("DA:"):
            if current is None:
                raise CoverageError(f"LCOV DA without source at line {number}")
            fields = raw[3:].split(",")
            try:
                line, hits = int(fields[0]), int(fields[1])
            except (IndexError, ValueError) as exc:
                raise CoverageError(f"Malformed LCOV DA at line {number}") from exc
            if line <= 0 or hits < 0:
                raise CoverageError(f"Invalid LCOV line/count at line {number}")
            record_lines[line] = max(record_lines.get(line, 0), hits)
        elif raw.startswith(("LF:", "LH:")):
            if current is None:
                raise CoverageError(f"LCOV summary without source at line {number}")
            try:
                value = int(raw[3:])
            except ValueError as exc:
                raise CoverageError(f"Invalid LCOV summary at line {number}") from exc
            if value < 0:
                raise CoverageError(f"Negative LCOV summary at line {number}")
            if raw.startswith("LF:"):
                line_count = value
            else:
                hit_count = value
        elif raw == "end_of_record":
            if current is None:
                raise CoverageError(f"LCOV end_of_record without source at line {number}")
            if not record_lines and line_count != 0:
                raise CoverageError(f"LCOV source has no DA records or explicit LF:0: {current}")
            if (line_count is None) != (hit_count is None):
                raise CoverageError(f"LCOV requires both LF and LH when either is supplied: {current}")
            physical_uncovered = sum(hits == 0 for hits in record_lines.values())
            record = LineCoverage(record_lines)
            if line_count is not None and hit_count is not None:
                if line_count < len(record_lines) or hit_count > line_count:
                    raise CoverageError(f"LCOV summary is smaller than its DA inventory or LH exceeds LF: {current}")
                summary_uncovered = line_count - hit_count
                if summary_uncovered < physical_uncovered:
                    raise CoverageError(f"LCOV summary omits uncovered DA lines: {current}")
                record.extra_lines = line_count - len(record_lines)
                record.extra_uncovered = summary_uncovered - physical_uncovered
            destination = reports.setdefault(current, LineCoverage())
            destination.absorb(record)
            current = None
    if current is not None:
        raise CoverageError(f"Unterminated LCOV record: {current}")
    if not reports:
        raise CoverageError("Empty LCOV report (no source records)")
    return reports


def merge_reports(reports: Iterable[Mapping[str, Mapping[int, int]]]) -> dict[str, LineCoverage]:
    merged: dict[str, LineCoverage] = {}
    for report in reports:
        for name, lines in report.items():
            destination = merged.setdefault(name, LineCoverage())
            destination.absorb(lines)
    return merged


def evaluate(inventory: set[str], changes: dict[str, set[int]], reports: Mapping[str, Mapping[int, int]], *, declaration_modules: set[str] | frozenset[str] = frozenset()) -> CoverageResult:
    missing = tuple(sorted(inventory - reports.keys() - declaration_modules))
    total_lines = total_covered = changed_count = changed_covered = 0
    uncovered: dict[str, list[int]] = {}
    for name in sorted(inventory & reports.keys()):
        lines = reports[name]
        covered, total = lines.totals() if isinstance(lines, LineCoverage) else (sum(hits > 0 for hits in lines.values()), len(lines))
        total_lines += total
        total_covered += covered
        executable_changes = changes.get(name, set()) & lines.keys()
        changed_count += len(executable_changes)
        changed_covered += sum(lines[line] > 0 for line in executable_changes)
        missed = sorted(line for line in executable_changes if lines[line] == 0)
        if missed:
            uncovered[name] = missed
    return CoverageResult(total_covered, total_lines, changed_covered, changed_count, missing, uncovered)


def percentage(covered: int, total: int) -> str:
    return f"{covered * 100 / total:.4f}% ({covered}/{total})" if total else "n/a (0 executable lines)"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--base", required=True, help="PR base commit (or an empty Git tree for initial history)")
    parser.add_argument("--head", default="HEAD", help="Tested commit, defaults to HEAD")
    parser.add_argument("--working-tree", action="store_true", help="Include staged, unstaged and untracked source")
    parser.add_argument("--repo", default=".", type=Path)
    parser.add_argument("--report", nargs=2, action="append", required=True, metavar=("LCOV_FILE", "SOURCE_ROOT"))
    parser.add_argument("--total-min", type=float, default=95)
    parser.add_argument("--changed-min", type=float, default=95)
    parser.add_argument("--json-output", type=Path)
    args = parser.parse_args(argv)
    if not 0 <= args.total_min <= 100 or not 0 <= args.changed_min <= 100:
        parser.error("Coverage thresholds must be between 0 and 100")
    try:
        root = Path(git(args.repo.resolve(), "rev-parse", "--show-toplevel").strip())
        base = revision(root, args.base, tree=True)
        head = None if args.working_tree else revision(root, args.head)
        parsed = []
        for filename, source_root in args.report:
            parsed.append(parse_lcov((root / filename).read_text(encoding="utf-8"), root / source_root, root))
        inventory = source_inventory(root, head)
        reports = merge_reports(parsed)
        declaration_modules = missing_declaration_modules(root, head, inventory - reports.keys())
        result = evaluate(inventory, changed_lines(root, base, head), reports, declaration_modules=declaration_modules)
        failures = result.failures(args.total_min, args.changed_min)
        print("Total production line coverage: " + percentage(result.total_covered, result.total_lines))
        print("Changed executable line coverage: " + percentage(result.changed_covered, result.changed_lines))
        for name, lines in result.uncovered_changed.items():
            print(f"Uncovered changed lines: {name}:" + ",".join(map(str, lines)))
        for failure in failures:
            print("ERROR: " + failure, file=sys.stderr)
        if args.json_output:
            args.json_output.parent.mkdir(parents=True, exist_ok=True)
            args.json_output.write_text(json.dumps({
                "base_tree": base, "head": head or "working-tree",
                "total": {"covered": result.total_covered, "lines": result.total_lines, "minimum": args.total_min},
                "changed": {"covered": result.changed_covered, "lines": result.changed_lines, "minimum": args.changed_min},
                "missing_files": list(result.missing_files), "declaration_only_modules": sorted(declaration_modules),
                "uncovered_changed": result.uncovered_changed,
                "passed": not failures, "failures": failures,
            }, indent=2) + "\n", encoding="utf-8")
        return 1 if failures else 0
    except (CoverageError, OSError, UnicodeError) as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
