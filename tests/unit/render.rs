use super::*;
use crate::compare::{Finding, FindingKind, Snapshot, compare};
use crate::report::{Counter, Coverage};
#[test]
fn bounded_encoded_data() {
    let text = "[link](https://bad)\n# injected `code` <script>\u{202e}\0";
    let encoded = data(text);
    assert!(encoded.contains("<script>"));
    assert!(!encoded.contains('\n'));
    assert!(encoded.contains("\\u202e"));
    assert!(data(&"x".repeat(600)).contains("omitted 88 characters"));
    assert_eq!(
        data("plain 123/path.txt: value"),
        "\"plain 123/path.txt: value\""
    );
}
#[test]
fn complete_empty_review() {
    let r = compare(&Snapshot::default(), &Snapshot::default(), &[]);
    let md = markdown(&r);
    for expected in [
        "Review instructions",
        "Manual privacy review",
        "No findings",
        "no coverage evidence",
        "Nearest-rank",
        "undefined",
        "Limits and interpretation",
        "omitted: 0",
    ] {
        assert!(md.contains(expected), "missing {expected}");
    }
    let j: serde_json::Value = serde_json::from_str(&json(&r).unwrap()).unwrap();
    assert_eq!(j["verdict"], "inconclusive");
}
#[test]
fn full_render_and_omission() {
    let mut a = Snapshot::default();
    let mut b = Snapshot::default();
    a.coverage.insert(
        "[scope]".into(),
        Coverage {
            lines: Counter {
                missed: 1,
                covered: 9,
            },
            branches: Some(Counter {
                missed: 0,
                covered: 0,
            }),
            classes: vec![],
        },
    );
    b.coverage.insert(
        "[scope]".into(),
        Coverage {
            lines: Counter {
                missed: 2,
                covered: 8,
            },
            branches: Some(Counter {
                missed: 0,
                covered: 0,
            }),
            classes: vec![],
        },
    );
    b.coverage.insert(
        "missing".into(),
        Coverage {
            lines: Counter {
                missed: 0,
                covered: 1,
            },
            branches: None,
            classes: vec![],
        },
    );
    let mut r = compare(&a, &b, &[]);
    let id = ScopedId {
        scope: "m".into(),
        id: crate::report::TestId {
            suite: vec!["s".into()],
            classname: "c".into(),
            name: "[unsafe]\n测试中文`````<script>".into(),
        },
    };
    r.findings = (0..102)
        .map(|_| Finding {
            kind: FindingKind::RetryNotice,
            baseline: Some(id.clone()),
            candidate: None,
            scope: None,
            policy_finding: false,
            baseline_observation: None,
            candidate_observation: None,
        })
        .collect();
    r.diagnostics = vec!["[diagnostic]\n".into(); 101];
    r.durations.baseline.p50_seconds = Some(1.0);
    let md = markdown(&r);
    assert!(md.contains("Findings shown: 100; omitted: 2"));
    assert!(md.contains("Diagnostics shown: 100; omitted: 1"));
    assert!(md.contains("delta=-10.000000 percentage points"));
    assert!(md.contains("1.000000 s"));
    assert!(md.contains("not available on both sides"));
    assert!(md.contains("[unsafe]"));
    assert!(md.contains("测试中文`````<script>"));
    assert!(md.contains("``````text\n"));
    assert!(md.contains("[scope]"));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&json(&r).unwrap()).unwrap()["findings"]
            .as_array()
            .unwrap()
            .len(),
        102
    );
}
#[test]
fn unicode_and_embedded_fences_remain_readable_data() {
    assert_eq!(data("测试中文"), "\"测试中文\"");
    let content = "\"```\n<script>测试</script>`````\"";
    let mut out = String::new();
    fenced(&mut out, content);
    assert!(out.starts_with("``````text\n"));
    assert!(out.ends_with("\n``````\n\n"));
    assert!(out.contains("<script>测试</script>"));
    let mut empty = String::new();
    fenced(&mut empty, "");
    assert!(empty.starts_with("```text"));
}
#[test]
fn tiny_nonzero_values_keep_sign_magnitude_and_units() {
    assert_eq!(number(Some(0.0)), "0.000000");
    assert_eq!(number(Some(1e-6)), "0.000001");
    assert_eq!(number(Some(1e-7)), "1.000000e-7");
    assert_eq!(number(Some(-1e-7)), "-1.000000e-7");
    let mut a = Snapshot::default();
    let mut b = Snapshot::default();
    a.coverage.insert(
        "large".into(),
        Coverage {
            lines: Counter {
                covered: u64::MAX,
                missed: 0,
            },
            branches: None,
            classes: vec![],
        },
    );
    b.coverage.insert(
        "large".into(),
        Coverage {
            lines: Counter {
                covered: u64::MAX - 1,
                missed: 1,
            },
            branches: None,
            classes: vec![],
        },
    );
    let mut report = compare(&a, &b, &[]);
    report.durations.baseline.p50_seconds = Some(1e-9);
    let md = markdown(&report);
    assert!(md.contains("delta=-5.421011e-18 percentage points"));
    assert!(!md.contains("delta=-0.000000"));
    assert!(md.contains("P50=1.000000e-9 s"));
    let structured: serde_json::Value = serde_json::from_str(&json(&report).unwrap()).unwrap();
    assert!(
        structured["coverage"][0]["lines"]["percentage_point_delta"]
            .as_f64()
            .unwrap()
            < 0.0
    );
    assert_eq!(
        structured["coverage"][0]["lines"]["baseline"]["covered"].as_u64(),
        Some(u64::MAX)
    );
}
