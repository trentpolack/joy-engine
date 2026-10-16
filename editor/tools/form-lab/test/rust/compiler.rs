// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The compile lifecycle: builds report geometry or located diagnostics, budgets expire runaway or stalled builds,
//! and a newer request abandons an older one.

use form_lab::compiler::{Budgets, CompileOutcome, CompilePhase, CompileRequest, Compiler};
use joy_engine::serde_json::json;

fn request(source: &str) -> CompileRequest {
    CompileRequest { source: source.to_owned(), overrides: json!({}), materials: None }
}

#[test]
fn builds_report_geometry_or_located_diagnostics() {
    let mut compiler = Compiler::new();
    assert_eq!(compiler.phase(), CompilePhase::Idle);
    compiler.start(request("box(0,0,0,1,1,1)"));
    assert_ne!(compiler.phase(), CompilePhase::Idle);
    match compiler.finish() {
        Some(CompileOutcome::Built { data, milliseconds }) => {
            assert_eq!(data.positions.len(), 24);
            assert!(milliseconds >= 0.0);
        }
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(compiler.phase(), CompilePhase::Idle);
    compiler.start(request("point(0,0,0)\npoint(missing,0,0)"));
    let outcome = compiler.finish().unwrap();
    let CompileOutcome::Rejected(diagnostic) = &outcome else {
        panic!("unexpected {outcome:?}");
    };
    assert_eq!(diagnostic.line, 2);
    assert!(outcome.failure_message().unwrap().starts_with("Line 2, column 7: "), "{:?}", outcome.failure_message());
}

#[test]
fn execution_and_startup_budgets_expire_builds() {
    let mut compiler = Compiler::with_budgets(Budgets { startup_milliseconds: 10_000.0, execution_milliseconds: 0.0 });
    compiler.start(request("repeat 2000 as i { point(i, 0, 0) }"));
    assert_eq!(compiler.finish(), Some(CompileOutcome::ExecutionExpired));
    assert_eq!(
        CompileOutcome::ExecutionExpired.failure_message().unwrap(),
        "Build exceeded 2 seconds. Reduce loops or resolution. Previous preview retained."
    );
    let mut stalled = Compiler::with_budgets(Budgets { startup_milliseconds: -1.0, execution_milliseconds: 2_000.0 });
    stalled.start(request("point(0,0,0)"));
    assert_eq!(stalled.poll(), Some(CompileOutcome::StartupExpired));
}

#[test]
fn a_newer_request_abandons_the_older_build() {
    let mut compiler = Compiler::new();
    compiler.start(request("repeat 100000 as i { point(i, 0, 0) }"));
    compiler.start(request("point(1,2,3)"));
    match compiler.finish() {
        Some(CompileOutcome::Built { data, .. }) => assert_eq!(data.positions, [1.0, 2.0, 3.0]),
        other => panic!("unexpected {other:?}"),
    }
    compiler.start(request("point(0,0,0)"));
    compiler.cancel();
    assert_eq!(compiler.phase(), CompilePhase::Idle);
    assert_eq!(compiler.poll(), None);
}
