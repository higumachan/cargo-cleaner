use clap::Parser;
use std::path::PathBuf;
use std::{error::Error, io};

use cargo_cleaner::find_cargo_projects;
use cargo_cleaner::tui::{Event, Tui};
use cargo_cleaner::tui_app::{ui, App};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture, Event as CrosstermEvent},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use dirs::home_dir;
use ratatui::prelude::CrosstermBackend;
use ratatui::Terminal;
use std::sync::Arc;

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
                    if app.handle_key(key.code).is_none() {
                        return Ok(());
                    }
                }
            }
        }
    }
}
