use crossterm::event::{
    self, Event as CrosstermEvent, KeyCode, KeyModifiers, MouseButton, MouseEventKind,
};
use tokio::sync::mpsc::UnboundedSender;

#[derive(Debug, Clone)]
pub enum Event {
    Key(KeyCode, KeyModifiers),
    /// Left mouse button click at (column, row).
    Mouse(u16, u16),
    Tick,
}

pub fn spawn_event_task(tx: UnboundedSender<Event>) {
    tokio::spawn(async move {
        loop {
            // Poll for crossterm events with a 250ms timeout for tick
            let result = tokio::task::spawn_blocking(|| {
                if event::poll(std::time::Duration::from_millis(250)).unwrap_or(false) {
                    event::read().ok()
                } else {
                    None
                }
            })
            .await;

            match result {
                Ok(Some(CrosstermEvent::Key(key))) => {
                    if tx.send(Event::Key(key.code, key.modifiers)).is_err() {
                        break;
                    }
                }
                Ok(Some(CrosstermEvent::Mouse(mouse))) => {
                    if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                        && tx.send(Event::Mouse(mouse.column, mouse.row)).is_err()
                    {
                        break;
                    }
                }
                Ok(None) => {
                    // Timeout — send a tick
                    if tx.send(Event::Tick).is_err() {
                        break;
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });
}
