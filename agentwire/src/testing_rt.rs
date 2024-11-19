//! Testing runtime.

#![doc(hidden)]

use crate::agent;
use futures::prelude::*;
use std::{
    env,
    panic::{catch_unwind, AssertUnwindSafe},
    pin::Pin,
    process,
    time::Duration,
};
use tokio::{process::Command, runtime, time};

/// Name of the environment variable used to pass the test ID.
pub const BROKER_TEST_ID_ENV: &str = "AGENTWIRE_BROKER_TEST_ID";

/// Default timeout for broker tests.
pub const DEFAULT_TIMEOUT: u64 = 60_000;

/// Runs a broker test.
pub fn run_broker_test(
    test_name: &str,
    test_id: &str,
    timeout: Duration,
    init: impl FnOnce(),
    f: Pin<Box<dyn Future<Output = ()>>>,
) {
    let test_id = format!("{test_id:?}");
    if env::var(BROKER_TEST_ID_ENV).map_or(false, |var| var == test_id) {
        let result = catch_unwind(AssertUnwindSafe(|| {
            init();
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(f);
        }));
        process::exit(result.is_err().into());
    }
    let mut test_runner_args = env::args();
    let mut child_args = Vec::new();
    while let Some(arg) = test_runner_args.next() {
        match arg.as_str() {
            "--bench"
            | "--exclude-should-panic"
            | "--force-run-in-process"
            | "--ignored"
            | "--include-ignored"
            | "--show-output"
            | "--test" => {
                child_args.push(arg);
            }
            "--color" | "-Z" => {
                child_args.push(arg);
                if let Some(arg) = test_runner_args.next() {
                    child_args.push(arg);
                }
            }
            _ => {}
        }
    }
    child_args.push("--quiet".into());
    child_args.push("--test-threads".into());
    child_args.push("1".into());
    child_args.push("--nocapture".into());
    child_args.push("--exact".into());
    child_args.push("--".into());
    child_args.push(test_name.into());
    let result = runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let mut child = Command::new(env::current_exe().unwrap())
                .args(&child_args)
                .env(BROKER_TEST_ID_ENV, test_id)
                .env(agent::process::ARGS_ENV, shell_words::join(&child_args))
                .spawn()
                .unwrap();
            time::timeout(timeout, child.wait())
                .await
                .expect("timeouted")
                .unwrap()
        });
    assert!(result.success(), "test failed");
}
