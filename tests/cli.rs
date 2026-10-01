use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "test-evidence-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, content: impl AsRef<[u8]>) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        path
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/cli")
        .join(name)
}
fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_test-evidence"))
}
fn compare(baseline: &Path, candidate: &Path, extra: &[&str]) -> Output {
    cli()
        .arg("compare")
        .arg("--baseline")
        .arg(baseline)
        .arg("--candidate")
        .arg(candidate)
        .args(extra)
        .output()
        .unwrap()
}
fn json(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).unwrap()
}
fn code(output: &Output) -> i32 {
    output.status.code().unwrap()
}
fn pass() -> String {
    fs::read_to_string(fixture("pass.xml")).unwrap()
}

#[test]
fn help_and_version_are_available() {
    for args in [vec!["--help"], vec!["compare", "--help"], vec!["--version"]] {
        let out = cli().args(args).output().unwrap();
        assert_eq!(code(&out), 0);
        assert!(!out.stdout.is_empty());
    }
    let help = cli().args(["compare", "--help"]).output().unwrap();
    assert!(String::from_utf8_lossy(&help.stdout).contains("TEST-*.xml"));
}
#[test]
fn missing_arguments_and_unknown_formats_are_errors() {
    assert_eq!(code(&cli().arg("compare").output().unwrap()), 2);
    assert_eq!(
        code(&compare(
            &fixture("pass.xml"),
            &fixture("pass.xml"),
            &["--format", "html"]
        )),
        2
    );
}
#[test]
fn identical_snapshot_is_accepted_in_both_formats() {
    let md = compare(&fixture("pass.xml"), &fixture("pass.xml"), &[]);
    assert_eq!(code(&md), 0, "{}", String::from_utf8_lossy(&md.stdout));
    assert!(String::from_utf8_lossy(&md.stdout).contains('#'));
    let out = compare(
        &fixture("pass.xml"),
        &fixture("pass.xml"),
        &["--format", "json"],
    );
    assert_eq!(code(&out), 0);
    assert_eq!(json(&out)["verdict"], "accepted");
}
#[test]
fn failed_candidate_is_policy_finding_and_private_bodies_are_omitted() {
    for format in ["markdown", "json"] {
        let out = compare(
            &fixture("pass.xml"),
            &fixture("fail.xml"),
            &["--format", format],
        );
        assert_eq!(code(&out), 1);
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(!text.contains("PRIVATE_"));
        assert!(!String::from_utf8_lossy(&out.stderr).contains("PRIVATE_"));
    }
}
#[test]
fn malformed_missing_and_unsupported_inputs_are_inconclusive() {
    let temp = Temp::new();
    for (name, body) in [
        ("malformed.xml", "<testsuite>"),
        ("unsupported.xml", "<html/>"),
        ("empty.xml", "<testsuite name=\"empty\"/>"),
    ] {
        let bad = temp.write(name, body);
        let out = compare(&fixture("pass.xml"), &bad, &["--format", "json"]);
        assert_eq!(code(&out), 2);
        assert_eq!(json(&out)["verdict"], "inconclusive");
    }
    assert_eq!(
        code(&compare(
            &temp.0.join("missing.xml"),
            &fixture("pass.xml"),
            &[]
        )),
        2
    );
    assert_eq!(
        code(&compare(
            &temp.0.join("missing-dir"),
            &fixture("pass.xml"),
            &[]
        )),
        2
    );
}
#[test]
fn empty_directory_and_non_standard_report_names_are_not_green() {
    let temp = Temp::new();
    assert_eq!(code(&compare(&temp.0, &fixture("pass.xml"), &[])), 2);
    temp.write("results.xml", pass());
    assert_eq!(code(&compare(&temp.0, &fixture("pass.xml"), &[])), 2);
    // Explicit files are accepted regardless of filename.
    assert_eq!(
        code(&compare(
            &temp.0.join("results.xml"),
            &fixture("pass.xml"),
            &[]
        )),
        0
    );
}
#[test]
fn recursive_scope_distinguishes_modules_and_ignores_report_filename() {
    let baseline = Temp::new();
    let candidate = Temp::new();
    for module in ["module-a", "module-b"] {
        baseline.write(&format!("{module}/TEST-old.xml"), pass());
        candidate.write(&format!("{module}/TEST-new.xml"), pass());
    }
    let first = compare(&baseline.0, &candidate.0, &["--format", "json"]);
    assert_eq!(code(&first), 0);
    let second = compare(&baseline.0, &candidate.0, &["--format", "json"]);
    assert_eq!(first.stdout, second.stdout);
    // Moving a report to a different module changes its identity.
    fs::rename(candidate.0.join("module-b"), candidate.0.join("module-c")).unwrap();
    assert_ne!(code(&compare(&baseline.0, &candidate.0, &[])), 0);
}
#[test]
fn duplicate_identity_across_files_is_not_green() {
    let temp = Temp::new();
    temp.write("TEST-one.xml", pass());
    temp.write("TEST-two.xml", pass());
    assert_eq!(code(&compare(&temp.0, &temp.0, &[])), 2);
}
#[test]
fn explicit_mapping_accounts_for_renamed_tests() {
    let mapping = fixture("mapping.json");
    let out = compare(
        &fixture("pass.xml"),
        &fixture("renamed.xml"),
        &["--mapping", mapping.to_str().unwrap(), "--format", "json"],
    );
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stdout));
    assert_ne!(
        code(&compare(&fixture("pass.xml"), &fixture("renamed.xml"), &[])),
        0
    );
}
#[test]
fn bad_mapping_is_human_readable_and_does_not_echo_data() {
    let temp = Temp::new();
    let mapping = temp.write("mapping.json", "PRIVATE_SECRET_INVALID_JSON");
    let out = compare(
        &fixture("pass.xml"),
        &fixture("pass.xml"),
        &["--mapping", mapping.to_str().unwrap()],
    );
    assert_eq!(code(&out), 2);
    assert!(out.stdout.is_empty());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("invalid mapping JSON"));
    assert!(!err.contains("PRIVATE_SECRET"));
}
#[test]
fn optional_jacoco_requires_both_inputs() {
    let path = fixture("jacoco.xml");
    let path = path.to_str().unwrap();
    for flag in ["--baseline-jacoco", "--candidate-jacoco"] {
        assert_eq!(
            code(&compare(
                &fixture("pass.xml"),
                &fixture("pass.xml"),
                &[flag, path]
            )),
            2
        );
    }
    assert_eq!(
        code(&compare(
            &fixture("pass.xml"),
            &fixture("pass.xml"),
            &["--baseline-jacoco", path, "--candidate-jacoco", path]
        )),
        0
    );
}
#[test]
fn invalid_jacoco_is_inconclusive() {
    let path = fixture("pass.xml");
    assert_eq!(
        code(&compare(
            &path,
            &path,
            &[
                "--baseline-jacoco",
                path.to_str().unwrap(),
                "--candidate-jacoco",
                path.to_str().unwrap()
            ]
        )),
        2
    );
}
#[test]
fn paths_with_spaces_and_unicode_work() {
    let temp = Temp::new();
    let path = temp.write("folder with spaces/測試 report.xml", pass());
    assert_eq!(code(&compare(&path, &path, &[])), 0);
}
#[test]
fn oversized_file_is_rejected_without_reading_it_all() {
    let temp = Temp::new();
    let path = temp.write("large.xml", "");
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(16 * 1024 * 1024 + 1)
        .unwrap();
    let out = compare(&path, &fixture("pass.xml"), &[]);
    assert_eq!(code(&out), 2);
    assert!(String::from_utf8_lossy(&out.stdout).contains("file size limit"));
}
#[test]
fn too_many_files_and_deep_directories_are_rejected() {
    let temp = Temp::new();
    for i in 0..501 {
        temp.write(&format!("TEST-{i:03}.xml"), pass());
    }
    let out = compare(&temp.0, &fixture("pass.xml"), &[]);
    assert_eq!(code(&out), 2);
    assert!(String::from_utf8_lossy(&out.stdout).contains("file limit"));
    let deep = Temp::new();
    deep.write(&format!("{}TEST-test.xml", "d/".repeat(65)), pass());
    let out = compare(&deep.0, &fixture("pass.xml"), &[]);
    assert_eq!(code(&out), 2);
    assert!(String::from_utf8_lossy(&out.stdout).contains("depth limit"));
}
#[cfg(unix)]
#[test]
fn symlink_files_directories_and_direct_inputs_are_not_followed() {
    use std::os::unix::fs::symlink;
    let temp = Temp::new();
    temp.write("TEST-valid.xml", pass());
    symlink(fixture("pass.xml"), temp.0.join("TEST-link.xml")).unwrap();
    symlink(&temp.0, temp.0.join("loop")).unwrap();
    let out = compare(&temp.0, &temp.0, &[]);
    assert_eq!(code(&out), 2);
    assert!(String::from_utf8_lossy(&out.stdout).contains("symbolic link"));
    assert_eq!(
        code(&compare(
            &temp.0.join("TEST-link.xml"),
            &fixture("pass.xml"),
            &[]
        )),
        2
    );
}

