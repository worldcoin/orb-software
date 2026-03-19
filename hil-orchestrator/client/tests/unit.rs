use clap::Parser;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use hiltop::{
    app::{App, LockAction, Mode},
    event::Event,
    ui::dashboard::{runner_table, staleness_style},
};
use orb_hil_types::{Platform, RunnerStatus};
use ratatui::style::Color;

fn make_runner(id: &str, platform: Platform, locked: bool, online: bool) -> RunnerStatus {
    RunnerStatus {
        id: id.to_string(),
        hostname: format!("{}-host", id),
        platform,
        locked,
        online,
        current_job: None,
        current_run_id: None,
        pr_ref: None,
        last_heartbeat: 0,
    }
}

// CLI-01: Args parse orchestrator URL
#[test]
fn test_args_parse() {
    #[derive(Parser, Debug)]
    struct Args {
        #[arg(long, env = "ORCHESTRATOR_URL", default_value = "http://localhost:8080")]
        orchestrator_url: String,
    }

    let args = Args::parse_from(["cmd", "--orchestrator-url", "http://foo:9090"]);
    assert_eq!(args.orchestrator_url, "http://foo:9090");
}

// CLI-03: Staleness style green for age < 10s
#[test]
fn test_staleness_style_green() {
    let style = staleness_style(5);
    assert_eq!(style.fg, Some(Color::Green));
}

// CLI-03: Staleness style yellow for 10..=60s
#[test]
fn test_staleness_style_yellow() {
    let style = staleness_style(30);
    assert_eq!(style.fg, Some(Color::Yellow));
}

// CLI-03: Staleness style red for > 60s
#[test]
fn test_staleness_style_red() {
    let style = staleness_style(90);
    assert_eq!(style.fg, Some(Color::Red));
}

// CLI-06: platform_unlocked_count
#[test]
fn test_platform_coverage_count() {
    let mut app = App::new();
    app.runners = vec![
        make_runner("r1", Platform::Pearl, false, true),
        make_runner("r2", Platform::Pearl, false, true),
        make_runner("r3", Platform::Diamond, true, true),
    ];
    assert_eq!(app.platform_unlocked_count(Platform::Pearl), 2);
    assert_eq!(app.platform_unlocked_count(Platform::Diamond), 0);
}

// CLI-05: selected_runner returns None when no selection
#[test]
fn test_selected_runner_returns_none_when_empty() {
    let app = App::new();
    assert!(app.selected_runner().is_none());
}

// CLI-02: runner_table builds Table widget
#[test]
fn test_runner_table_rows() {
    let runner = make_runner("r1", Platform::Diamond, false, true);
    let _table_one = runner_table(&[runner]);
    let _table_empty = runner_table(&[]);
    // Verifies it builds Table widgets without panicking
}

// CLI-04: App::handle_event updates runners on RunnersUpdated
#[tokio::test]
async fn test_app_handle_runners_updated() {
    let mut app = App::new();
    let runner = make_runner("r1", Platform::Diamond, false, true);
    let client = reqwest::Client::new();
    app.handle_event(
        Event::RunnersUpdated(vec![runner]),
        &client,
        "http://localhost:8080",
    )
    .await;
    assert_eq!(app.runners.len(), 1);
}

// CLI-04: App::handle_event sets should_quit on 'q'
#[tokio::test]
async fn test_app_quit_on_q() {
    let mut app = App::new();
    let client = reqwest::Client::new();
    app.handle_event(
        Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
        &client,
        "http://localhost:8080",
    )
    .await;
    assert!(app.should_quit);
}

// CLI-04: App::handle_event moves selection down on Down arrow
#[tokio::test]
async fn test_app_navigation_down() {
    let mut app = App::new();
    app.runners = vec![
        make_runner("r1", Platform::Diamond, false, true),
        make_runner("r2", Platform::Diamond, false, true),
        make_runner("r3", Platform::Diamond, false, true),
    ];
    let client = reqwest::Client::new();

    app.handle_event(
        Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
        &client,
        "http://localhost:8080",
    )
    .await;
    assert_eq!(app.table_state.selected(), Some(0));

    app.handle_event(
        Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
        &client,
        "http://localhost:8080",
    )
    .await;
    assert_eq!(app.table_state.selected(), Some(1));
}

