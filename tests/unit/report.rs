use super::*;
fn junit(body: &str) -> Result<Report, ParseError> {
    parse_junit(format!("<testsuite name=\"suite\">{body}</testsuite>").as_bytes())
}
fn case(attrs: &str, body: &str) -> String {
    format!("<testcase classname=\"C\" name=\"n\" {attrs}>{body}</testcase>")
}
#[test]
fn realistic_surefire_statuses_retries_and_privacy() {
    let report = parse_junit(include_bytes!("../fixtures/parser/surefire.xml")).unwrap();
    assert!(report.warnings.is_empty());
    assert_eq!(
        report.cases.iter().map(|c| c.status).collect::<Vec<_>>(),
        vec![
            Status::Passed,
            Status::Failed,
            Status::Error,
            Status::Skipped,
            Status::Passed
        ]
    );
    assert_eq!(report.cases[0].id.name, "passes & preserves \"identity\"");
    assert_eq!(report.cases[2].duration_seconds, None);
    assert_eq!(report.cases[4].retry_failures, 2);
    let serialized = serde_json::to_string(&report).unwrap();
    for secret in [
        "SECRET",
        "private",
        "secret",
        "AssertionError",
        "stacktrace",
    ] {
        assert!(!serialized.contains(secret));
    }
}
#[test]
fn nested_jupiter_preserves_exact_identity() {
    let r = parse_junit(include_bytes!("../fixtures/parser/jupiter.xml")).unwrap();
    assert_eq!(r.cases.len(), 2);
    assert!(r.warnings.is_empty());
    assert_eq!(r.cases[0].id.suite, ["Jupiter engine", "NestedContainer"]);
    assert_eq!(r.cases[0].duration_seconds, Some(0.001));
}
#[test]
fn identities_are_required_and_bounded() {
    for xml in [
        "<testsuite/>",
        "<testsuite name=\" \"/>",
        "<testsuite name=\"s\"><testcase name=\"n\"/></testsuite>",
        "<testsuite name=\"s\"><testcase classname=\"c\"/></testsuite>",
        "<testsuite name=\"s\"><testcase classname=\"\" name=\"n\"/></testsuite>",
    ] {
        assert!(parse_junit(xml.as_bytes()).is_err(), "{xml}");
    }
    assert!(
        parse_junit(
            format!(
                "<testsuite name=\"{}\"/>",
                "x".repeat(MAX_IDENTITY_BYTES + 1)
            )
            .as_bytes()
        )
        .is_err()
    );
}
#[test]
fn duplicates_rejected_but_suites_disambiguate() {
    assert!(
        junit(&format!("{}{}", case("", ""), case("", "")))
            .unwrap_err()
            .0
            .contains("duplicate")
    );
    let xml = "<testsuites><testsuite name='a'><testcase name='n' classname='c'/></testsuite><testsuite name='b'><testcase name='n' classname='c'/></testsuite></testsuites>";
    assert_eq!(parse_junit(xml.as_bytes()).unwrap().cases.len(), 2);
}
#[test]
fn missing_duration_unknown_invalid_duration_rejected() {
    assert_eq!(
        junit(&case("", "")).unwrap().cases[0].duration_seconds,
        None
    );
    for d in ["", "NaN", "inf", "-inf", "-0.1", "oops", "1e999", " 1"] {
        assert!(junit(&case(&format!("time='{d}'"), "")).is_err(), "{d}");
    }
    assert_eq!(
        junit(&case("time='0'", "")).unwrap().cases[0].duration_seconds,
        Some(0.0)
    );
}
#[test]
fn validates_declared_counts_and_reconciles() {
    for count in ["-1", "+1", "1.0", "NaN", "18446744073709551616", ""] {
        assert!(parse_junit(format!("<testsuite name='s' tests='{count}'/>").as_bytes()).is_err());
    }
    let r = parse_junit(b"<testsuite name='s' tests='4' failures='2' errors='1' skipped='1'/>")
        .unwrap();
    assert_eq!(r.warnings.len(), 4);
    assert!(
        parse_junit(b"<testsuites tests='1'/>")
            .unwrap()
            .warnings
            .len()
            == 1
    );
    assert!(junit(&case("assertions='-1'", "")).is_err());
}
#[test]
fn statuses_are_conservative_and_contradictions_rejected() {
    for body in [
        "<failure/><error/>",
        "<error/><failure/>",
        "<error/><error/>",
    ] {
        assert_eq!(
            junit(&case("", body)).unwrap().cases[0].status,
            Status::Error
        );
    }
    assert_eq!(
        junit(&case("", "<failure/><failure/>")).unwrap().cases[0].status,
        Status::Failed
    );
    for body in [
        "<skipped/><failure/>",
        "<error/><skipped/>",
        "<skipped/><error/>",
    ] {
        assert!(junit(&case("", body)).is_err());
    }
    assert!(junit(&case("", "<vendor><failure/></vendor>")).is_err());
}
#[test]
fn rejects_wrong_structure_and_malformed_xml() {
    for xml in [
        "",
        " ",
        "<foo/>",
        "<testsuite name='s'>",
        "<testsuite name='s'></testsuites>",
        "<testsuite name='s'/><testsuite name='s'/>",
        "x<testsuite name='s'/>",
        "<testsuite name='s'/>x",
        "<testsuite name='s'><wrapper><testcase name='n' classname='c'/></wrapper></testsuite>",
        "<testsuites><testsuites/></testsuites>",
        "<testsuite name='s'><x><testsuite name='nested'/></x></testsuite>",
        "<testsuite name='s'><testcase name='n' classname='c'><testcase name='n2' classname='c'/></testcase></testsuite>",
        "<testsuite name='s' name='s2'/>",
        "<testsuite name='&unknown;'/>",
        "<testsuite name='s'/><![CDATA[x]]>",
        "<testsuite name='s'/>&amp;",
    ] {
        assert!(parse_junit(xml.as_bytes()).is_err(), "accepted {xml}");
    }
}
#[test]
fn rejects_dtd_and_unknown_entities_without_resolution() {
    for xml in [
        "<!DOCTYPE testsuite SYSTEM 'file:///etc/passwd'><testsuite name='s'/>",
        "<!DOCTYPE testsuite [<!ENTITY xxe SYSTEM 'http://localhost/secret'>]><testsuite name='s'>&xxe;</testsuite>",
        "<testsuite name='s'>&unknown;</testsuite>",
    ] {
        assert!(parse_junit(xml.as_bytes()).is_err());
    }
    assert!(
        parse_junit(b"<testsuite name='s'>&amp;&lt;&gt;&apos;&quot;&#32;&#x20;</testsuite>")
            .is_ok()
    );
}
#[test]
fn encodings_and_xml_prolog() {
    assert!(parse_junit(&[0xff]).is_err());
    assert!(
        parse_junit(b"<?xml version='1.0' encoding='ISO-8859-1'?><testsuite name='s'/>").is_err()
    );
    assert!(parse_junit(b"<testsuite name='s'/><?xml version='1.0'?>").is_err());
    assert!(
        parse_junit(
            b"<?xml version='1.0'?><!-- comment --><?build instruction?><testsuite name='s'/>"
        )
        .is_ok()
    );
}
#[test]
fn resource_limits_are_inclusive_and_enforced() {
    let xml = b"<testsuite name='s'/>";
    assert!(parse_junit_limits(xml, xml.len(), 1, 0).is_ok());
    assert!(
        parse_junit_limits(xml, xml.len() - 1, 1, 0)
            .unwrap_err()
            .0
            .contains("byte")
    );
    assert!(
        parse_junit_limits(xml, xml.len(), 0, 0)
            .unwrap_err()
            .0
            .contains("depth")
    );
    let cases = b"<testsuite name='s'><testcase name='n' classname='c'/></testsuite>";
    assert!(parse_junit_limits(cases, cases.len(), 2, 1).is_ok());
    assert!(
        parse_junit_limits(cases, cases.len(), 2, 0)
            .unwrap_err()
            .0
            .contains("case")
    );
    assert!(parse_junit_limits(cases, cases.len(), 1, 1).is_err());
}
#[test]
fn jacoco_uses_only_root_counter_and_sorted_scope() {
    let xml = br#"<?xml version="1.0"?><!DOCTYPE report PUBLIC "-//JACOCO//DTD Report 1.1//EN" "report.dtd"><report name="r"><package name="p"><class name="p/Z"><counter type="LINE" missed="99" covered="99"/></class><class name="p/A"/><counter type="LINE" missed="99" covered="99"/></package><counter type="LINE" missed="2" covered="8"/><counter type="BRANCH" missed="1" covered="3"/><counter type="METHOD" missed="0" covered="2"/></report>"#;
    let c = parse_jacoco(xml).unwrap();
    assert_eq!(
        c.lines,
        Counter {
            missed: 2,
            covered: 8
        }
    );
    assert_eq!(
        c.branches,
        Some(Counter {
            missed: 1,
            covered: 3
        })
    );
    assert_eq!(c.classes, ["p/A", "p/Z"]);
}
#[test]
fn jacoco_missing_optional_branch_and_zero_lines_are_explicit() {
    let c =
        parse_jacoco(b"<report><counter type='LINE' missed='0' covered='0'/></report>").unwrap();
    assert_eq!(c.lines.covered, 0);
    assert_eq!(c.branches, None);
    assert!(c.classes.is_empty());
}
#[test]
fn jacoco_rejects_invalid_or_ambiguous_evidence() {
    for xml in [
        "<testsuite/>",
        "<report/>",
        "<report><counter type='LINE' covered='1'/></report>",
        "<report><counter type='LINE' missed='1'/></report>",
        "<report><counter type='LINE' missed='-1' covered='1'/></report>",
        "<report><counter type='LINE' missed='18446744073709551615' covered='1'/></report>",
        "<report><counter type='LINE' missed='0' covered='1'/><counter type='LINE' missed='0' covered='1'/></report>",
        "<report><package><class name='a'/><class name='a'/></package></report>",
        "<!DOCTYPE report SYSTEM 'http://evil.test/x'><report/>",
        "<!DOCTYPE report [<!ENTITY x 'y'>]><report/>",
    ] {
        assert!(parse_jacoco(xml.as_bytes()).is_err(), "{xml}");
    }
}
#[test]
fn error_display_does_not_contain_input() {
    let e = junit("<testcase name='SECRET'/>").unwrap_err();
    assert!(!e.to_string().contains("SECRET"));
    let _: &dyn Error = &e;
}

