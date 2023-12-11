use clap::Parser;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex, RwLock};
use std::{error::Error, io};

use cargo_cleaner::notify_rw_lock::{NotifyRwLock, NotifySender};
use cargo_cleaner::tui::{Event, Tui};
use cargo_cleaner::{find_cargo_projects, Progress, ProjectTargetAnalysis};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event as CrosstermEvent, KeyCode,
        KeyEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use dirs::home_dir;
use itertools::Itertools;
use ratatui::prelude::*;
use ratatui::widgets::*;
use uuid::Uuid;

const DELETE_COMMAND_KEY: char = 'd';

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

pub enum DeleteState {
    Confirm,
    Deleting(Arc<NotifyRwLock<Progress>>),
}

struct App {
    state: TableState,
    items: Arc<NotifyRwLock<Vec<ProjectTargetAnalysis>>>,
    selected_items: HashSet<Uuid>,
    is_loading: Arc<NotifyRwLock<bool>>,
    load_progress: Arc<NotifyRwLock<Progress>>,
    delete_state: Option<DeleteState>,
    dry_run: bool,
    notify_tx: SyncSender<()>,
}

impl App {
    fn new(
        dry_run: bool,
        notify_tx: SyncSender<()>,
        load_progress: Arc<NotifyRwLock<Progress>>,
    ) -> App {
        App {
            state: TableState::default(),
            items: Arc::new(NotifyRwLock::new(notify_tx.clone(), vec![])),
            selected_items: HashSet::new(),
            is_loading: Arc::new(NotifyRwLock::new(notify_tx.clone(), true)),
            load_progress,
            delete_state: None,
            dry_run,
            notify_tx,
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

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    #[arg(long, default_value = "false")]
    dry_run: bool,
    #[arg(short = 'r', long)]
    search_root: Option<String>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    // start find job
    let (notify_tx, notify_rx) = std::sync::mpsc::sync_channel(1);

    let search_root = args
        .search_root
        .as_ref()
        .map(|it| PathBuf::from(it))
        .unwrap_or_else(|| home_dir().expect("can not found HOME_DIR"));

    let (analysis_receiver, load_progress) =
        find_cargo_projects(search_root.as_path(), 8, notify_tx.clone());

    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // create app and run it
    let app = App::new(args.dry_run, notify_tx, load_progress.clone());
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
                        KeyCode::Char(DELETE_COMMAND_KEY) => {
                            let mut is_reset = false;
                            match &app.delete_state {
                                Some(DeleteState::Confirm) => {}
                                Some(DeleteState::Deleting(delete_progress)) => {
                                    let progress = delete_progress.read();
                                    if progress.scanned == progress.total {
                                        // 削除が終わっていたら、ポップアップを閉じることができる
                                        app.items.write().retain(|it| {
                                            !app.selected_items.contains(&it.id.into())
                                        });
                                        app.selected_items.clear();
                                        is_reset = true;
                                    }
                                }
                                None => {
                                    // ポップアップが閉じていれば、ポップアップを開く
                                    app.delete_state = Some(DeleteState::Confirm);
                                }
                            }
                            if is_reset {
                                app.delete_state = None;
                            }
                        }
                        KeyCode::Char('Y') => match app.delete_state {
                            Some(DeleteState::Confirm) => {
                                let items = app.items.read();
                                let selected_items = app.selected_items.clone();
                                let remove_targets = items
                                    .iter()
                                    .filter(|it| selected_items.contains(&it.id.into()))
                                    .cloned()
                                    .collect_vec();
                                let delete_progress = Arc::new(NotifyRwLock::new(
                                    app.notify_tx.clone(),
                                    Progress {
                                        total: remove_targets.len(),
                                        scanned: 0,
                                    },
                                ));
                                app.delete_state =
                                    Some(DeleteState::Deleting(delete_progress.clone()));
                                std::thread::spawn(move || {
                                    for target in remove_targets {
                                        if app.dry_run {
                                            std::thread::sleep(std::time::Duration::from_millis(
                                                1000,
                                            ));
                                        } else {
                                            std::fs::remove_dir_all(
                                                target.project_path.join("target"),
                                            )
                                            .unwrap();
                                        }
                                        delete_progress.write().scanned += 1;
                                    }
                                });
                            }
                            _ => {}
                        },
                        KeyCode::Char('n') => {
                            if let Some(DeleteState::Confirm) = app.delete_state {
                                app.delete_state = None;
                            }
                        }
                        (KeyCode::Char('j') | KeyCode::Down) => app.next(),
                        (KeyCode::Char('k') | KeyCode::Up) => app.previous(),
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
        .direction(Direction::Vertical)
        .constraints([Constraint::Max(1), Constraint::Min(0)])
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
        .block(Block::default().borders(Borders::ALL).title(format!(
            "Cargo Cleaner {}",
            if app.dry_run { "(dry-run)" } else { "" }
        )))
        .highlight_style(selected_style)
        .highlight_symbol(">> ")
        .widths(&[
            Constraint::Percentage(50),
            Constraint::Max(30),
            Constraint::Max(10),
        ]);
    f.render_stateful_widget(t, rects[1], &mut app.state);

    let load_progress = app.load_progress.read();
    let gauge = Gauge::default()
        .block(Block::default())
        .gauge_style(Style::new().light_green().on_gray())
        .percent(progress_percent(&load_progress))
        .label(Span::styled(
            progress_text(&load_progress),
            Style::default().fg(Color::Black),
        ));

    f.render_widget(gauge, rects[0]);

    if let Some(delete_state) = &app.delete_state {
        let size = f.size();
        let block = Block::default().borders(Borders::ALL);
        let area = centered_rect(60, 30, size);
        f.render_widget(Clear, area); //this clears out the background
        let mut gauge = Gauge::default()
            .block(block)
            .gauge_style(Style::new().light_blue().on_black())
            .red();

        let gauge = match delete_state {
            DeleteState::Confirm => gauge.percent(0).label(Span::styled(
                format!(
                    "Are you sure you want to delete the target directory for {} crates? (Y/n)",
                    app.selected_items.len()
                ),
                Style::default().fg(Color::Yellow),
            )),
            DeleteState::Deleting(progress) => {
                let progress = progress.read();
                gauge
                    .percent(progress_percent(&&&&&&&&&progress))
                    .label(Span::styled(
                        delete_progress_text(&progress, app.dry_run),
                        Style::default().fg(Color::Yellow),
                    ))
            }
        };

        f.render_widget(gauge, area);
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

fn progress_percent(progress: &Progress) -> u16 {
    let total = progress.total;
    let scanned = progress.scanned;
    if total == 0 {
        0
    } else {
        (scanned as f64 / total as f64 * 100.0) as u16
    }
}

fn progress_text(progress: &Progress) -> String {
    if progress.scanned == progress.total {
        "Finished".to_string()
    } else {
        format!("Loading {:6} / {:6}", progress.scanned, progress.total)
    }
}

fn delete_progress_text(progress: &Progress, dry_run: bool) -> String {
    if progress.scanned == progress.total {
        format!("Finished Please Push '{}'", DELETE_COMMAND_KEY)
    } else {
        // format!("Deleting {:6} / {:6}", progress.scanned, progress.total)
        format!(
            "Deleting {:6} / {:6} {}",
            progress.scanned,
            progress.total,
            if dry_run { "(dry-run)" } else { "" }
        )
    }
}
