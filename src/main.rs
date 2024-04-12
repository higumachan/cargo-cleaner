use clap::Parser;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::SyncSender;
use std::sync::Arc;
use std::{error::Error, io};

use cargo_cleaner::notify_rw_lock::NotifyRwLock;
use cargo_cleaner::tui::{Event, Tui};
use cargo_cleaner::{find_cargo_projects, Progress, ProjectTargetAnalysis};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture, Event as CrosstermEvent, KeyCode},
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
            Cell::from(self.project_path.to_str().unwrap()).style(Style::default()),
            Cell::from(self.project_name.as_deref().unwrap_or("NOT FOUND NAME"))
                .style(Style::default()),
            Cell::from(format!(
                "{:.2}GiB",
                self.size as f64 / (1024.0 * 1024.0 * 1024.0)
            ))
            .style(Style::default()),
        ]
    }
}

pub enum DeleteState {
    Confirm,
    Deleting(Arc<NotifyRwLock<Progress>>),
}

pub enum CursorMode {
    Normal,
    Select,
    Unselect,
}

struct App {
    table_state: TableState,
    items: Arc<NotifyRwLock<Vec<ProjectTargetAnalysis>>>,
    selected_items: HashSet<Uuid>,
    scan_progress: Arc<NotifyRwLock<Progress>>,
    delete_state: Option<DeleteState>,
    dry_run: bool,
    mode: CursorMode,
    show_help_popup: bool,
    notify_tx: SyncSender<()>,
}

