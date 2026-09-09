use crossterm::event;
use crossterm::event::{Event as CrosstermEvent, KeyCode, KeyEventKind};
use ratatui::prelude::CrosstermBackend;
use std::sync::mpsc::Receiver;
use std::time::Duration;

/// Returns the key code for press and repeat events.
///
/// Windows emits both `Press` and `Release` for a single key tap. Ignoring
/// `Release` prevents the same key from being handled twice. `Repeat` is kept
/// so holding a key still moves the cursor on Windows. On macOS/Linux without
/// keyboard enhancement flags, key-repeat arrives as additional `Press`
/// events, so those platforms keep working as well.
pub fn key_code_if_pressed_or_repeat(ev: &CrosstermEvent) -> Option<KeyCode> {
    match ev {
        CrosstermEvent::Key(key)
            if key.kind == KeyEventKind::Press || key.kind == KeyEventKind::Repeat =>
        {
            Some(key.code)
        }
        _ => None,
    }
}

#[derive(Clone, Debug)]
pub enum Event {
    Parent(CrosstermEvent),
    AsyncUpdate,
}

pub struct Tui<'a> {
    terminal: &'a mut ratatui::Terminal<CrosstermBackend<std::io::Stdout>>,
    async_update_rx: Receiver<()>,
}

impl<'a> Tui<'a> {
    pub fn new(
        terminal: &'a mut ratatui::Terminal<CrosstermBackend<std::io::Stdout>>,
        async_update_rx: Receiver<()>,
    ) -> Self {
        Self {
            terminal,
            async_update_rx,
        }
    }

    pub fn draw(&mut self, f: impl FnOnce(&mut ratatui::Frame<'_>)) -> std::io::Result<()> {
        self.terminal.draw(f)?;
        Ok(())
    }

    pub fn read_event(&mut self) -> anyhow::Result<Event> {
        loop {
            if event::poll(Duration::from_micros(100))? {
                return Ok(event::read().map(Event::Parent)?);
            } else if self.async_update_rx.try_recv().is_ok() {
                return Ok(Event::AsyncUpdate);
            }
        }
    }
}