#[test]
fn aggregate_case_limit_is_enforced_across_files() {
    let temp = Temp::new();
    let mut report = String::from("<testsuite name=\"many\">");
    for i in 0..50_001 {
        report.push_str(&format!("<testcase classname=\"C\" name=\"t{i}\"/>"));
    }
    report.push_str("</testsuite>");
    temp.write("TEST-first.xml", &report);
    temp.write("TEST-second.xml", &report);
    let out = compare(&temp.0, &fixture("pass.xml"), &[]);
    assert_eq!(code(&out), 2);
    assert!(String::from_utf8_lossy(&out.stdout).contains("aggregate test case limit"));
}

#[test]
fn aggregate_bytes_include_all_reports_and_optional_coverage() {
    let temp = Temp::new();
    let header = "<testsuite name=\"padding\"><testcase classname=\"C\" name=\"t\"/><!--";
    let trailer = "--></testsuite>";
    let mut report = header.as_bytes().to_vec();
    report.resize(16 * 1024 * 1024 - trailer.len(), b' ');
    report.extend_from_slice(trailer.as_bytes());
    for i in 0..9 {
        temp.write(&format!("m{i}/TEST-large.xml"), &report);
    }
    let out = compare(&temp.0, &fixture("pass.xml"), &[]);
    assert_eq!(code(&out), 2);
    assert!(String::from_utf8_lossy(&out.stdout).contains("aggregate byte limit"));
}

