use super::*;
fn case(name: &str, status: Status, duration: Option<f64>, retry: u32) -> ScopedCase {
    ScopedCase {
        scope: "module".into(),
        case: TestCase {
            id: TestId {
                suite: vec!["suite".into()],
                classname: "Class".into(),
                name: name.into(),
            },
            status,
            duration_seconds: duration,
            retry_failures: retry,
        },
    }
}
fn snapshot(cases: Vec<ScopedCase>) -> Snapshot {
    Snapshot {
        cases,
        ..Snapshot::default()
    }
}
fn id(name: &str) -> ScopedId {
    let c = case(name, Status::Passed, Some(1.0), 0);
    ScopedId {
        scope: c.scope,
        id: c.case.id,
    }
}
fn clean() -> Snapshot {
    snapshot(vec![case("a", Status::Passed, Some(1.0), 0)])
}
fn cv(covered: u64, missed: u64, branches: Option<Counter>, classes: Vec<&str>) -> Coverage {
    Coverage {
        lines: Counter { covered, missed },
        branches,
        classes: classes.into_iter().map(String::from).collect(),
    }
}
#[test]
fn clean_counts_and_codes() {
    let r = compare(&clean(), &clean(), &[]);
    assert_eq!(r.verdict, Verdict::Accepted);
    assert_eq!(r.matched_cases, 1);
    assert_eq!(r.verdict.exit_code(), 0);
    assert_eq!(Verdict::PolicyFindings.exit_code(), 1);
    assert_eq!(Verdict::Inconclusive.exit_code(), 2);
    assert!(r.findings.is_empty());
}
#[test]
fn complete_status_matrix() {
    for before in [
        Status::Passed,
        Status::Failed,
        Status::Error,
        Status::Skipped,
    ] {
        for after in [
            Status::Passed,
            Status::Failed,
            Status::Error,
            Status::Skipped,
        ] {
            let r = compare(
                &snapshot(vec![case("a", before, Some(1.0), 0)]),
                &snapshot(vec![case("a", after, Some(2.0), 0)]),
                &[],
            );
            let expected = matches!(after, Status::Failed | Status::Error)
                || (after == Status::Skipped && before != Status::Skipped);
            assert_eq!(
                r.verdict == Verdict::PolicyFindings,
                expected,
                "{before:?} -> {after:?}"
            );
            if before == Status::Passed && after == Status::Failed {
                assert!(
                    r.findings
                        .iter()
                        .any(|f| f.kind == FindingKind::PassedToFailure)
                );
            }
            if before == Status::Passed && after == Status::Error {
                assert!(
                    r.findings
                        .iter()
                        .any(|f| f.kind == FindingKind::PassedToError)
                );
            }
            if before != Status::Passed && after == Status::Passed {
                assert!(r.findings.iter().any(|f| f.kind == FindingKind::Improved));
            }
        }
    }
}
#[test]
fn additions_absences_and_scopes() {
    let a = clean();
    let b = snapshot(vec![
        case("b", Status::Passed, Some(1.0), 0),
        case("c", Status::Skipped, None, 0),
    ]);
    let r = compare(&a, &b, &[]);
    for k in [
        FindingKind::NewlyAdded,
        FindingKind::MissingInCandidate,
        FindingKind::NewlySkipped,
    ] {
        assert!(r.findings.iter().any(|f| f.kind == k));
    }
    let mut b = clean();
    b.cases[0].scope = "other".into();
    assert_eq!(compare(&a, &b, &[]).matched_cases, 0);
}
#[test]
fn explicit_mapping_and_json() {
    let a = clean();
    let b = snapshot(vec![case("renamed", Status::Passed, Some(1.0), 0)]);
    let mapping = IdentityMapping {
        baseline: id("a"),
        candidate: id("renamed"),
    };
    let encoded = serde_json::to_string(&mapping).unwrap();
    let parsed: IdentityMapping = serde_json::from_str(&encoded).unwrap();
    let r = compare(&a, &b, &[parsed]);
    assert_eq!(r.verdict, Verdict::Accepted);
    assert_eq!(r.mappings_applied, 1);
    assert_eq!(r.matched_cases, 1);
    assert!(
        serde_json::from_str::<IdentityMapping>("{\"baseline\":{},\"candidate\":{},\"extra\":0}")
            .is_err()
    );
}
#[test]
fn rejects_mapping_ambiguity() {
    let a = snapshot(vec![
        case("a", Status::Passed, None, 0),
        case("b", Status::Passed, None, 0),
    ]);
    let b = snapshot(vec![
        case("a", Status::Passed, None, 0),
        case("b", Status::Passed, None, 0),
        case("c", Status::Passed, None, 0),
    ]);
    for mappings in [
        vec![IdentityMapping {
            baseline: id("missing"),
            candidate: id("c"),
        }],
        vec![IdentityMapping {
            baseline: id("a"),
            candidate: id("missing"),
        }],
        vec![
            IdentityMapping {
                baseline: id("a"),
                candidate: id("c"),
            },
            IdentityMapping {
                baseline: id("a"),
                candidate: id("b"),
            },
        ],
        vec![
            IdentityMapping {
                baseline: id("a"),
                candidate: id("c"),
            },
            IdentityMapping {
                baseline: id("b"),
                candidate: id("c"),
            },
        ],
        vec![IdentityMapping {
            baseline: id("a"),
            candidate: id("b"),
        }],
    ] {
        assert_eq!(compare(&a, &b, &mappings).verdict, Verdict::Inconclusive);
    }
}
#[test]
fn full_identity_swap_is_valid() {
    let a = snapshot(vec![
        case("a", Status::Passed, None, 0),
        case("b", Status::Passed, None, 0),
    ]);
    let mappings = [
        IdentityMapping {
            baseline: id("a"),
            candidate: id("b"),
        },
        IdentityMapping {
            baseline: id("b"),
            candidate: id("a"),
        },
    ];
    assert_eq!(compare(&a, &a, &mappings).verdict, Verdict::Accepted);
}
#[test]
fn invalid_evidence_never_accepts() {
    let mut a = clean();
    a.cases.push(a.cases[0].clone());
    a.warnings.push("suite mismatch".into());
    a.errors.push("unreadable evidence".into());
    let r = compare(&a, &clean(), &[]);
    assert_eq!(r.verdict, Verdict::Inconclusive);
    assert_eq!(r.diagnostics.len(), 3);
    assert_eq!(
        compare(&Snapshot::default(), &Snapshot::default(), &[]).verdict,
        Verdict::Inconclusive
    );
}
#[test]
fn nearest_rank_and_exclusion_priority() {
    let mut cases: Vec<_> = (1..=20)
        .rev()
        .map(|i| case(&i.to_string(), Status::Passed, Some(i as f64), 0))
        .collect();
    cases.extend([
        case("skip", Status::Skipped, None, 1),
        case("retry", Status::Passed, None, 1),
        case("missing", Status::Passed, None, 0),
        case("nan", Status::Passed, Some(f64::NAN), 0),
        case("negative", Status::Passed, Some(-1.0), 0),
    ]);
    let a = snapshot(cases);
    let r = compare(&a, &a, &[]);
    let s = &r.durations.baseline;
    assert_eq!(s.sample_count, 20);
    assert_eq!(s.p50_seconds, Some(10.0));
    assert_eq!(s.p95_seconds, Some(19.0));
    assert_eq!(
        (
            s.excluded_missing,
            s.excluded_skipped,
            s.excluded_retried,
            s.excluded_invalid
        ),
        (1, 1, 1, 2)
    );
    assert_eq!(r.durations.matched_eligible_pairs, 20);
    assert_eq!(r.durations.matched_pairs, 25);
    assert_eq!(r.verdict, Verdict::Inconclusive);
    assert_eq!(
        r.findings
            .iter()
            .filter(|f| f.kind == FindingKind::RetryNotice)
            .count(),
        4
    );
}
#[test]
fn matched_population_excludes_both_when_either_invalid() {
    let a = snapshot(vec![
        case("a", Status::Passed, Some(0.0), 0),
        case("b", Status::Passed, Some(99.0), 0),
    ]);
    let b = snapshot(vec![
        case("a", Status::Passed, Some(2.0), 0),
        case("b", Status::Passed, None, 0),
    ]);
    let r = compare(&a, &b, &[]);
    assert_eq!(r.durations.baseline.sample_count, 2);
    assert_eq!(r.durations.matched_baseline.sample_count, 1);
    assert_eq!(r.durations.matched_baseline.p95_seconds, Some(0.0));
    assert_eq!(r.durations.matched_candidate.p50_seconds, Some(2.0));
}
#[test]
fn coverage_regression_and_unknown_scope() {
    let mut a = clean();
    let mut b = clean();
    a.coverage.insert(
        "report".into(),
        cv(
            8,
            2,
            Some(Counter {
                covered: 4,
                missed: 1,
            }),
            vec!["A"],
        ),
    );
    b.coverage.insert(
        "report".into(),
        cv(
            7,
            3,
            Some(Counter {
                covered: 4,
                missed: 1,
            }),
            vec!["A"],
        ),
    );
    let r = compare(&a, &b, &[]);
    assert_eq!(r.verdict, Verdict::PolicyFindings);
    assert_eq!(
        r.coverage[0].lines.as_ref().unwrap().percentage_point_delta,
        Some(-10.0)
    );
    assert!(
        r.findings
            .iter()
            .any(|f| f.kind == FindingKind::CoverageDecrease)
    );
    b.coverage.get_mut("report").unwrap().classes = vec!["B".into()];
    assert_eq!(compare(&a, &b, &[]).verdict, Verdict::Inconclusive);
    b.coverage.get_mut("report").unwrap().classes = vec!["A".into()];
    b.coverage.get_mut("report").unwrap().lines.missed = 4;
    assert_eq!(compare(&a, &b, &[]).verdict, Verdict::Inconclusive);
    b.coverage.clear();
    assert_eq!(
        compare(&a, &b, &[]).coverage[0].lines.as_ref().map(|_| ()),
        None
    );
}
#[test]
fn coverage_branch_and_zero_total() {
    let mut a = clean();
    let mut b = clean();
    a.coverage.insert(
        "r".into(),
        cv(
            0,
            0,
            Some(Counter {
                covered: 2,
                missed: 0,
            }),
            vec![],
        ),
    );
    b.coverage.insert(
        "r".into(),
        cv(
            0,
            0,
            Some(Counter {
                covered: 1,
                missed: 1,
            }),
            vec![],
        ),
    );
    let r = compare(&a, &b, &[]);
    assert_eq!(r.verdict, Verdict::PolicyFindings);
    assert_eq!(r.coverage[0].lines.as_ref().unwrap().baseline.percent, None);
    b.coverage.get_mut("r").unwrap().branches = None;
    assert_eq!(compare(&a, &b, &[]).verdict, Verdict::Inconclusive);
    a.coverage.get_mut("r").unwrap().branches = None;
    assert_eq!(compare(&a, &b, &[]).verdict, Verdict::Accepted);
    a.coverage.get_mut("r").unwrap().branches = Some(Counter {
        covered: 0,
        missed: 0,
    });
    b.coverage.get_mut("r").unwrap().branches = Some(Counter {
        covered: 0,
        missed: 1,
    });
    assert_eq!(compare(&a, &b, &[]).verdict, Verdict::Inconclusive);
}
#[test]
fn deterministic_order() {
    let a = snapshot(vec![
        case("b", Status::Failed, None, 1),
        case("a", Status::Passed, None, 0),
    ]);
    let mut b = snapshot(a.cases.clone());
    b.cases.reverse();
    assert_eq!(
        serde_json::to_string(&compare(&a, &a, &[])).unwrap(),
        serde_json::to_string(&compare(&b, &b, &[])).unwrap()
    );
}
#[test]
fn tiny_coverage_regression_is_not_lost_to_float_rounding() {
    let mut a = clean();
    let mut b = clean();
    a.coverage
        .insert("huge".into(), cv(u64::MAX, 0, None, vec!["A"]));
    b.coverage
        .insert("huge".into(), cv(u64::MAX - 1, 1, None, vec!["A"]));
    let r = compare(&a, &b, &[]);
    assert_eq!(r.verdict, Verdict::PolicyFindings);
    assert!(
        r.coverage[0]
            .lines
            .as_ref()
            .unwrap()
            .percentage_point_delta
            .unwrap()
            < 0.0
    );
    assert_eq!(compare(&b, &a, &[]).verdict, Verdict::Accepted);
}
#[test]
fn observations_preserve_status_duration_and_retry_count() {
    let a = snapshot(vec![case("a", Status::Failed, Some(1.5), 2)]);
    let b = snapshot(vec![case("a", Status::Error, Some(2.5), 3)]);
    let r = compare(&a, &b, &[]);
    let f = r
        .findings
        .iter()
        .find(|f| f.kind == FindingKind::CandidateFailure)
        .unwrap();
    assert_eq!(
        f.baseline_observation.as_ref().unwrap().status,
        Status::Failed
    );
    assert_eq!(f.baseline_observation.as_ref().unwrap().retry_failures, 2);
    assert_eq!(f.candidate_observation.as_ref().unwrap().retry_failures, 3);
    assert_eq!(
        f.candidate_observation.as_ref().unwrap().duration_seconds,
        Some(2.5)
    );
}
