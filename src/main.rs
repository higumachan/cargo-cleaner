use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};
use std::{error::Error, io};

use cargo_cleaner::{find_cargo_projects, ProjectTargetAnalysis};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use itertools::Itertools;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui_multi_highlight_table::{
    Cell, Highlight, MultiHighlightTable, Row, RowHighlight, RowId, TableState,
};
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
    remove_popup: Arc<RwLock<bool>>,
    state: TableState,
    items: Arc<Mutex<Vec<ProjectTargetAnalysis>>>,
    selected_items: HashSet<RowId>,
    is_loading: Arc<Mutex<bool>>,
}

impl App {
    fn new() -> App {
        App {
            remove_popup: Arc::new(RwLock::new(false)),
            state: TableState::default(),
            items: Arc::new(Mutex::new(vec![])),
            selected_items: HashSet::new(),
            is_loading: Arc::new(Mutex::new(true)),
        }
    }
    pub fn next(&mut self) {
        let id = self.state.selected();
        let items = self.items.lock().unwrap();
        let i = if let Some(id) = id {
            let i = items
                .iter()
                .find_position(|it| id == RowId::from(it.id))
                .map(|t| t.0);
            match i {
                Some(i) => {
                    if i < items.len() - 1 {
                        i + 1
                    } else {
                        i
                    }
                }
                None => 0,
            }
        } else {
            0
        };
        self.state.select(
            items
                .get(i)
                .map(|t| t.id.into())
                .or_else(|| items.get(0).map(|t| t.id.into())),
        );
    }

    pub fn previous(&mut self) {
        let id = self.state.selected();
        let items = self.items.lock().unwrap();
        let i = if let Some(id) = id {
            let i = items
                .iter()
                .find_position(|it| id == RowId::from(it.id))
                .map(|t| t.0);
            match i {
                Some(i) => {
                    if i > 0 {
                        i - 1
                    } else {
                        i
                    }
                }
                None => 0,
            }
        } else {
            0
        };
        self.state.select(
            items
                .get(i)
                .map(|t| t.id.into())
                .or_else(|| items.get(0).map(|t| t.id.into())),
        );
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
    let app = App::new();
    let items = app.items.clone();

    std::thread::spawn(move || {
        for analysis in analysis_receiver {
            match analysis {
                Ok(analysis) => {
                    if analysis.size > 0 {
                        let mut items = items.lock().unwrap();
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
        *is_loading.lock().unwrap() = false;
    });

    let res = run_app(&mut terminal, app);

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

fn run_app<B: Backend>(terminal: &mut Terminal<B>, mut app: App) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui(f, &mut app))?;

        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Char('r') => {
                        let mut remove_popup = app.remove_popup.write().unwrap();
                        *remove_popup = !*remove_popup;
                    }
                    KeyCode::Char('Y') => {
                        if *app.remove_popup.read().unwrap() {
                            let items = app.items.lock().unwrap();
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
                                *remove_popup.write().unwrap() = false;
                            });
                        }
                    }
                    KeyCode::Down => app.next(),
                    KeyCode::Up => app.previous(),
                    KeyCode::Char(' ') => {
                        if let Some(selected) = app.state.selected() {
                            if app.selected_items.contains(&selected) {
                                app.selected_items.remove(&selected);
                                app.state
                                    .remove_row_state(selected, RowHighlight::UserHighlight0)
                            } else {
                                app.selected_items.insert(selected);
                                app.state
                                    .add_row_state(selected, RowHighlight::UserHighlight0)
                            }
                        }
                    }
                    _ => {}
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
    let header = Row::new(Uuid::new_v4().into(), ProjectTargetAnalysis::header());
    let items = app.items.lock().unwrap();
    let rows = items.iter().map(|item| {
        let cells = item.cells();
        Row::new(item.id.into(), cells).height(1).bottom_margin(0)
    });
    let t =
        MultiHighlightTable::new(rows)
            .header(header)
            .block(Block::default().borders(Borders::ALL).title(
                if *app.is_loading.lock().unwrap() {
                    "Now loading crates "
                } else {
                    "Finished loading"
                },
            ))
            .add_highlight(
                RowHighlight::UserHighlight0,
                Highlight::default().style(Style::default().bg(Color::Yellow)),
            )
            .selected_highlight(Highlight::default().style(selected_style).symbol(">> "))
            .widths(&[
                Constraint::Percentage(50),
                Constraint::Max(30),
                Constraint::Max(10),
            ]);
    f.render_stateful_widget(t, rects[0], &mut app.state);

    if *app.remove_popup.read().unwrap() {
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