// CLI-07: lock_runner returns Err on 409 with body message
#[tokio::test]
async fn test_lock_409_message() {
    use hiltop::api::lock_runner;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/runners/abc/lock"))
        .respond_with(
            ResponseTemplate::new(409).set_body_json(
                serde_json::json!({"error": "cannot lock: would leave zero online unlocked runners for platform"}),
            ),
        )
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let result = lock_runner(&client, &server.uri(), "abc").await;
    assert_eq!(
        result,
        Err(
            "cannot lock: would leave zero online unlocked runners for platform".to_string()
        )
    );
}

// CLI-07: lock_runner returns Ok on 200
#[tokio::test]
async fn test_lock_success() {
    use hiltop::api::lock_runner;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/runners/r1/lock"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let result = lock_runner(&client, &server.uri(), "r1").await;
    assert_eq!(result, Ok(()));
}

// CLI-07: unlock_runner returns Ok on 200
#[tokio::test]
async fn test_unlock_success() {
    use hiltop::api::unlock_runner;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/runners/r1/unlock"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let result = unlock_runner(&client, &server.uri(), "r1").await;
    assert_eq!(result, Ok(()));
}

// CLI-05: pressing 'l' on the last unlocked runner enters Confirm with warn_last=true
#[tokio::test]
async fn test_confirm_mode_entered_on_l() {
    let mut app = App::new();
    app.runners = vec![make_runner("r1", Platform::Diamond, false, true)];
    app.table_state.select(Some(0));

    let client = reqwest::Client::new();
    app.handle_event(
        Event::Key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE)),
        &client,
        "http://localhost",
    )
    .await;

    match &app.mode {
        Mode::Confirm {
            runner_id,
            action: LockAction::Lock,
            warn_last,
        } => {
            assert_eq!(runner_id, "r1");
            assert!(*warn_last, "should warn: only 1 unlocked Diamond runner");
        }
        other => panic!("expected Mode::Confirm, got {other:?}"),
    }
}

// CLI-05: pressing 'l' with 2 unlocked runners enters Confirm with warn_last=false
#[tokio::test]
async fn test_confirm_mode_entered_no_warn() {
    let mut app = App::new();
    app.runners = vec![
        make_runner("r1", Platform::Diamond, false, true),
        make_runner("r2", Platform::Diamond, false, true),
    ];
    app.table_state.select(Some(0));

    let client = reqwest::Client::new();
    app.handle_event(
        Event::Key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE)),
        &client,
        "http://localhost",
    )
    .await;

    match &app.mode {
        Mode::Confirm {
            runner_id,
            action: LockAction::Lock,
            warn_last,
        } => {
            assert_eq!(runner_id, "r1");
            assert!(!*warn_last, "should NOT warn: 2 unlocked Diamond runners");
        }
        other => panic!("expected Mode::Confirm, got {other:?}"),
    }
}

// CLI-05: pressing 'n' in Confirm mode returns to Dashboard without HTTP call
#[tokio::test]
async fn test_cancel_confirm_on_n() {
    let mut app = App::new();
    app.mode = Mode::Confirm {
        runner_id: "r1".to_string(),
        action: LockAction::Lock,
        warn_last: false,
    };

    let client = reqwest::Client::new();
    app.handle_event(
        Event::Key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)),
        &client,
        "http://localhost",
    )
    .await;

    assert!(
        matches!(app.mode, Mode::Dashboard),
        "expected Mode::Dashboard after 'n'"
    );
}

// CLI-09: fetch_results returns Err when stub called — Wave 1 impl
#[test]
#[ignore]
fn test_results_query_stub() {
    // api.rs is a stub; will be implemented in a later plan
    // fetch_results should return Err when called against the stub
}
