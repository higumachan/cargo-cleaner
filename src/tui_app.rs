use crossterm::event::KeyCode;
use itertools::Itertools;
use ratatui::prelude::*;
use ratatui::widgets::*;
use std::collections::HashSet;
use std::sync::mpsc::SyncSender;
use std::sync::Arc;
use uuid::Uuid;

use crate::notify_rw_lock::NotifyRwLock;
use crate::Progress;
use crate::ProjectTargetAnalysis;
use crate::GIB_SIZE;

const DELETE_COMMAND_KEY: char = 'd';
const COLUMNS: usize = 3;

pub trait TableRow {
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
            Cell::from(format!("{:.2}GiB", self.size as f64 / (GIB_SIZE as f64)))
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

pub struct App {
    pub table_state: TableState,
    pub items: Arc<NotifyRwLock<Vec<ProjectTargetAnalysis>>>,
    pub selected_items: HashSet<Uuid>,
    pub scan_progress: Arc<NotifyRwLock<Progress>>,
    pub delete_state: Option<DeleteState>,
    pub dry_run: bool,
    pub mode: CursorMode,
    pub show_help_popup: bool,
    pub notify_tx: SyncSender<()>,
}

impl App {
    pub fn new(
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

    pub fn handle_key(&mut self, key: KeyCode) -> Option<()> {
        match key {
            KeyCode::Char('q') => return None,
            KeyCode::Char(DELETE_COMMAND_KEY) => {
                let mut is_reset = false;
                match &self.delete_state {
                    Some(DeleteState::Confirm) => {}
                    Some(DeleteState::Deleting(delete_progress)) => {
                        let progress = delete_progress.read();
                        if progress.scanned == progress.total {
                            self.items
                                .write()
                                .retain(|it| !self.selected_items.contains(&it.id));
                            self.selected_items.clear();
                            is_reset = true;
                        }
                    }
                    None => {
                        if !self.selected_items.is_empty() {
                            self.delete_state = Some(DeleteState::Confirm);
                        }
                    }
                }
                if is_reset {
                    self.delete_state = None;
                }
            }
            KeyCode::Char('Y') => {
                if let Some(DeleteState::Confirm) = self.delete_state {
                    let items = self.items.read();
                    let selected_items = self.selected_items.clone();
                    let remove_targets = items
                        .iter()
                        .filter(|it| selected_items.contains(&it.id))
                        .cloned()
                        .collect_vec();

                    assert_eq!(remove_targets.len(), selected_items.len());

                    let delete_progress = Arc::new(NotifyRwLock::new(
                        self.notify_tx.clone(),
                        Progress {
                            total: remove_targets.len(),
                            scanned: 0,
                        },
                    ));
                    self.delete_state = Some(DeleteState::Deleting(delete_progress.clone()));
                    let dry_run = self.dry_run;
                    std::thread::spawn(move || {
                        for target in remove_targets {
                            if dry_run {
                                std::thread::sleep(std::time::Duration::from_millis(1000));
                            } else {
                                std::process::Command::new("cargo")
                                    .arg("clean")
                                    .current_dir(target.project_path.clone())
                                    .stderr(std::process::Stdio::null())
                                    .spawn()
                                    .expect("failed to execute process")
                                    .wait()
                                    .expect("failed to wait for process");
                            }
                            delete_progress.write().scanned += 1;
                        }
                    });
                }
            }
            KeyCode::Char('n') => {
                if let Some(DeleteState::Confirm) = self.delete_state {
                    self.delete_state = None;
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.next();
                after_move(self);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.previous();
                after_move(self);
            }
            KeyCode::Char('g') => {
                self.table_state.select(Some(0));
                after_move(self);
            }
            KeyCode::Char('G') => {
                {
                    let items = self.items.read();
                    self.table_state.select(Some(items.len() - 1));
                }
                after_move(self);
            }
            KeyCode::Char(' ') => {
                if let Some(selected) = self.table_state.selected() {
                    let selected_id = self.items.read()[selected].id;
                    if self.selected_items.contains(&selected_id) {
                        self.selected_items.remove(&selected_id);
                    } else {
                        self.selected_items.insert(selected_id);
                    }
                }
            }
            KeyCode::Char('v') => {
                self.mode = CursorMode::Select;
                after_move(self);
            }
            KeyCode::Char('V') => {
                self.mode = CursorMode::Unselect;
                after_move(self);
            }
            KeyCode::Char('h') => {
                self.show_help_popup = !self.show_help_popup;
            }
            KeyCode::Esc => {
                self.show_help_popup = false;
                self.mode = CursorMode::Normal;
            }
            _ => {}
        }
        Some(())
    }
}

pub fn after_move(app: &mut App) {
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

pub fn ui(f: &mut Frame, app: &mut App) {
    let height = f.area().height;
    let rects = Layout::default()
        .direction(Direction::Vertical)
        .spacing(0)
        .constraints([
            Constraint::Max(1),
            Constraint::Max(height - 2),
            Constraint::Max(1),
        ])
        .split(f.area());

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
        let t = Table::new(
            rows,
            &[
                Constraint::Percentage(50),
                Constraint::Max(30),
                Constraint::Max(10),
            ],
        )
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(format!(
            "Cargo Cleaner {}",
            if app.dry_run { "(dry-run)" } else { "" }
        )))
        .row_highlight_style(selected_style)
        .highlight_symbol(">> ");
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

        let size = f.area();
        let area = sized_centered_rect(width as u16, height as u16, size);
        f.render_widget(Clear, area);

        let block = Block::default().title("Help").borders(Borders::ALL);
        let paragraph = Paragraph::new(text).block(block);
        f.render_widget(paragraph, area);
    }

    delete_popup(f, app);
}

pub fn delete_popup(f: &mut Frame, app: &mut App) {
    if let Some(delete_state) = &app.delete_state {
        let size = f.area();
        let block = Block::default().borders(Borders::ALL);
        let area = centered_rect(60, 30, size);
        f.render_widget(Clear, area);
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

pub fn status_bar(f: &mut Frame, app: &mut App, rect: Rect) {
    let rects = Layout::default()
        .direction(Direction::Horizontal)
        .spacing(0)
        .constraints([
            Constraint::Min(50),
            Constraint::Min(20),
            Constraint::Min(10),
        ])
        .split(rect);
    let items = app.items.read();
    let total_gib_size = items.iter().map(|it| it.size).sum::<u64>() as f64 / (GIB_SIZE as f64);
    let selected_gib_size = items
        .iter()
        .filter(|it| app.selected_items.contains(&it.id))
        .map(|it| it.size)
        .sum::<u64>() as f64
        / (GIB_SIZE as f64);

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
        .spacing(0)
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
        format!(
            "Deleting {:6} / {:6} {}",
            progress.scanned,
            progress.total,
            if dry_run { "(dry-run)" } else { "" }
        )
    }
}
