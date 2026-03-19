use chrono::Utc;
use orb_hil_types::RunnerStatus;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::Text,
    widgets::{Block, Borders, Cell, Row, Table},
};

use crate::app::App;

pub fn render_dashboard(frame: &mut ratatui::Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .split(frame.area());

    let header_text = Text::raw("HIL Runner Dashboard");
    frame.render_widget(header_text, chunks[0]);

    let table = runner_table(&app.runners);
    frame.render_stateful_widget(table, chunks[1], &mut app.table_state);

    let status = app
        .status_msg
        .as_deref()
        .unwrap_or("q: quit  ↑↓: navigate  l: lock  u: unlock");
    let status_text = Text::raw(status);
    frame.render_widget(status_text, chunks[2]);
}

pub fn staleness_style(age_secs: i64) -> Style {
    if age_secs < 10 {
        Style::default().fg(Color::Green)
    } else if age_secs <= 60 {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Red)
    }
}

pub fn runner_table(runners: &[RunnerStatus]) -> Table<'static> {
    let header = Row::new(vec![
        Cell::from("Hostname").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Orb ID").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Platform").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Status").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Locked").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Job/PR").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Heartbeat").style(Style::default().add_modifier(Modifier::BOLD)),
    ])
    .height(1);

    let now = Utc::now().timestamp();
    let rows: Vec<Row<'static>> = runners
        .iter()
        .map(|r| {
            let age = now - r.last_heartbeat;
            let style = staleness_style(age);
            let online_label = if r.online { "online" } else { "OFFLINE" };
            let locked_label = if r.locked { "LOCKED" } else { "unlocked" };
            let pr_number = r.pr_ref.as_deref().and_then(|s| {
                // "refs/pull/123/merge" → "#123"
                s.strip_prefix("refs/pull/")
                    .and_then(|rest| rest.split('/').next())
                    .map(|n| format!("#{n}"))
            });
            let job_label = match (&r.current_job, &pr_number) {
                (Some(job), Some(pr)) => format!("{job} {pr}"),
                (Some(job), None) => job.clone(),
                (None, Some(pr)) => pr.clone(),
                (None, None) => "-".to_string(),
            };
            let age_label = format!("{age}s");

            Row::new(vec![
                Cell::from(r.hostname.clone()),
                Cell::from(r.id.clone()),
                Cell::from(r.platform.to_string()),
                Cell::from(online_label).style(style),
                Cell::from(locked_label),
                Cell::from(job_label),
                Cell::from(age_label).style(style),
            ])
        })
        .collect();

    Table::new(
        rows,
        [
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(10),
            Constraint::Percentage(10),
            Constraint::Percentage(10),
            Constraint::Percentage(20),
            Constraint::Percentage(10),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL).title("Runners"))
    .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
}
