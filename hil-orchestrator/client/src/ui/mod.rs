pub mod dashboard;
pub mod detail;
pub mod popup;
pub mod results;

use crate::app::{App, LockAction, Mode};
use dashboard::render_dashboard;
use popup::draw_popup;

pub fn draw(frame: &mut ratatui::Frame, app: &mut App) {
    match &app.mode.clone() {
        Mode::Dashboard => render_dashboard(frame, app),
        Mode::Detail(_id) => {
            render_dashboard(frame, app);
            // detail pane overlay rendered in plan 03-04
        }
        Mode::Results => {
            // results view rendered in plan 03-04
            render_dashboard(frame, app);
        }
        Mode::Confirm {
            runner_id,
            action,
            warn_last,
        } => {
            render_dashboard(frame, app);
            let verb = match action {
                LockAction::Lock => "Lock",
                LockAction::Unlock => "Unlock",
            };
            let mut msg = format!("{verb} runner \"{runner_id}\"?");
            if *warn_last {
                msg.push_str(
                    "\n\nWARNING: This is the last unlocked runner for this platform!",
                );
            }
            msg.push_str("\n\n[y] Confirm   [n/Esc] Cancel");
            draw_popup(frame, &msg, frame.area());
        }
    }
}