#[test]
fn mapping_missing_file_and_directory_fail_cleanly() {
    let temp = Temp::new();
    for path in [&temp.0, &temp.0.join("missing.json")] {
        let out = compare(
            &fixture("pass.xml"),
            &fixture("pass.xml"),
            &["--mapping", path.to_str().unwrap()],
        );
        assert_eq!(code(&out), 2);
        assert!(String::from_utf8_lossy(&out.stderr).starts_with("error: "));
    }
}

#[cfg(unix)]
#[test]
fn non_utf8_paths_and_non_regular_inputs_fail_closed() {
    use std::os::unix::{ffi::OsStringExt, net::UnixListener};
    let temp = Temp::new();
    let filename = std::ffi::OsString::from_vec(vec![
        b'T', b'E', b'S', b'T', b'-', 255, b'.', b'x', b'm', b'l',
    ]);
    fs::write(temp.0.join(filename), pass()).unwrap();
    assert_eq!(code(&compare(&temp.0, &fixture("pass.xml"), &[])), 2);
    let other = Temp::new();
    other.write("TEST-good.xml", pass());
    let path = other.0.join("socket");
    let Ok(_listener) = UnixListener::bind(&path) else {
        // Some sandboxed runners disallow AF_UNIX sockets. Still exercise a
        // non-regular direct input without requiring external commands.
        assert_eq!(
            code(&compare(Path::new("/dev/null"), &fixture("pass.xml"), &[])),
            2
        );
        return;
    };
    assert_eq!(code(&compare(&other.0, &fixture("pass.xml"), &[])), 2);
    assert_eq!(code(&compare(&path, &fixture("pass.xml"), &[])), 2);
}

