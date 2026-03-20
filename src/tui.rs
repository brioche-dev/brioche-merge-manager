use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::{self, Stdout};
use tokio::sync::mpsc;

use crate::{
    app::{Action, App},
    event::{spawn_event_task, Event},
    ui,
};

pub struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalGuard {
    pub fn new() -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        );
        let _ = self.terminal.show_cursor();
    }
}

pub async fn run(mut app: App) -> Result<()> {
    let mut guard = TerminalGuard::new()?;

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<Event>();
    let (action_tx, mut action_rx) = mpsc::unbounded_channel::<Action>();

    // Start crossterm event polling
    spawn_event_task(event_tx);

    // Trigger initial load
    action_tx.send(Action::Refresh)?;

    loop {
        guard.terminal.draw(|f| ui::render(f, &mut app))?;

        tokio::select! {
            Some(event) = event_rx.recv() => {
                if let Some(action) = app.handle_event(event) {
                    action_tx.send(action)?;
                }
            }
            Some(action) = action_rx.recv() => {
                app.update(action, &action_tx).await?;
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}
