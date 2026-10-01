"""Regression tests for the coverage gate, including real Git diffs."""

import contextlib
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

import check_coverage as gate


class SourceSelectionTests(unittest.TestCase):
    def test_runtime_sources_are_included(self):
        for name in ["src/lib.rs", "src/main.rs", "src/parser/xml.rs", "src/cli.rs", "src/tests.rs"]:
            with self.subTest(name=name):
                self.assertTrue(gate.production_source(name))

    def test_nonproduction_files_are_excluded(self):
        for name in ["README.md", "scripts/check_coverage.py", "tests/integration.rs", "target/generated.rs", "examples/demo.rs"]:
            with self.subTest(name=name):
                self.assertFalse(gate.production_source(name))


class DeclarationModuleTests(unittest.TestCase):
    def test_plain_declarations_and_line_comments(self):
        for content in ["", "\n\t\n", "//! Module docs\n// comment\n", "pub mod compare;\nmod private;\n", "  pub\tmod report; // trailing comment\n"]:
            with self.subTest(content=content):
                self.assertTrue(gate.declaration_only_module(content))

    def test_every_other_syntax_requires_coverage(self):
        for content in [
            "fn main() {}", "pub fn run() {}", "macro_rules! hide { () => {} }",
            "include!(\"generated.rs\");", "#[path = \"other.rs\"]\npub mod other;",
            "#![allow(dead_code)]\npub mod other;", "pub mod nested {}",
            "pub(crate) mod report;", "pub use report::*;", "const VALUE: u8 = 1;",
            "/* comment */\npub mod report;", "pub mod report; fn main() {}",
            "// comment\nfn main() {}", "pub mod report;\nprintln!(\"hidden\");",
        ]:
            with self.subTest(content=content):
                self.assertFalse(gate.declaration_only_module(content))

    def test_declaration_module_never_fabricates_a_denominator(self):
        result = gate.evaluate({"src/lib.rs"}, {}, {}, declaration_modules={"src/lib.rs"})
        self.assertEqual(result.missing_files, ())
        self.assertEqual((result.total_covered, result.total_lines), (0, 0))
        self.assertIn("No executable production lines found", result.failures(95, 95)[0])


