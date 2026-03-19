use clap::Parser;
use color_eyre::Result;
use crossterm::{
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
    },
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::panic;
use tokio::sync::mpsc;

use hiltop::{
    app::App,
    event::{keyboard_task, poller_task},
    ui,
};

#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// URL of the HIL orchestrator server
    #[arg(long, env = "ORCHESTRATOR_URL", default_value = option_env!("DEFAULT_ORCHESTRATOR_URL").unwrap_or("http://localhost:8080"))]
    orchestrator_url: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();

    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen);
        original_hook(info);
    }));

    run(args).await
}

async fn run(args: Args) -> Result<()> {
    let mut stdout = std::io::stdout();
    enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen)?;

    let result = run_app(args).await;

    disable_raw_mode()?;
    execute!(stdout, LeaveAlternateScreen)?;

    result
}

async fn run_app(args: Args) -> Result<()> {
    let backend = CrosstermBackend::new(std::io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let (tx, mut rx) = mpsc::channel(32);
    let client = reqwest::Client::new();

    tokio::spawn(keyboard_task(tx.clone()));
    tokio::spawn(poller_task(
        tx,
        client.clone(),
        args.orchestrator_url.clone(),
    ));

    let mut app = App::new();

    loop {
        terminal.draw(|f| ui::draw(f, &mut app))?;

        if let Some(ev) = rx.recv().await {
            app.handle_event(ev, &client, &args.orchestrator_url).await;
        } else {
            break;
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}
