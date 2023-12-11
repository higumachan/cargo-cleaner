use crossterm::event;
use crossterm::event::Event as CrosstermEvent;
use ratatui::prelude::CrosstermBackend;
use std::sync::mpsc::Receiver;
use std::time::Duration;

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