#[test]
fn malformed_character_references_controls_and_prologs_are_rejected() {
    for xml in [
        "<testsuite name='s'>&#wat;</testsuite>",
        "<testsuite name='s'>&#0;</testsuite>",
        "<testsuite name='s'>&#xD800;</testsuite>",
        "<testsuite name='&#0;'/>",
        "<testsuite name='s'>\u{0}</testsuite>",
        "<?xml version='1.1'?><testsuite name='s'/>",
        "<?xml version='1.0'?><?xml version='1.0'?><testsuite name='s'/>",
        "<?xml encoding='UTF-8'?><testsuite name='s'/>",
        "<!-- bad -- comment --><testsuite name='s'/>",
        "<testsuite name='s' time='NaN'/>",
    ] {
        assert!(parse_junit(xml.as_bytes()).is_err(), "{xml}");
    }
}
#[test]
fn standard_jacoco_doctype_is_single_and_before_root() {
    let dtd = "<!DOCTYPE report PUBLIC \"-//JACOCO//DTD Report 1.1//EN\" \"report.dtd\">";
    assert!(parse_jacoco(format!("{dtd}{dtd}<report/>").as_bytes()).is_err());
    assert!(parse_jacoco(format!("<report/>{dtd}").as_bytes()).is_err());
    assert!(parse_jacoco(format!("{dtd}<?xml version='1.0'?><report/>").as_bytes()).is_err());
}