impl App {
    fn new(
        dry_run: bool,
        notify_tx: SyncSender<()>,
        scan_progress: Arc<NotifyRwLock<Progress>>,
    ) -> App {
        App {
            table_state: TableState::default(),
            items: Arc::new(NotifyRwLock::new(notify_tx.clone(), vec![])),
            selected_items: HashSet::new(),
            scan_progress,
            delete_state: None,
            mode: CursorMode::Normal,
            show_help_popup: false,
            dry_run,
            notify_tx,
        }
    }
    pub fn next(&mut self) {
        let i = match self.table_state.selected() {
            Some(i) => {
                if i >= self.items.read().len() - 1 {
                    i
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    pub fn previous(&mut self) {
        let i = match self.table_state.selected() {
            Some(i) => {
                if i == 0 {
                    i
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
    }
}

#[derive(Parser)] // requires `derive` feature
#[command(name = "cargo")]
#[command(bin_name = "cargo")]
enum CargoCli {
    Cleaner(Args),
}

#[derive(clap::Args)]
#[command(author, version, about)]
struct Args {
    #[arg(long, default_value = "false")]
    dry_run: bool,
    #[arg(short = 'r', long)]
    search_root: Option<String>,
    #[arg(short = 'p', long)]
    scan_workers: Option<usize>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let CargoCli::Cleaner(args) = CargoCli::parse();

    // start find job
    let (notify_tx, notify_rx) = std::sync::mpsc::sync_channel(1);

    let search_root = args
        .search_root
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().expect("can not found HOME_DIR"));

    let scan_workers = args.scan_workers.unwrap_or((num_cpus::get() - 1).max(1));
    let (analysis_receiver, scan_progress) =
        find_cargo_projects(search_root.as_path(), scan_workers, notify_tx.clone());

    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // create app and run it
    let app = App::new(args.dry_run, notify_tx, scan_progress.clone());
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

#[allow(clippy::single_match)]
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
                                        app.items
                                            .write()
                                            .retain(|it| !app.selected_items.contains(&it.id));
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
                                    .filter(|it| selected_items.contains(&it.id))
                                    .cloned()
                                    .collect_vec();

                                assert_eq!(remove_targets.len(), selected_items.len());

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
                                            std::process::Command::new("cargo")
                                                .arg("clean")
                                                .current_dir(target.project_path.clone())
                                                .spawn()
                                                .expect("failed to execute process");
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
                        KeyCode::Char('j') | KeyCode::Down => {
                            app.next();
                            after_move(&mut app);
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            app.previous();
                            after_move(&mut app);
                        }
                        KeyCode::Char('g') => {
                            app.table_state.select(Some(0));
                            after_move(&mut app);
                        }
                        KeyCode::Char('G') => {
                            {
                                let items = app.items.read();
                                app.table_state.select(Some(items.len() - 1));
                            }
                            after_move(&mut app);
                        }
                        KeyCode::Char(' ') => {
                            if let Some(selected) = app.table_state.selected() {
                                let selected_id = app.items.read()[selected].id;
                                if app.selected_items.contains(&selected_id) {
                                    app.selected_items.remove(&selected_id);
                                } else {
                                    app.selected_items.insert(selected_id);
                                }
                            }
                        }
                        KeyCode::Char('v') => {
                            app.mode = CursorMode::Select;
                            after_move(&mut app); // select current selected item
                        }
                        KeyCode::Char('V') => {
                            app.mode = CursorMode::Unselect;
                            after_move(&mut app); // unselect current selected item
                        }
                        KeyCode::Char('h') => {
                            app.show_help_popup = !app.show_help_popup;
                        }
                        KeyCode::Esc => {
                            app.show_help_popup = false;
                            app.mode = CursorMode::Normal;
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

fn after_move(app: &mut App) {
    match app.mode {
        CursorMode::Normal => {}
        CursorMode::Select => {
            if let Some(selected) = app.table_state.selected() {
                let selected_id = app.items.read()[selected].id;
                app.selected_items.insert(selected_id);
            }
        }
        CursorMode::Unselect => {
            if let Some(selected) = app.table_state.selected() {
                let selected_id = app.items.read()[selected].id;
                app.selected_items.remove(&selected_id);
            }
        }
    }
}

fn ui(f: &mut Frame, app: &mut App) {
    let height = f.size().height;
    let rects = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Max(1),
            Constraint::Max(height - 2),
            Constraint::Max(1),
        ])
        .split(f.size());

    {
        let selected_style = Style::default().fg(Color::White).bg(Color::Green);
        let header = Row::new(ProjectTargetAnalysis::header());
        let items = app.items.read();
        let rows = items.iter().map(|item| {
            let cells = item.cells();
            let row = Row::new(cells).height(1).bottom_margin(0);
            if app.selected_items.contains(&item.id) {
                row.style(Style::default().fg(Color::Blue).bg(Color::Yellow))
            } else {
                row.style(Style::default().fg(Color::Green))
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
        f.render_stateful_widget(t, rects[1], &mut app.table_state);
    }

    {
        let scan_progress = app.scan_progress.read();
        let gauge = Gauge::default()
            .block(Block::default())
            .gauge_style(Style::new().light_green().on_gray())
            .percent(progress_percent(&scan_progress))
            .label(Span::styled(
                progress_text(&scan_progress),
                Style::default().fg(Color::Black),
            ));

        f.render_widget(gauge, rects[0]);
    }

    status_bar(f, app, rects[2]);

    if app.show_help_popup {
        let text = Text::styled(
            "h      : toggle help\n\
             j or ↓ : move down\n\
             k or ↑ : move up\n\
             g      : move to top\n\
             G      : move to bottom\n\
             space  : toggle select\n\
             v      : into select mode\n\
             V      : into unselect mode\n\
             d      : open delete window\n\
             q      : quit",
            Style::default().fg(Color::Yellow),
        );

        let width = text.width() + 2;
        let height = text.height() + 2;

        let size = f.size();
        let area = sized_centered_rect(width as u16, height as u16, size);
        f.render_widget(Clear, area); //this clears out the background

        let block = Block::default().title("Help").borders(Borders::ALL);
        let paragraph = Paragraph::new(text).block(block);
        f.render_widget(paragraph, area);
    }

    delete_popup(f, app);
}

fn delete_popup(f: &mut Frame, app: &mut App) {
    if let Some(delete_state) = &app.delete_state {
        let size = f.size();
        let block = Block::default().borders(Borders::ALL);
        let area = centered_rect(60, 30, size);
        f.render_widget(Clear, area); //this clears out the background
        let gauge = Gauge::default()
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
                    .percent(progress_percent(&progress))
                    .label(Span::styled(
                        delete_progress_text(&progress, app.dry_run),
                        Style::default().fg(Color::Yellow),
                    ))
            }
        };

        f.render_widget(gauge, area);
    }
}

fn status_bar(f: &mut Frame, app: &mut App, rect: Rect) {
    let rects = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(50),
            Constraint::Min(20),
            Constraint::Min(10),
        ])
        .split(rect);
    let items = app.items.read();
    let total_gib_size =
        items.iter().map(|it| it.size).sum::<u64>() as f64 / (1024.0 * 1024.0 * 1024.0);
    let selected_gib_size = items
        .iter()
        .filter(|it| app.selected_items.contains(&it.id))
        .map(|it| it.size)
        .sum::<u64>() as f64
        / (1024.0 * 1024.0 * 1024.0);

    let status_text = format!(
        "Total: {:.2} GiB, Selected: {:.2} GiB",
        total_gib_size, selected_gib_size
    );
    let text = Span::styled(status_text, Style::default().fg(Color::Green));
    let block = Block::default();
    let paragraph = Paragraph::new(text).block(block);
    f.render_widget(paragraph, rects[0]);

    let help_text = Span::styled("h: help", Style::default().fg(Color::Green));
    let block = Block::default();
    let paragraph = Paragraph::new(help_text).block(block);
    f.render_widget(paragraph, rects[1]);

    let mode_text = match app.mode {
        CursorMode::Normal => "Normal",
        CursorMode::Select => "Select",
        CursorMode::Unselect => "Unselect",
    };
    let text = Span::styled(mode_text, Style::default().fg(Color::White).bg(Color::Blue));
    let block = Block::default();
    let paragraph = Paragraph::new(text)
        .block(block)
        .alignment(Alignment::Right);
    f.render_widget(paragraph, rects[2]);
}

fn sized_centered_rect(min_width: u16, min_height: u16, r: Rect) -> Rect {
    let margin_side = (r.width - min_width) / 2;
    let width = r.width - margin_side * 2;
    let margin_top = (r.height - min_height) / 2;
    let height = r.height - margin_top * 2;
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Max(margin_top),
            Constraint::Max(height),
            Constraint::Max(margin_top),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Max(margin_side),
            Constraint::Max(width),
            Constraint::Max(margin_side),
        ])
        .split(popup_layout[1])[1]
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
        format!("Scanning {:6} / {:6}", progress.scanned, progress.total)
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