class LcovTests(unittest.TestCase):
    def setUp(self):
        self.root = Path("/tmp/coverage-repo")

    def parse(self, content, source_root=None):
        return gate.parse_lcov(content, source_root or self.root, self.root)

    def test_absolute_and_relative_paths(self):
        self.assertEqual(self.parse("SF:/tmp/coverage-repo/src/lib.rs\nDA:2,1\nend_of_record\n"), {"src/lib.rs": {2: 1}})
        self.assertEqual(self.parse("SF:src/main.rs\nDA:2,0\nend_of_record\n", self.root), {"src/main.rs": {2: 0}})

    def test_duplicate_records_merge_without_inflating_denominator(self):
        result = self.parse("TN:first\nSF:src/main.rs\nDA:2,0\nLF:1\nLH:0\nend_of_record\nTN:second\nSF:src/main.rs\nDA:2,3\nDA:3,0,checksum\nLF:2\nLH:1\nend_of_record\n")
        self.assertEqual(result, {"src/main.rs": {2: 3, 3: 0}})
        self.assertEqual(gate.merge_reports([result, result]), result)

    def test_llvm_function_summary_exceeds_physical_da_lines(self):
        # Regression: real LLVM exports had 243 DA lines but LF/LH of 257/257;
        # the native server had 271 covered DA lines but LF/LH of 354/326.
        report = self.parse("SF:src/lib.rs\nDA:1,1\nDA:2,1\nLF:4\nLH:3\nend_of_record\n")
        result = gate.evaluate(set(report), {"src/lib.rs": {1, 2}}, report)
        self.assertEqual((result.total_covered, result.total_lines), (3, 4))
        self.assertEqual((result.changed_covered, result.changed_lines), (2, 2))
        self.assertEqual(len(result.failures(95, 95)), 1)

    def test_duplicate_da_and_reports_do_not_inflate_summary(self):
        report = self.parse("SF:src/lib.rs\nDA:1,0\nDA:1,2\nDA:2,0\nLF:3\nLH:1\nend_of_record\n")
        merged = gate.merge_reports([report, report])
        result = gate.evaluate(set(merged), {}, merged)
        self.assertEqual((result.total_covered, result.total_lines), (1, 3))
        self.assertEqual(merged["src/lib.rs"], {1: 2, 2: 0})

    def test_summary_retains_uncovered_overlapping_function_line(self):
        report = self.parse("SF:src/lib.rs\nDA:1,2\nDA:2,1\nLF:2\nLH:1\nend_of_record\n")
        self.assertEqual(report["src/lib.rs"].totals(), (1, 2))

    def test_complementary_records_merge_only_proven_physical_hits(self):
        first = self.parse("SF:src/lib.rs\nDA:1,2\nDA:2,0\nLF:4\nLH:2\nend_of_record\n")
        second = self.parse("SF:src/lib.rs\nDA:1,0\nDA:2,1\nLF:4\nLH:2\nend_of_record\n")
        merged = gate.merge_reports([first, second])
        self.assertEqual(merged["src/lib.rs"].totals(), (3, 4))

    def test_llvm_summary_only_without_da_is_rejected(self):
        with self.assertRaises(gate.CoverageError):
            self.parse("SF:src/lib.rs\nLF:257\nLH:257\nend_of_record\n")

    def test_impossible_summary_is_rejected(self):
        for content in [
            "SF:a\nDA:1,1\nDA:2,1\nLF:1\nLH:1\nend_of_record\n",
            "SF:a\nDA:1,1\nLF:1\nLH:2\nend_of_record\n",
            "SF:a\nDA:1,0\nLF:1\nLH:1\nend_of_record\n",
        ]:
            with self.subTest(content=content), self.assertRaises(gate.CoverageError):
                self.parse(content)

    def test_explicit_zero_executable_record_is_preserved(self):
        self.assertEqual(self.parse("SF:src/types.rs\nLF:0\nLH:0\nend_of_record\n"), {"src/types.rs": {}})

    def test_malformed_or_incomplete_reports_fail_closed(self):
        invalid = ["SF:src/worker.rs\nend_of_record\n", "", "TN:empty\n", "SF:\nend_of_record\n", "SF:../escape.rs\nend_of_record\n", "DA:1,1\n", "end_of_record\n", "SF:a\nDA:1,no\nend_of_record\n", "SF:a\nDA:0,1\nend_of_record\n", "SF:a\nDA:1,-1\nend_of_record\n", "SF:a\nDA:1\nend_of_record\n", "SF:a\n", "SF:a\nSF:b\n", "LF:2\n", "SF:a\nLF:-1\nend_of_record\n", "SF:a\nLF:x\nend_of_record\n", "SF:a\nDA:1,1\nLF:2\nend_of_record\n", "SF:a\nDA:1,0\nLH:1\nend_of_record\n"]
        for content in invalid:
            with self.subTest(content=content), self.assertRaises(gate.CoverageError):
                self.parse(content)


class EvaluationTests(unittest.TestCase):
    def test_exact_thresholds_pass(self):
        result = gate.CoverageResult(95, 100, 95, 100, (), {})
        self.assertEqual(result.failures(95, 95), [])

    def test_rounding_cannot_bypass_threshold(self):
        result = gate.CoverageResult(94999, 100000, 94999, 100000, (), {})
        self.assertEqual(len(result.failures(95, 95)), 2)

    def test_total_and_changed_95_percent_gates_are_independent(self):
        self.assertEqual(len(gate.CoverageResult(94, 100, 95, 100, (), {}).failures(95, 95)), 1)
        self.assertEqual(len(gate.CoverageResult(95, 100, 94, 100, (), {}).failures(95, 95)), 1)

    def test_missing_unchanged_and_changed_sources_fail(self):
        result = gate.evaluate({"a", "b", "c"}, {"a": {1}, "c": {1}}, {"a": {1: 1}})
        self.assertEqual(result.missing_files, ("b", "c"))
        self.assertEqual(len(result.failures(95, 95)), 1)

    def test_no_executable_changed_lines_is_allowed_but_empty_total_is_not(self):
        self.assertEqual(gate.CoverageResult(1, 1, 0, 0, (), {}).failures(95, 95), [])
        self.assertTrue(gate.CoverageResult(0, 0, 0, 0, (), {}).failures(95, 95))
        self.assertIn("n/a", gate.percentage(0, 0))

    def test_changed_denominator_uses_instrumented_lines(self):
        result = gate.evaluate({"a"}, {"a": {1, 2, 3, 4}}, {"a": {2: 1, 4: 0, 6: 1}, "deleted": {1: 0}})
        self.assertEqual((result.total_covered, result.total_lines), (2, 3))
        self.assertEqual((result.changed_covered, result.changed_lines), (1, 2))
        self.assertEqual(result.uncovered_changed, {"a": [4]})


class GitTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.git("init", "-q")
        self.git("config", "user.email", "ci-test@example.invalid")
        self.git("config", "user.name", "Coverage Tests")
        self.write("README.md", "initial\n")
        self.commit()
        self.base = self.git("rev-parse", "HEAD").strip()

    def git(self, *args):
        return subprocess.run(["git", "-C", str(self.root), *args], check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True).stdout

    def write(self, name, text):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def commit(self):
        self.git("add", "--all")
        self.git("commit", "-qm", "test")

    def changes(self, working_tree=False):
        return gate.changed_lines(self.root, self.base, None if working_tree else "HEAD")

    def test_added_source_counts_all_lines(self):
        self.write("src/main.rs", "const FIRST: usize = 1;\nconst SECOND: usize = 2;\n")
        self.commit()
        self.assertEqual(self.changes(), {"src/main.rs": {1, 2}})
        self.assertEqual(gate.source_inventory(self.root, "HEAD"), {"src/main.rs"})

    def test_modified_deleted_and_renamed_lines(self):
        self.write("src/old.rs", "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n")
        self.write("src/deleted.rs", "remove\n")
        self.commit()
        self.base = self.git("rev-parse", "HEAD").strip()
        self.git("mv", "src/old.rs", "src/renamed.rs")
        self.write("src/renamed.rs", "a\nb\nc\nchanged\ne\nf\ng\nh\ni\nj\n")
        self.git("rm", "src/deleted.rs")
        self.commit()
        self.assertEqual(self.changes(), {"src/renamed.rs": {4}})

    def test_unchanged_rename_has_no_changed_lines(self):
        self.write("src/old.rs", "const A: usize = 1;\n")
        self.commit()
        self.base = self.git("rev-parse", "HEAD").strip()
        self.git("mv", "src/old.rs", "src/new.rs")
        self.commit()
        self.assertEqual(self.changes(), {"src/new.rs": set()})

    def test_deletion_only_has_no_added_lines(self):
        self.write("src/main.rs", "a\nb\nc\n")
        self.commit()
        self.base = self.git("rev-parse", "HEAD").strip()
        self.write("src/main.rs", "a\nc\n")
        self.commit()
        self.assertEqual(self.changes(), {"src/main.rs": set()})

    def test_working_tree_includes_staged_unstaged_and_untracked_but_not_tests(self):
        self.write("src/staged.rs", "one\n")
        self.git("add", "src/staged.rs")
        self.write("src/staged.rs", "one\ntwo\n")
        self.write("src/worker.rs", "worker\n")
        self.write("tests/worker.rs", "test\n")
        self.assertEqual(self.changes(True), {"src/staged.rs": {1, 2}, "src/worker.rs": {1}})
        self.assertEqual(self.changes(), {})
        self.assertEqual(gate.source_inventory(self.root, None), {"src/staged.rs", "src/worker.rs"})

    def test_spaces_tabs_unicode_and_pathspec_characters(self):
        name = "src/space\tü [*].rs"
        self.write(name, "one\ntwo\n")
        self.commit()
        self.assertEqual(self.changes(), {name: {1, 2}})

    def test_binary_source_fails(self):
        self.write("src/binary.rs", "binary\0data\n")
        with self.assertRaises(gate.CoverageError):
            self.changes(True)
        self.commit()
        with self.assertRaises(gate.CoverageError):
            self.changes()

    def test_invalid_base_fails(self):
        with self.assertRaises(gate.CoverageError):
            gate.revision(self.root, "nonexistent")

    def test_empty_tree_can_be_initial_base(self):
        empty = subprocess.run(["git", "-C", str(self.root), "hash-object", "-t", "tree", "--stdin"], input="", text=True, check=True, stdout=subprocess.PIPE).stdout.strip()
        self.assertEqual(gate.revision(self.root, empty, tree=True), empty)

    def test_module_hub_missing_record_passes_without_filename_exclusion(self):
        self.write("src/hub.rs", "//! Documentation\npub mod runtime;\n")
        self.write("src/runtime.rs", "fn runtime() {}\n")
        self.commit()
        self.write("coverage.lcov", "SF:src/runtime.rs\nDA:1,1\nend_of_record\n")
        args = ["--repo", str(self.root), "--base", self.base, "--report", "coverage.lcov", ".", "--json-output", str(self.root / "summary.json")]
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(gate.main(args), 0)
            summary = json.loads((self.root / "summary.json").read_text())
            self.assertEqual(summary["declaration_only_modules"], ["src/hub.rs"])
            self.assertEqual(summary["total"]["lines"], 1)
            for runtime in ["fn run() {}\n", "include!(\"generated.rs\");\n"]:
                self.write("src/hub.rs", runtime)
                self.assertEqual(gate.main(args + ["--working-tree"]), 1)

    def test_declaration_check_uses_committed_contents_in_commit_mode(self):
        self.write("src/lib.rs", "fn missed() {}\n")
        self.commit()
        self.write("src/lib.rs", "pub mod runtime;\n")
        self.assertEqual(gate.missing_declaration_modules(self.root, "HEAD", {"src/lib.rs"}), set())
        self.assertEqual(gate.missing_declaration_modules(self.root, None, {"src/lib.rs"}), {"src/lib.rs"})
        self.write("src/lib.rs", "fn missed() {}\n")
        self.assertEqual(gate.missing_declaration_modules(self.root, None, {"src/lib.rs"}), set())

    def test_cli_defaults_to_95_percent_for_both_gates(self):
        name = "src/main.rs"
        self.write(name, "const VALUE: usize = 1;\n" * 20)
        self.commit()
        args = ["--repo", str(self.root), "--base", self.base, "--report", "coverage.lcov", ".", "--json-output", str(self.root / "summary.json")]
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            for covered, expected_exit in [(19, 0), (18, 1)]:
                records = "".join(f"DA:{line},{int(line <= covered)}\n" for line in range(1, 21))
                self.write("coverage.lcov", f"SF:{name}\n{records}LF:20\nLH:{covered}\nend_of_record\n")
                self.assertEqual(gate.main(args), expected_exit)
                summary = json.loads((self.root / "summary.json").read_text())
                self.assertEqual(summary["total"]["minimum"], 95)
                self.assertEqual(summary["changed"]["minimum"], 95)

    def test_cli_pass_fail_json_and_missing_report(self):
        name = "src/main.rs"
        self.write(name, "const ONE: usize = 1;\nconst TWO: usize = 2;\n")
        self.commit()
        self.write("coverage.lcov", f"SF:{name}\nDA:1,1\nDA:2,1\nend_of_record\n")
        args = ["--repo", str(self.root), "--base", self.base, "--report", "coverage.lcov", ".", "--json-output", str(self.root / "summary.json")]
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(gate.main(args), 0)
            self.assertTrue(json.loads((self.root / "summary.json").read_text())["passed"])
            self.write("coverage.lcov", f"SF:{name}\nDA:1,1\nDA:2,0\nend_of_record\n")
            self.assertEqual(gate.main(args), 1)
            self.assertFalse(json.loads((self.root / "summary.json").read_text())["passed"])
            self.write("coverage.lcov", "")
            self.assertEqual(gate.main(args), 2)
            (self.root / "coverage.lcov").unlink()
            self.assertEqual(gate.main(args), 2)


if __name__ == "__main__":
    unittest.main()
