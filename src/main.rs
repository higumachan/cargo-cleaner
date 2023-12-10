use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex, RwLock};
use std::{error::Error, io};

use cargo_cleaner::notify_rw_lock::NotifyRwLock;
use cargo_cleaner::tui::{Event, Tui};
use cargo_cleaner::{find_cargo_projects, ProjectTargetAnalysis};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event as CrosstermEvent, KeyCode,
        KeyEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use itertools::Itertools;
use ratatui::prelude::*;
use ratatui::widgets::*;
use uuid::Uuid;

const COLUMNS: usize = 3;

trait TableRow {
    fn header() -> [Cell<'static>; COLUMNS];
    fn cells(&self) -> [Cell; COLUMNS];
}

impl TableRow for ProjectTargetAnalysis {
    fn header() -> [Cell<'static>; COLUMNS] {
        [
            Cell::from("Project Path").style(Style::default().fg(Color::Yellow)),
            Cell::from("Project Name").style(Style::default().fg(Color::Yellow)),
            Cell::from("Size(GiB)").style(Style::default().fg(Color::Yellow)),
        ]
    }

    fn cells(&self) -> [Cell; COLUMNS] {
        [
            Cell::from(self.project_path.to_str().unwrap())
                .style(Style::default().fg(Color::Green)),
            Cell::from(
                self.project_name
                    .as_ref()
                    .map(|name| name.as_str())
                    .unwrap_or("NOT FOUND NAME"),
            )
            .style(Style::default().fg(Color::Green)),
            Cell::from(format!(
                "{:.2}GiB",
                self.size as f64 / (1024.0 * 1024.0 * 1024.0)
            ))
            .style(Style::default().fg(Color::Green)),
        ]
    }
}

struct App {
    remove_popup: Arc<NotifyRwLock<bool>>,
    state: TableState,
    items: Arc<NotifyRwLock<Vec<ProjectTargetAnalysis>>>,
    selected_items: HashSet<Uuid>,
    is_loading: Arc<NotifyRwLock<bool>>,
}

impl App {
    fn new(notify_tx: SyncSender<()>) -> App {
        App {
            remove_popup: Arc::new(NotifyRwLock::new(notify_tx.clone(), false)),
            state: TableState::default(),
            items: Arc::new(NotifyRwLock::new(notify_tx.clone(), vec![])),
            selected_items: HashSet::new(),
            is_loading: Arc::new(NotifyRwLock::new(notify_tx.clone(), true)),
        }
    }
    pub fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.items.read().len() - 1 {
                    i
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn previous(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    i
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    // start find job
    let (analysis_receiver, finish_receiver) = find_cargo_projects(&Path::new("/Users/yuta"), 8);

    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // create app and run it
    let (notify_tx, notify_rx) = std::sync::mpsc::sync_channel(1);
    let app = App::new(notify_tx);
    let items = Arc::clone(&app.items);

    std::thread::spawn(move || {
        for analysis in analysis_receiver {
            match analysis {
                Ok(analysis) => {
                    if analysis.size > 0 {
                        let mut items = items.write();
                        let insert_index = items
                            .binary_search_by_key(&std::cmp::Reverse(analysis.size), |it| {
                                std::cmp::Reverse(it.size)
                            })
                            .unwrap_or_else(|it| it);
                        items.insert(insert_index, analysis);
                    }
                }
                Err(_err) => {}
            }
        }
    });
    let is_loading = app.is_loading.clone();
    std::thread::spawn(move || {
        finish_receiver.recv().unwrap();
        *is_loading.write() = false;
    });

    let res = run_app(&mut terminal, app, notify_rx);

    // restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{err:?}");
    }

    Ok(())
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mut app: App,
    notify_rx: std::sync::mpsc::Receiver<()>,
) -> anyhow::Result<()> {
    let mut tui = Tui::new(terminal, notify_rx);

    loop {
        tui.draw(|f| ui(f, &mut app))?;

        match tui.read_event()? {
            Event::AsyncUpdate => {}
            Event::Parent(ev) => {
                if let CrosstermEvent::Key(key) = ev {
                    match key.code {
                        KeyCode::Char('q') => return Ok(()),
                        KeyCode::Char('r') => {
                            let mut remove_popup = app.remove_popup.write();
                            *remove_popup = !*remove_popup;
                        }
                        KeyCode::Char('Y') => {
                            if *app.remove_popup.read() {
                                let items = app.items.read();
                                let selected_items = app.selected_items.clone();
                                let remove_targets = items
                                    .iter()
                                    .filter(|it| selected_items.contains(&it.id.into()))
                                    .cloned()
                                    .collect_vec();
                                let remove_popup = app.remove_popup.clone();
                                std::thread::spawn(move || {
                                    for target in remove_targets {
                                        // std::fs::remove_dir_all(target.project_path.join("target"))
                                        //     .unwrap();
                                        std::thread::sleep(std::time::Duration::from_millis(1000));
                                    }
                                    *remove_popup.write() = false;
                                });
                            }
                        }
                        KeyCode::Down => app.next(),
                        KeyCode::Up => app.previous(),
                        KeyCode::Char(' ') => {
                            if let Some(selected) = app.state.selected() {
                                let selected_id = app.items.read()[selected].id;
                                if app.selected_items.contains(&selected_id) {
                                    app.selected_items.remove(&selected_id);
                                } else {
                                    app.selected_items.insert(selected_id);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

fn ui(f: &mut Frame, app: &mut App) {
    let rects = Layout::default()
        .constraints([Constraint::Percentage(100)])
        .split(f.size());

    let selected_style = Style::default().add_modifier(Modifier::REVERSED);
    let header = Row::new(ProjectTargetAnalysis::header());
    let items = app.items.read();
    let rows = items.iter().map(|item| {
        let cells = item.cells();
        let row = Row::new(cells).height(1).bottom_margin(0);
        if app.selected_items.contains(&item.id.into()) {
            row.style(Style::default().bg(Color::Yellow))
        } else {
            row
        }
    });
    let t = Table::new(rows)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(if *app.is_loading.read() {
                    "Now loading crates "
                } else {
                    "Finished loading"
                }),
        )
        .highlight_style(selected_style)
        .highlight_symbol(">> ")
        .widths(&[
            Constraint::Percentage(50),
            Constraint::Max(30),
            Constraint::Max(10),
        ]);
    f.render_stateful_widget(t, rects[0], &mut app.state);

    if *app.remove_popup.read() {
        let size = f.size();
        let block = Block::default().borders(Borders::ALL);
        let area = centered_rect(60, 40, size);
        f.render_widget(Clear, area); //this clears out the background
        f.render_widget(
            Paragraph::new(
                "Are you sure you want to delete the target directory for these crates? (Y/n)",
            )
            .alignment(Alignment::Center)
            .block(block),
            area,
        );
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
