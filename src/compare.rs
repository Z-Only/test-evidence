//! Conservative deterministic accounting; no fuzzy identity matching.
use crate::report::{Counter, Coverage, Status, TestCase, TestId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopedId {
    pub scope: String,
    pub id: TestId,
}
#[derive(Debug, Clone)]
pub struct ScopedCase {
    pub scope: String,
    pub case: TestCase,
}
#[derive(Debug, Default)]
pub struct Snapshot {
    pub cases: Vec<ScopedCase>,
    pub coverage: BTreeMap<String, Coverage>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityMapping {
    pub baseline: ScopedId,
    pub candidate: ScopedId,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Accepted,
    PolicyFindings,
    Inconclusive,
}
impl Verdict {
    pub fn exit_code(self) -> i32 {
        match self {
            Self::Accepted => 0,
            Self::PolicyFindings => 1,
            Self::Inconclusive => 2,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    MissingInCandidate,
    NewlySkipped,
    PassedToFailure,
    PassedToError,
    CandidateFailure,
    NewlyAdded,
    Improved,
    RetryNotice,
    CoverageDecrease,
}
#[derive(Debug, Clone, Serialize)]
pub struct Observation {
    pub status: Status,
    pub duration_seconds: Option<f64>,
    pub retry_failures: u32,
}
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub kind: FindingKind,
    pub baseline: Option<ScopedId>,
    pub candidate: Option<ScopedId>,
    pub scope: Option<String>,
    pub policy_finding: bool,
    pub baseline_observation: Option<Observation>,
    pub candidate_observation: Option<Observation>,
}
#[derive(Debug, Default, Serialize)]
pub struct DurationSummary {
    pub reported_cases: usize,
    pub sample_count: usize,
    pub excluded_missing: usize,
    pub excluded_skipped: usize,
    pub excluded_retried: usize,
    pub excluded_invalid: usize,
    pub p50_seconds: Option<f64>,
    pub p95_seconds: Option<f64>,
}
#[derive(Debug, Serialize)]
pub struct Durations {
    pub baseline: DurationSummary,
    pub candidate: DurationSummary,
    pub matched_baseline: DurationSummary,
    pub matched_candidate: DurationSummary,
    pub matched_pairs: usize,
    pub matched_eligible_pairs: usize,
}
#[derive(Debug, Serialize)]
pub struct CounterSummary {
    pub missed: u64,
    pub covered: u64,
    pub total: u64,
    pub percent: Option<f64>,
}
#[derive(Debug, Serialize)]
pub struct CounterComparison {
    pub baseline: CounterSummary,
    pub candidate: CounterSummary,
    pub percentage_point_delta: Option<f64>,
    pub same_denominator: bool,
}
#[derive(Debug, Serialize)]
pub struct CoverageComparison {
    pub scope: String,
    pub same_reported_classes: bool,
    pub numerically_comparable: bool,
    pub lines: Option<CounterComparison>,
    pub branches: Option<CounterComparison>,
}
#[derive(Debug, Serialize)]
pub struct Comparison {
    pub schema_version: u32,
    pub verdict: Verdict,
    pub baseline_cases: usize,
    pub candidate_cases: usize,
    pub matched_cases: usize,
    pub mappings_applied: usize,
    pub findings: Vec<Finding>,
    pub diagnostics: Vec<String>,
    pub durations: Durations,
    pub coverage: Vec<CoverageComparison>,
    pub limitations: Vec<String>,
}
fn index<'a>(
    s: &'a Snapshot,
    label: &str,
    d: &mut Vec<String>,
) -> BTreeMap<ScopedId, &'a TestCase> {
    let mut out = BTreeMap::new();
    for c in &s.cases {
        let id = ScopedId {
            scope: c.scope.clone(),
            id: c.case.id.clone(),
        };
        match out.entry(id) {
            std::collections::btree_map::Entry::Occupied(entry) => d.push(format!(
                "{label}: duplicate identity {}",
                serde_json::to_string(entry.key()).expect("identity serialization")
            )),
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(&c.case);
            }
        }
    }
    for m in s.warnings.iter().chain(&s.errors) {
        d.push(format!("{label}: {m}"));
    }
    if s.cases.is_empty() {
        d.push(format!(
            "{label}: no reported testcases; completeness cannot be established"
        ));
    }
    out
}
fn eligible(c: &TestCase) -> bool {
    c.status != Status::Skipped
        && c.retry_failures == 0
        && c.duration_seconds
            .is_some_and(|v| v.is_finite() && v >= 0.0)
}
fn durations<'a>(cases: impl IntoIterator<Item = &'a TestCase>) -> DurationSummary {
    let mut s = DurationSummary::default();
    let mut v = Vec::new();
    for c in cases {
        s.reported_cases += 1;
        if c.status == Status::Skipped {
            s.excluded_skipped += 1;
        } else if c.retry_failures > 0 {
            s.excluded_retried += 1;
        } else {
            match c.duration_seconds {
                None => s.excluded_missing += 1,
                Some(d) if d.is_finite() && d >= 0.0 => v.push(d),
                Some(_) => s.excluded_invalid += 1,
            }
        }
    }
    v.sort_by(f64::total_cmp);
    s.sample_count = v.len();
    if !v.is_empty() {
        s.p50_seconds = Some(v[(v.len() as f64 * 0.5).ceil() as usize - 1]);
        s.p95_seconds = Some(v[(v.len() as f64 * 0.95).ceil() as usize - 1]);
    }
    s
}
fn counter(c: &Counter) -> CounterSummary {
    let total = c.missed.saturating_add(c.covered);
    CounterSummary {
        missed: c.missed,
        covered: c.covered,
        total,
        percent: (total > 0).then(|| c.covered as f64 / total as f64 * 100.0),
    }
}
fn counters(a: &Counter, b: &Counter) -> CounterComparison {
    let a = counter(a);
    let b = counter(b);
    CounterComparison {
        same_denominator: a.total == b.total,
        percentage_point_delta: if a.total == b.total && a.total > 0 {
            let magnitude = b.covered.abs_diff(a.covered) as f64 / a.total as f64 * 100.0;
            Some(if b.covered < a.covered {
                -magnitude
            } else {
                magnitude
            })
        } else {
            a.percent.zip(b.percent).map(|(a, b)| b - a)
        },
        baseline: a,
        candidate: b,
    }
}
fn finding(
    kind: FindingKind,
    baseline: Option<ScopedId>,
    candidate: Option<ScopedId>,
    policy_finding: bool,
) -> Finding {
    Finding {
        kind,
        baseline,
        candidate,
        scope: None,
        policy_finding,
        baseline_observation: None,
        candidate_observation: None,
    }
}
fn coverage(
    a: &Snapshot,
    b: &Snapshot,
    d: &mut Vec<String>,
    f: &mut Vec<Finding>,
) -> Vec<CoverageComparison> {
    let scopes: BTreeSet<_> = a.coverage.keys().chain(b.coverage.keys()).collect();
    let mut out = Vec::new();
    for scope in scopes {
        let (Some(l), Some(r)) = (a.coverage.get(scope), b.coverage.get(scope)) else {
            d.push(format!(
                "coverage {scope}: evidence missing from one snapshot"
            ));
            out.push(CoverageComparison {
                scope: scope.clone(),
                same_reported_classes: false,
                numerically_comparable: false,
                lines: None,
                branches: None,
            });
            continue;
        };
        let same_reported_classes =
            l.classes.iter().collect::<BTreeSet<_>>() == r.classes.iter().collect::<BTreeSet<_>>();
        let lines = counters(&l.lines, &r.lines);
        let branches = l
            .branches
            .as_ref()
            .zip(r.branches.as_ref())
            .map(|(a, b)| counters(a, b));
        let numerically_comparable = same_reported_classes
            && lines.same_denominator
            && (l.branches.is_some() == r.branches.is_some())
            && branches.as_ref().is_none_or(|c| c.same_denominator);
        if !numerically_comparable {
            d.push(format!("coverage {scope}: changed reported classes, denominator, or counter availability; comparability unknown"));
        } else if lines.percentage_point_delta.is_some_and(|d| d < 0.0)
            || branches
                .as_ref()
                .is_some_and(|b| b.percentage_point_delta.is_some_and(|d| d < 0.0))
        {
            f.push(Finding {
                kind: FindingKind::CoverageDecrease,
                baseline: None,
                candidate: None,
                scope: Some(scope.clone()),
                policy_finding: true,
                baseline_observation: None,
                candidate_observation: None,
            });
        }
        out.push(CoverageComparison {
            scope: scope.clone(),
            same_reported_classes,
            numerically_comparable,
            lines: Some(lines),
            branches,
        });
    }
    out
}
pub fn compare(
    baseline: &Snapshot,
    candidate: &Snapshot,
    mappings: &[IdentityMapping],
) -> Comparison {
    let mut diagnostics = Vec::new();
    let a = index(baseline, "baseline", &mut diagnostics);
    let b = index(candidate, "candidate", &mut diagnostics);
    let mut remap = BTreeMap::new();
    let mut destinations = BTreeSet::new();
    for m in mappings {
        if !a.contains_key(&m.baseline) || !b.contains_key(&m.candidate) {
            diagnostics.push("identity mapping contains unknown endpoint".into());
        }
        if remap
            .insert(m.baseline.clone(), m.candidate.clone())
            .is_some()
            || !destinations.insert(m.candidate.clone())
        {
            diagnostics.push("identity mappings must be one-to-one; duplicate endpoint".into());
        }
    }
    let mut owners = BTreeMap::new();
    for id in a.keys() {
        let target = remap.get(id).unwrap_or(id);
        if b.contains_key(target) && owners.insert(target, id).is_some() {
            diagnostics.push("identity mapping collides with another exact or mapped match".into());
        }
    }
    let mut findings = Vec::new();
    let mut matched = BTreeSet::new();
    let mut pairs = Vec::new();
    for (id, left) in &a {
        let target = remap.get(id).unwrap_or(id);
        let Some(right) = b.get(target) else {
            findings.push(finding(
                FindingKind::MissingInCandidate,
                Some(id.clone()),
                None,
                true,
            ));
            continue;
        };
        matched.insert(target.clone());
        pairs.push((*left, *right));
        let kind = match (left.status, right.status) {
            (Status::Passed, Status::Failed) => Some((FindingKind::PassedToFailure, true)),
            (Status::Passed, Status::Error) => Some((FindingKind::PassedToError, true)),
            (s, Status::Skipped) if s != Status::Skipped => Some((FindingKind::NewlySkipped, true)),
            (Status::Failed | Status::Error | Status::Skipped, Status::Passed) => {
                Some((FindingKind::Improved, false))
            }
            _ => None,
        };
        if let Some((kind, policy)) = kind {
            findings.push(finding(
                kind,
                Some(id.clone()),
                Some(target.clone()),
                policy,
            ));
        }
    }
    for (id, c) in &b {
        if !matched.contains(id) {
            findings.push(finding(
                FindingKind::NewlyAdded,
                None,
                Some(id.clone()),
                false,
            ));
            if c.status == Status::Skipped {
                findings.push(finding(
                    FindingKind::NewlySkipped,
                    None,
                    Some(id.clone()),
                    true,
                ));
            }
        }
        if matches!(c.status, Status::Failed | Status::Error) {
            findings.push(finding(
                FindingKind::CandidateFailure,
                owners.get(id).map(|id| (*id).clone()),
                Some(id.clone()),
                true,
            ));
        }
    }
    for (map, is_baseline) in [(&a, true), (&b, false)] {
        for (id, c) in map {
            if c.retry_failures > 0 {
                findings.push(finding(
                    FindingKind::RetryNotice,
                    is_baseline.then(|| id.clone()),
                    (!is_baseline).then(|| id.clone()),
                    false,
                ));
            }
        }
    }
    let eligible_pairs: Vec<_> = pairs
        .iter()
        .filter(|(a, b)| eligible(a) && eligible(b))
        .collect();
    let durations = Durations {
        baseline: durations(baseline.cases.iter().map(|s| &s.case)),
        candidate: durations(candidate.cases.iter().map(|s| &s.case)),
        matched_baseline: durations(eligible_pairs.iter().map(|(a, _)| *a)),
        matched_candidate: durations(eligible_pairs.iter().map(|(_, b)| *b)),
        matched_pairs: pairs.len(),
        matched_eligible_pairs: eligible_pairs.len(),
    };
    if durations.baseline.excluded_invalid > 0 || durations.candidate.excluded_invalid > 0 {
        diagnostics.push("invalid non-finite or negative testcase duration".into());
    }
    let coverage = coverage(baseline, candidate, &mut diagnostics, &mut findings);
    for f in &mut findings {
        let observe = |c: &&TestCase| Observation {
            status: c.status,
            duration_seconds: c.duration_seconds.filter(|d| d.is_finite()),
            retry_failures: c.retry_failures,
        };
        f.baseline_observation = f.baseline.as_ref().and_then(|id| a.get(id)).map(observe);
        f.candidate_observation = f.candidate.as_ref().and_then(|id| b.get(id)).map(observe);
    }
    findings.sort_by(|a, b| {
        (&a.kind, &a.baseline, &a.candidate, &a.scope).cmp(&(
            &b.kind,
            &b.baseline,
            &b.candidate,
            &b.scope,
        ))
    });
    diagnostics.sort();
    diagnostics.dedup();
    let verdict = if !diagnostics.is_empty() {
        Verdict::Inconclusive
    } else if findings.iter().any(|f| f.policy_finding) {
        Verdict::PolicyFindings
    } else {
        Verdict::Accepted
    };
    Comparison{schema_version:1,verdict,baseline_cases:baseline.cases.len(),candidate_cases:candidate.cases.len(),matched_cases:matched.len(),mappings_applied:remap.len(),findings,diagnostics,durations,coverage,limitations:vec!["This compares supplied report snapshots only. Missing identities do not prove deletion; absent-from-both tests and partial runs are invisible. Provenance and run completeness are not independently verified.".into(),"Accepted means no default-policy findings in this evidence, not proof of test completeness, assertion quality, behavioral equivalence, or library/JDK compatibility.".into(),"Durations describe reported testcase durations in each single snapshot, not repeated-run latency, total retry cost, or build wall time. P50/P95 use nearest rank: sorted value at ceil(p*n), one-based. Skipped, retried, missing and invalid durations are excluded in that priority order. Matched summaries use only pairs eligible on both sides; no causal speedup claim is justified.".into(),"Coverage changes are numeric evidence only. Even unchanged reported classes and denominators do not establish identical code, instrumentation, or exclusions. Zero-total percentages are undefined. Absent coverage is not checked.".into(),"Retry records indicate observed retries, not proof of a root cause or future determinism.".into(),"Manual privacy review required before sharing. Test identities and diagnostic paths can contain sensitive data. Logs, stack traces, properties, and failure messages are not exported.".into()]}
}
#[cfg(test)]
#[path = "../tests/unit/compare.rs"]
mod tests;