#[test]
fn coverage_class_limit_is_enforced() {
    let mut xml = String::from("<report><package name='p'>");
    for i in 0..=MAX_TEST_CASES {
        xml.push_str(&format!("<class name='p/C{i}'/>"));
    }
    xml.push_str("</package><counter type='LINE' missed='0' covered='0'/></report>");
    assert!(
        parse_jacoco(xml.as_bytes())
            .unwrap_err()
            .0
            .contains("class limit")
    );
}

#[test]
fn test_identity_deserialization_rejects_unknown_fields() {
    assert!(
        serde_json::from_str::<TestId>(r#"{"suite":["s"],"classname":"c","name":"n"}"#).is_ok()
    );
    assert!(
        serde_json::from_str::<TestId>(
            r#"{"suite":["s"],"classname":"c","name":"n","clasname":"typo"}"#
        )
        .is_err()
    );
}

#[test]
fn misplaced_result_elements_never_become_passing_evidence() {
    for tag in [
        "failure",
        "error",
        "skipped",
        "flakyFailure",
        "flakyError",
        "rerunFailure",
        "rerunError",
    ] {
        for body in [
            format!("<result><{tag}/></result>"),
            format!("<system-out><{tag}/></system-out>"),
            format!("<failure><{tag}/></failure>"),
        ] {
            let error = junit(&case("", &body)).unwrap_err();
            assert!(error.0.contains("direct child"), "{body}");
        }
        assert!(junit(&format!("<{tag}/>")).is_err());
    }
}

#[test]
fn output_literals_are_not_result_markup_and_remain_private() {
    let report = junit(&case("", "<system-out><![CDATA[SECRET <failure/><rerunError/>]]></system-out><system-err>SECRET &lt;error/&gt;</system-err>")).unwrap();
    assert_eq!(report.cases[0].status, Status::Passed);
    assert_eq!(report.cases[0].retry_failures, 0);
    assert!(!serde_json::to_string(&report).unwrap().contains("SECRET"));
}

#[test]
fn surefire_retry_families_require_consistent_final_outcomes() {
    for body in [
        "<flakyFailure/><flakyError/>",
        "<flakyError/><flakyFailure/>",
    ] {
        let report = junit(&case("", body)).unwrap();
        assert_eq!(report.cases[0].status, Status::Passed);
        assert_eq!(report.cases[0].retry_failures, 2);
    }
    for body in [
        "<failure/><rerunFailure/><rerunError/>",
        "<rerunError/><error/>",
    ] {
        let report = junit(&case("", body)).unwrap();
        assert!(matches!(
            report.cases[0].status,
            Status::Failed | Status::Error
        ));
        assert!(report.cases[0].retry_failures > 0);
    }
    for body in [
        "<flakyFailure/><rerunFailure/>",
        "<rerunError/><flakyError/>",
        "<flakyFailure/><failure/>",
        "<error/><flakyError/>",
        "<flakyError/><skipped/>",
        "<rerunFailure/>",
        "<rerunError/><skipped/>",
    ] {
        assert!(junit(&case("", body)).is_err(), "{body}");
    }
    let xml = "<testsuite name='s'><testcase classname='C' name='flaky'><flakyFailure/></testcase><testcase classname='C' name='clean'/></testsuite>";
    assert_eq!(
        parse_junit(xml.as_bytes()).unwrap().cases[1].retry_failures,
        0
    );
}

#[test]
fn reconciliation_warnings_report_declared_and_observed_counts() {
    let report = parse_junit(
        b"<testsuite name='s' tests='5'><testcase name='a' classname='C'/></testsuite>",
    )
    .unwrap();
    assert_eq!(
        report.warnings,
        ["declared tests count 5 differs from parsed count 1"]
    );
}

#[test]
fn invalid_mixed_retry_fixture_is_rejected() {
    let error = parse_junit(include_bytes!(
        "../fixtures/parser/invalid-mixed-retries.xml"
    ))
    .unwrap_err();
    assert_eq!(error.0, "mixed flaky and rerun result families");
}