#[cfg(unix)]
#[test]
fn output_write_error_is_reported() {
    use std::process::Stdio;
    let full = fs::OpenOptions::new().write(true).open("/dev/full");
    if let Ok(full) = full {
        let out = cli()
            .arg("compare")
            .arg("--baseline")
            .arg(fixture("pass.xml"))
            .arg("--candidate")
            .arg(fixture("pass.xml"))
            .stdout(Stdio::from(full))
            .output()
            .unwrap();
        assert_eq!(code(&out), 2);
        assert!(String::from_utf8_lossy(&out.stderr).contains("cannot write output"));
    }
}

#[test]
fn excessive_mapping_entries_are_rejected() {
    let temp = Temp::new();
    let entry = r#"{"baseline":{"scope":"","id":{"suite":[],"classname":"C","name":"t"}},"candidate":{"scope":"","id":{"suite":[],"classname":"C","name":"t"}}}"#;
    let body = format!("[{}]", vec![entry; 100_001].join(","));
    let path = temp.write("mapping.json", body);
    let out = compare(
        &fixture("pass.xml"),
        &fixture("pass.xml"),
        &["--mapping", path.to_str().unwrap()],
    );
    assert_eq!(code(&out), 2);
    assert!(String::from_utf8_lossy(&out.stderr).contains("mapping entry limit"));
}

#[cfg(unix)]
#[test]
fn unreadable_inputs_and_symlink_auxiliary_files_are_rejected() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let temp = Temp::new();
    let path = temp.write("blocked.xml", pass());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o0)).unwrap();
    // Root can bypass mode permissions, so assert only when denial is effective.
    if fs::File::open(&path).is_err() {
        assert_eq!(code(&compare(&path, &fixture("pass.xml"), &[])), 2);
    }
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let dir = temp.0.join("blocked");
    fs::create_dir(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o0)).unwrap();
    if fs::read_dir(&dir).is_err() {
        assert_eq!(code(&compare(&dir, &fixture("pass.xml"), &[])), 2);
    }
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    let mapping = temp.0.join("mapping-link.json");
    symlink(fixture("mapping.json"), &mapping).unwrap();
    let out = compare(
        &fixture("pass.xml"),
        &fixture("pass.xml"),
        &["--mapping", mapping.to_str().unwrap()],
    );
    assert_eq!(code(&out), 2);
    assert!(String::from_utf8_lossy(&out.stderr).contains("symbolic links"));
}

#[cfg(unix)]
#[test]
fn non_utf8_directory_scope_is_rejected() {
    use std::os::unix::ffi::OsStringExt;
    let temp = Temp::new();
    let dir = temp.0.join(std::ffi::OsString::from_vec(vec![255]));
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("TEST-case.xml"), pass()).unwrap();
    assert_eq!(code(&compare(&temp.0, &fixture("pass.xml"), &[])), 2);
}

#[test]
fn rejected_report_diagnostics_identify_relative_source_without_absolute_path() {
    let temp = Temp::new();
    temp.write("module/TEST-broken.xml", "<broken/>");
    let out = compare(&temp.0, &fixture("pass.xml"), &["--format", "json"]);
    assert_eq!(code(&out), 2);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("module/TEST-broken.xml"));
    assert!(!text.contains(temp.0.to_str().unwrap()));
    let warning = temp.write(
        "TEST-warning.xml",
        "<testsuite name=\"S\" tests=\"2\"><testcase classname=\"C\" name=\"t\"/></testsuite>",
    );
    let out = compare(&warning, &fixture("pass.xml"), &["--format", "json"]);
    assert_eq!(code(&out), 2);
    assert!(String::from_utf8_lossy(&out.stdout).contains("TEST-warning.xml"));
}
