//! Bounded, UTF-8-only parsers for test evidence. Output never contains captured
//! stdout/stderr, exception bodies, or failure messages.
use quick_xml::{
    Reader,
    events::{BytesStart, Event},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};

pub const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_XML_DEPTH: usize = 128;
pub const MAX_TEST_CASES: usize = 100_000;
pub const MAX_IDENTITY_BYTES: usize = 16_384;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestId {
    pub suite: Vec<String>,
    pub classname: String,
    pub name: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Passed,
    Failed,
    Error,
    Skipped,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TestCase {
    pub id: TestId,
    pub status: Status,
    pub duration_seconds: Option<f64>,
    /// Number of failed attempts explicitly recorded by retry extensions.
    /// A final pass with retries is not clean evidence of equivalence.
    pub retry_failures: u32,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    pub cases: Vec<TestCase>,
    pub warnings: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Counter {
    pub missed: u64,
    pub covered: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Coverage {
    pub lines: Counter,
    pub branches: Option<Counter>,
    /// Sorted package-qualified class names. Equality is only a scope check,
    /// not proof that source code or instrumentation is equivalent.
    pub classes: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(pub String);
impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl Error for ParseError {}
fn err(message: &str) -> ParseError {
    ParseError(message.to_owned())
}

fn attributes(e: &BytesStart<'_>) -> Result<BTreeMap<String, String>, ParseError> {
    let mut out = BTreeMap::new();
    for a in e.attributes() {
        let a = a.map_err(|_| err("malformed or duplicate XML attribute"))?;
        let key = a.key.as_ref();
        let value = a
            .normalized_value(quick_xml::XmlVersion::Explicit1_0)
            .map_err(|_| err("invalid XML attribute entity"))?;
        if !value.chars().all(valid_xml_char) {
            return Err(err("invalid XML character in attribute"));
        }
        out.insert(key.to_owned(), value.into_owned());
    }
    Ok(out)
}
fn identity(a: &BTreeMap<String, String>, key: &str) -> Result<String, ParseError> {
    let value = a
        .get(key)
        .ok_or_else(|| err(&format!("missing identity attribute: {key}")))?;
    if value.trim().is_empty() || value.len() > MAX_IDENTITY_BYTES {
        return Err(err(&format!(
            "empty or oversized identity attribute: {key}"
        )));
    }
    Ok(value.clone())
}
fn valid_xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')
}
fn number(s: &str) -> Result<u64, ParseError> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(err("invalid nonnegative integer count"));
    }
    s.parse().map_err(|_| err("integer count overflow"))
}
fn duration(a: &BTreeMap<String, String>) -> Result<Option<f64>, ParseError> {
    a.get("time")
        .map(|s| {
            let n: f64 = s.parse().map_err(|_| err("invalid testcase duration"))?;
            if !n.is_finite() || n < 0.0 {
                return Err(err("duration must be finite and nonnegative"));
            }
            Ok(n)
        })
        .transpose()
}
fn counts(a: &BTreeMap<String, String>) -> Result<(), ParseError> {
    for key in [
        "tests",
        "failures",
        "errors",
        "skipped",
        "disabled",
        "assertions",
    ] {
        if let Some(s) = a.get(key) {
            number(s)?;
        }
    }
    Ok(())
}

/// Parse JUnit/Surefire test cases. Missing durations remain unknown, never zero.
/// Duplicate identities are rejected rather than silently merged.
pub fn parse_junit(bytes: &[u8]) -> Result<Report, ParseError> {
    parse_junit_limits(bytes, MAX_FILE_BYTES, MAX_XML_DEPTH, MAX_TEST_CASES)
}
fn parse_junit_limits(
    bytes: &[u8],
    byte_limit: usize,
    depth_limit: usize,
    case_limit: usize,
) -> Result<Report, ParseError> {
    let mut state = JunitState {
        stack: vec![],
        suites: vec![],
        current: None,
        retry_kind: None,
        cases: vec![],
        seen: BTreeSet::new(),
        case_limit,
        warnings: vec![],
        declarations: vec![],
    };
    walk(
        bytes,
        byte_limit,
        depth_limit,
        false,
        |start, name, attrs| {
            if start {
                state.start(name, attrs.unwrap())
            } else {
                state.end(name)
            }
        },
    )?;
    Ok(Report {
        cases: state.cases,
        warnings: state.warnings,
    })
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum RetryKind {
    Flaky,
    Rerun,
}

struct JunitState {
    stack: Vec<String>,
    suites: Vec<String>,
    current: Option<TestCase>,
    retry_kind: Option<RetryKind>,
    cases: Vec<TestCase>,
    seen: BTreeSet<TestId>,
    case_limit: usize,
    warnings: Vec<String>,
    declarations: Vec<(usize, BTreeMap<String, String>)>,
}
impl JunitState {
    fn start(&mut self, name: &str, a: BTreeMap<String, String>) -> Result<(), ParseError> {
        let parent = self.stack.last().map(String::as_str);
        if parent.is_none() && !matches!(name, "testsuite" | "testsuites") {
            return Err(err("expected testsuite or testsuites root"));
        }
        match name {
            "testsuites" => {
                if parent.is_some() {
                    return Err(err("nested testsuites container is unsupported"));
                }
                counts(&a)?;
                duration(&a)?;
                self.declarations.push((self.cases.len(), a.clone()));
            }
            "testsuite" => {
                if !matches!(parent, None | Some("testsuite" | "testsuites")) {
                    return Err(err("testsuite in unsupported location"));
                }
                counts(&a)?;
                duration(&a)?;
                self.suites.push(identity(&a, "name")?);
                self.declarations.push((self.cases.len(), a.clone()));
            }
            "testcase" => {
                if parent != Some("testsuite") || self.current.is_some() {
                    return Err(err("testcase must be a direct child of testsuite"));
                }
                if self.cases.len() >= self.case_limit {
                    return Err(err("test case limit exceeded"));
                }
                counts(&a)?;
                let id = TestId {
                    suite: self.suites.clone(),
                    classname: identity(&a, "classname")?,
                    name: identity(&a, "name")?,
                };
                if !self.seen.insert(id.clone()) {
                    return Err(err("duplicate test identity"));
                }
                self.retry_kind = None;
                self.current = Some(TestCase {
                    id,
                    status: Status::Passed,
                    duration_seconds: duration(&a)?,
                    retry_failures: 0,
                });
            }
            "failure" | "error" | "skipped" if parent == Some("testcase") => {
                let c = self
                    .current
                    .as_mut()
                    .ok_or_else(|| err("test result without testcase"))?;
                let status = match name {
                    "failure" => Status::Failed,
                    "error" => Status::Error,
                    _ => Status::Skipped,
                };
                if (c.status == Status::Skipped && status != Status::Skipped)
                    || (status == Status::Skipped && c.status != Status::Passed)
                {
                    return Err(err("contradictory skipped and failed testcase statuses"));
                }
                // Multiple failure/error elements can represent multiple assertions.
                if c.status != Status::Error {
                    c.status = status;
                }
            }
            "flakyFailure" | "flakyError" | "rerunFailure" | "rerunError"
                if parent == Some("testcase") =>
            {
                let kind = if matches!(name, "flakyFailure" | "flakyError") {
                    RetryKind::Flaky
                } else {
                    RetryKind::Rerun
                };
                if self.retry_kind.is_some_and(|previous| previous != kind) {
                    return Err(err("mixed flaky and rerun result families"));
                }
                self.retry_kind = Some(kind);
                let c = self
                    .current
                    .as_mut()
                    .ok_or_else(|| err("retry without testcase"))?;
                c.retry_failures = c
                    .retry_failures
                    .checked_add(1)
                    .ok_or_else(|| err("retry count overflow"))?;
            }
            "failure" | "error" | "skipped" | "flakyFailure" | "flakyError" | "rerunFailure"
            | "rerunError" => {
                return Err(err(
                    "test result element must be a direct child of testcase",
                ));
            }
            _ => {} // Other vendor extensions are ignored; no bodies enter the result.
        }
        self.stack.push(name.to_owned());
        Ok(())
    }
    fn end(&mut self, name: &str) -> Result<(), ParseError> {
        match name {
            "testcase" => {
                let case = self
                    .current
                    .take()
                    .ok_or_else(|| err("testcase end without start"))?;
                // Surefire represents eventual success with flaky* and all-failed
                // attempts with failure/error plus rerun*. These are distinct dialect states.
                match self.retry_kind.take() {
                    Some(RetryKind::Flaky) if case.status != Status::Passed => {
                        return Err(err(
                            "flaky retry markers contradict terminal testcase status",
                        ));
                    }
                    Some(RetryKind::Rerun)
                        if !matches!(case.status, Status::Failed | Status::Error) =>
                    {
                        return Err(err("rerun markers require terminal failure or error"));
                    }
                    _ => {}
                }
                self.cases.push(case);
            }
            "testsuite" | "testsuites" => {
                if let Some((first, declared)) = self.declarations.pop() {
                    let cases = &self.cases[first..];
                    for (key, actual) in [
                        ("tests", cases.len()),
                        (
                            "failures",
                            cases.iter().filter(|c| c.status == Status::Failed).count(),
                        ),
                        (
                            "errors",
                            cases.iter().filter(|c| c.status == Status::Error).count(),
                        ),
                        (
                            "skipped",
                            cases.iter().filter(|c| c.status == Status::Skipped).count(),
                        ),
                    ] {
                        if let Some(value) = declared.get(key)
                            && number(value)? != actual as u64
                        {
                            self.warnings.push(format!(
                                "declared {key} count {value} differs from parsed count {actual}"
                            ));
                        }
                    }
                }
                if name == "testsuite" {
                    self.suites.pop();
                }
            }
            _ => {}
        }
        self.stack.pop();
        Ok(())
    }
}

/// Parse only the report-level JaCoCo LINE counter (never sum nested counters).
/// The exact standard external JaCoCo declaration is inert; no DTD is resolved.
pub fn parse_jacoco(bytes: &[u8]) -> Result<Coverage, ParseError> {
    let mut stack = Vec::<String>::new();
    let mut lines = None;
    let mut branches = None;
    let mut classes = BTreeSet::new();
    walk(
        bytes,
        MAX_FILE_BYTES,
        MAX_XML_DEPTH,
        true,
        |start, name, attrs| {
            if start {
                let a = attrs.unwrap();
                if stack.is_empty() && name != "report" {
                    return Err(err("expected JaCoCo report root"));
                }
                if name == "class" && stack.last().map(String::as_str) == Some("package") {
                    if classes.len() >= MAX_TEST_CASES {
                        return Err(err("coverage class limit exceeded"));
                    }
                    if !classes.insert(identity(&a, "name")?) {
                        return Err(err("duplicate coverage class"));
                    }
                }
                if name == "counter" && stack.len() == 1 {
                    let counter_type = a.get("type").map(String::as_str);
                    if matches!(counter_type, Some("LINE" | "BRANCH")) {
                        let target = if counter_type == Some("LINE") {
                            &mut lines
                        } else {
                            &mut branches
                        };
                        if target.is_some() {
                            return Err(err("duplicate report coverage counter"));
                        }
                        let missed = number(
                            a.get("missed")
                                .ok_or_else(|| err("missing coverage missed count"))?,
                        )?;
                        let covered = number(
                            a.get("covered")
                                .ok_or_else(|| err("missing coverage covered count"))?,
                        )?;
                        missed
                            .checked_add(covered)
                            .ok_or_else(|| err("coverage total overflow"))?;
                        *target = Some(Counter { missed, covered });
                    }
                }
                stack.push(name.to_owned());
            } else {
                stack.pop();
            }
            Ok(())
        },
    )?;
    let lines = lines.ok_or_else(|| err("missing report-level LINE counter"))?;
    Ok(Coverage {
        lines,
        branches,
        classes: classes.into_iter().collect(),
    })
}

/// Shared bounded well-formedness envelope. The parser does not resolve URLs,
/// files, external entities, or DTDs. UTF-8 is the only accepted encoding.
fn walk(
    bytes: &[u8],
    byte_limit: usize,
    depth_limit: usize,
    allow_jacoco_doctype: bool,
    mut visitor: impl FnMut(bool, &str, Option<BTreeMap<String, String>>) -> Result<(), ParseError>,
) -> Result<(), ParseError> {
    if bytes.len() > byte_limit {
        return Err(err("XML byte limit exceeded"));
    }
    let source = std::str::from_utf8(bytes).map_err(|_| err("XML must be UTF-8"))?;
    if !source.chars().all(valid_xml_char) {
        return Err(err("invalid XML character"));
    }
    let mut reader = Reader::from_str(source);
    reader.config_mut().check_comments = true;
    let mut stack = Vec::<String>::new();
    let mut seen_root = false;
    let mut seen_declaration = false;
    let mut seen_doctype = false;
    loop {
        let event = reader.read_event().map_err(|_| err("malformed XML"))?;
        let empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(e) | Event::Empty(e) => {
                if stack.is_empty() {
                    if seen_root {
                        return Err(err("multiple XML roots"));
                    }
                    seen_root = true;
                }
                if stack.len() >= depth_limit {
                    return Err(err("XML depth limit exceeded"));
                }
                let name = e.name().as_ref().to_owned();
                let a = attributes(&e)?;
                visitor(true, &name, Some(a))?;
                if empty {
                    visitor(false, &name, None)?;
                } else {
                    stack.push(name);
                }
            }
            Event::End(e) => {
                let name = e.name().as_ref().to_owned();
                if stack.pop().as_deref() != Some(name.as_str()) {
                    return Err(err("mismatched XML closing tag"));
                }
                visitor(false, &name, None)?;
            }
            Event::DocType(d) => {
                let declaration = d.as_ref();
                let normalized = declaration.split_whitespace().collect::<Vec<_>>().join(" ");
                if !allow_jacoco_doctype
                    || seen_root
                    || seen_doctype
                    || normalized
                        != "report PUBLIC \"-//JACOCO//DTD Report 1.1//EN\" \"report.dtd\""
                {
                    return Err(err(
                        "DOCTYPE is forbidden (except the inert standard JaCoCo declaration)",
                    ));
                }
                seen_doctype = true;
            }
            Event::Decl(d) => {
                if seen_root || seen_declaration || seen_doctype {
                    return Err(err("misplaced or repeated XML declaration"));
                }
                if d.version()
                    .map_err(|_| err("invalid XML declaration"))?
                    .as_ref()
                    != "1.0"
                {
                    return Err(err("only XML version 1.0 is supported"));
                }
                seen_declaration = true;
                if let Some(enc) = d.encoding() {
                    let enc = enc.map_err(|_| err("invalid XML encoding declaration"))?;
                    if !enc.eq_ignore_ascii_case("utf-8") {
                        return Err(err("XML encoding must be UTF-8"));
                    }
                }
            }
            Event::Text(t) => {
                if stack.is_empty() && !t.as_ref().bytes().all(|b| b.is_ascii_whitespace()) {
                    return Err(err("text outside XML root"));
                }
            }
            Event::CData(_) | Event::GeneralRef(_) if stack.is_empty() => {
                return Err(err("data outside XML root"));
            }
            Event::GeneralRef(r) => {
                let n = r.as_ref();
                let escaped = format!("&{n};");
                let value = quick_xml::escape::unescape(&escaped)
                    .map_err(|_| err("unknown or malformed XML entity"))?;
                if !value.chars().all(valid_xml_char) {
                    return Err(err("invalid XML character reference"));
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !seen_root || !stack.is_empty() {
        return Err(err("empty or truncated XML document"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/unit/report.rs"]
mod tests;
