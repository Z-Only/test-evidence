//! Deterministic, bounded, local-only input loading.
use crate::compare::{IdentityMapping, ScopedCase, Snapshot};
use crate::report::{parse_jacoco, parse_junit};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const MAX_FILES: usize = 500;
pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_DIRECTORY_DEPTH: usize = 64;
pub const MAX_CASES: usize = 100_000;
// Also bound directories and non-report entries, so discovery itself is bounded.
const MAX_ENTRIES: usize = 100_000;

#[derive(Default)]
struct Budget {
    bytes: u64,
    entries: usize,
}

/// A direct file can have any filename. Directories select only TEST-*.xml files.
/// Scope is the slash-separated relative parent directory, or empty at the root.
pub fn load_snapshot(path: &Path, jacoco: Option<&Path>) -> Snapshot {
    let mut snapshot = Snapshot::default();
    let mut budget = Budget::default();
    let mut files = Vec::new();
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => snapshot
            .errors
            .push("input is a symbolic link; symbolic links are not followed".into()),
        Ok(meta) if meta.is_file() => files.push((path.to_path_buf(), String::new())),
        Ok(meta) if meta.is_dir() => {
            if let Err(e) = discover(
                path,
                path,
                0,
                &mut budget,
                &mut files,
                &mut snapshot.warnings,
            ) {
                snapshot.errors.push(e);
            }
        }
        Ok(_) => snapshot
            .errors
            .push("input must be a regular file or directory".into()),
        Err(e) => snapshot
            .errors
            .push(format!("cannot inspect input: {}", e.kind())),
    }
    if files.is_empty() {
        snapshot
            .errors
            .push("no test reports found (directories require TEST-*.xml filenames)".into());
    }
    for (file, scope) in files {
        let name = file.file_name().unwrap_or_default().to_string_lossy();
        let locator = if scope.is_empty() {
            name.into_owned()
        } else {
            format!("{scope}/{name}")
        };
        match read_bounded(&file, &mut budget)
            .and_then(|bytes| parse_junit(&bytes).map_err(|e| e.to_string()))
        {
            Ok(report) => {
                snapshot.warnings.extend(
                    report
                        .warnings
                        .into_iter()
                        .map(|warning| format!("report {locator:?}: {warning}")),
                );
                if snapshot.cases.len().saturating_add(report.cases.len()) > MAX_CASES {
                    snapshot
                        .errors
                        .push("aggregate test case limit exceeded (100000)".into());
                    break;
                }
                if report.cases.is_empty() {
                    snapshot
                        .errors
                        .push(format!("report {locator:?} contains no test cases"));
                }
                snapshot
                    .cases
                    .extend(report.cases.into_iter().map(|case| ScopedCase {
                        scope: scope.clone(),
                        case,
                    }));
            }
            Err(error) => snapshot
                .errors
                .push(format!("test report {locator:?} rejected: {error}")),
        }
    }
    if let Some(file) = jacoco {
        match read_bounded(file, &mut budget)
            .and_then(|bytes| parse_jacoco(&bytes).map_err(|e| e.to_string()))
        {
            Ok(coverage) => {
                snapshot.coverage.insert(String::new(), coverage);
            }
            Err(error) => snapshot
                .errors
                .push(format!("JaCoCo report rejected: {error}")),
        }
    }
    snapshot
}

fn discover(
    root: &Path,
    directory: &Path,
    depth: usize,
    budget: &mut Budget,
    files: &mut Vec<(PathBuf, String)>,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    if depth > MAX_DIRECTORY_DEPTH {
        return Err("directory depth limit exceeded (64)".into());
    }
    let entries = fs::read_dir(directory)
        .map_err(|e| format!("cannot read input directory: {}", e.kind()))?;
    let mut paths = Vec::new();
    for entry in entries {
        budget.entries += 1;
        if budget.entries > MAX_ENTRIES {
            return Err("directory entry limit exceeded (100000)".into());
        }
        paths.push(
            entry
                .map_err(|e| format!("cannot read directory entry: {}", e.kind()))?
                .path(),
        );
    }
    paths.sort();
    for path in paths {
        let meta = fs::symlink_metadata(&path)
            .map_err(|e| format!("cannot inspect directory entry: {}", e.kind()))?;
        if meta.file_type().is_symlink() {
            warnings.push(
                "symbolic link omitted from input directory; evidence may be incomplete".into(),
            );
        } else if meta.is_dir() {
            discover(root, &path, depth + 1, budget, files, warnings)?;
        } else if meta.is_file() {
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                return Err("non-UTF-8 filename is unsupported".into());
            };
            if !name.starts_with("TEST-") || !name.ends_with(".xml") {
                continue;
            }
            if files.len() >= MAX_FILES {
                return Err("test report file limit exceeded (500)".into());
            }
            let relative = path
                .parent()
                .unwrap_or(root)
                .strip_prefix(root)
                .map_err(|_| "invalid relative report scope")?;
            let scope = relative
                .components()
                .map(|c| {
                    c.as_os_str()
                        .to_str()
                        .ok_or("non-UTF-8 directory name is unsupported")
                })
                .collect::<Result<Vec<_>, _>>()?
                .join("/");
            files.push((path, scope));
        } else {
            warnings.push(
                "non-regular entry omitted from input directory; evidence may be incomplete".into(),
            );
        }
    }
    Ok(())
}

fn read_bounded(path: &Path, budget: &mut Budget) -> Result<Vec<u8>, String> {
    let meta =
        fs::symlink_metadata(path).map_err(|e| format!("cannot inspect file: {}", e.kind()))?;
    if meta.file_type().is_symlink() {
        return Err("symbolic links are not followed".into());
    }
    if !meta.is_file() {
        return Err("expected a regular file".into());
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err("file size limit exceeded (16 MiB)".into());
    }
    if meta.len() > MAX_TOTAL_BYTES.saturating_sub(budget.bytes) {
        return Err("aggregate byte limit exceeded (128 MiB)".into());
    }
    let file = File::open(path).map_err(|e| format!("cannot open file: {}", e.kind()))?;
    // Bound actual reads as well as metadata, including files that grow during a read.
    let remaining = MAX_FILE_BYTES.min(MAX_TOTAL_BYTES.saturating_sub(budget.bytes));
    let mut bytes = Vec::new();
    file.take(remaining + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read file: {}", e.kind()))?;
    budget.bytes += bytes.len() as u64;
    if bytes.len() as u64 > remaining {
        return Err("file or aggregate byte limit exceeded during read".into());
    }
    Ok(bytes)
}

/// Mapping JSON is an array of explicit baseline/candidate scoped identities.
pub fn load_mapping(path: &Path) -> Result<Vec<IdentityMapping>, String> {
    let bytes = read_bounded(path, &mut Budget::default())?;
    let mappings: Vec<IdentityMapping> = serde_json::from_slice(&bytes).map_err(|e| {
        format!(
            "invalid mapping JSON at line {}, column {}",
            e.line(),
            e.column()
        )
    })?;
    if mappings.len() > MAX_CASES {
        return Err("mapping entry limit exceeded (100000)".into());
    }
    Ok(mappings)
}
