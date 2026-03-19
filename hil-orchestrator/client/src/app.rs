use crossterm::event::{KeyCode, KeyEvent};
use orb_hil_types::{Platform, ResultRecord, RunnerStatus};
use ratatui::widgets::TableState;

use crate::api::{lock_runner, unlock_runner};
use crate::event::Event;

#[derive(Debug, Clone)]
pub enum LockAction {
    Lock,
    Unlock,
}

#[derive(Debug, Clone)]
pub enum Mode {
    Dashboard,
    Confirm {
        runner_id: String,
        action: LockAction,
        warn_last: bool,
    },
    Detail(String),
    Results,
}

#[derive(Debug, Clone, Default)]
pub struct ResultsFilter {
    pub platform: String,
    pub rts_name: String,
    pub pr_number: String,
    pub github_run_id: String,
}

pub struct App {
    pub mode: Mode,
    pub runners: Vec<RunnerStatus>,
    pub table_state: TableState,
    pub results: Vec<ResultRecord>,
    pub results_filter: ResultsFilter,
    pub status_msg: Option<String>,
    pub should_quit: bool,
}

impl App {
    pub fn new() -> Self {
        Self {
            mode: Mode::Dashboard,
            runners: Vec::new(),
            table_state: TableState::default(),
            results: Vec::new(),
            results_filter: ResultsFilter::default(),
            status_msg: None,
            should_quit: false,
        }
    }

    pub fn selected_runner(&self) -> Option<&RunnerStatus> {
        let idx = self.table_state.selected()?;
        self.runners.get(idx)
    }

    pub fn platform_unlocked_count(&self, platform: Platform) -> usize {
        self.runners
            .iter()
            .filter(|r| r.platform == platform && !r.locked)
            .count()
    }

    pub async fn handle_event(
        &mut self,
        ev: Event,
        client: &reqwest::Client,
        url: &str,
    ) {
        match ev {
            Event::RunnersUpdated(runners) => {
                self.runners = runners;
            }
            Event::ResultsUpdated(results) => {
                self.results = results;
            }
            Event::ApiError(msg) => {
                self.status_msg = Some(msg);
            }
            Event::Key(key) => {
                if let Mode::Confirm {
                    ref runner_id,
                    ref action,
                    ..
                } = self.mode.clone()
                {
                    if key.code == KeyCode::Char('y') {
                        let result = match action {
                            LockAction::Lock => lock_runner(client, url, runner_id).await,
                            LockAction::Unlock => unlock_runner(client, url, runner_id).await,
                        };
                        match result {
                            Ok(()) => self.status_msg = None,
                            Err(e) => self.status_msg = Some(e),
                        }
                        self.mode = Mode::Dashboard;
                        return;
                    }
                }
                self.handle_key(key);
            }
            Event::Tick => {}
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        match self.mode.clone() {
            Mode::Dashboard => self.handle_dashboard_key(key),
            Mode::Confirm { runner_id, action, .. } => {
                self.handle_confirm_key(key, runner_id, action);
            }
            Mode::Detail(_) | Mode::Results => {
                // handled in later plans
            }
        }
    }

    fn handle_dashboard_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.should_quit = true;
            }
            KeyCode::Down => {
                let count = self.runners.len();
                if count == 0 {
                    return;
                }
                let next = match self.table_state.selected() {
                    None => 0,
                    Some(i) => {
                        if i + 1 >= count {
                            0
                        } else {
                            i + 1
                        }
                    }
                };
                self.table_state.select(Some(next));
            }
            KeyCode::Up => {
                let count = self.runners.len();
                if count == 0 {
                    return;
                }
                let prev = match self.table_state.selected() {
                    None => count - 1,
                    Some(0) => count - 1,
                    Some(i) => i - 1,
                };
                self.table_state.select(Some(prev));
            }
            KeyCode::Char('l') => {
                if let Some(runner) = self.selected_runner() {
                    let runner_id = runner.id.clone();
                    let platform = runner.platform;
                    // warn_last if locking this runner would leave 0 unlocked runners for the platform
                    let unlocked_count = self.platform_unlocked_count(platform);
                    let warn_last = unlocked_count <= 1;
                    self.mode = Mode::Confirm {
                        runner_id,
                        action: LockAction::Lock,
                        warn_last,
                    };
                }
            }
            KeyCode::Char('u') => {
                if let Some(runner) = self.selected_runner() {
                    let runner_id = runner.id.clone();
                    self.mode = Mode::Confirm {
                        runner_id,
                        action: LockAction::Unlock,
                        warn_last: false,
                    };
                }
            }
            _ => {}
        }
    }

    fn handle_confirm_key(
        &mut self,
        key: KeyEvent,
        _runner_id: String,
        _action: LockAction,
    ) {
        match key.code {
            KeyCode::Char('n') | KeyCode::Esc => {
                self.mode = Mode::Dashboard;
            }
            // 'y' confirm handled in plan 03-03 (requires async HTTP call)
            _ => {}
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
