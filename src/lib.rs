pub mod notify_rw_lock;
pub mod tui;
pub mod tui_app;

use crate::notify_rw_lock::{NotifyRwLock, NotifySender};
use cargo_toml::Manifest;
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;
use uuid::Uuid;

/// Job for the threaded project finder. First the path to be searched, second the sender to create
/// new jobs for recursively searching the dirs
struct Job(PathBuf, Sender<Job>);

pub struct Progress {
    pub total: usize,
    pub scanned: usize,
}

/// Recursively scan the given path for cargo projects using the specified number of threads.
///
/// When the number of threads is 0, use as many threads as virtual CPU cores.
pub fn find_cargo_projects(
    path: &Path,
    mut num_threads: usize,
    notify_tx: NotifySender,
) -> (
    Receiver<anyhow::Result<ProjectTargetAnalysis>>,
    Arc<NotifyRwLock<Progress>>,
) {
    let progress = Arc::new(NotifyRwLock::new(
        notify_tx,
        Progress {
            total: 1, // 最初に入っているディレクトリは必ずスキャンする
            scanned: 0,
        },
    ));
    if num_threads == 0 {
        num_threads = num_cpus::get();
    }

    let (result_tx, result_rx) = unbounded();
    let path = path.to_owned();
    std::thread::spawn({
        let progress = progress.clone();
        move || {
            std::thread::scope(move |scope| {
                let (job_tx, job_rx) = unbounded();

                (0..num_threads)
                    .map(|_| (job_rx.clone(), result_tx.clone()))
                    .for_each(|(job_rx, result_tx)| {
                        scope.spawn({
                            let progress = progress.clone();
                            || {
                                job_rx.into_iter().for_each(move |job| {
                                    find_cargo_projects_task(
                                        job,
                                        result_tx.clone(),
                                        progress.clone(),
                                    )
                                })
                            }
                        });
                    });

                job_tx.clone().send(Job(path, job_tx)).unwrap();
            });
        }
    });

    (result_rx, progress)
}

/// Scan the given directory and report to the results Sender if the directory contains a
/// Cargo.toml . Detected subdirectories should be queued as a new job in with the job_sender.
///
/// This function is supposed to be called by the threadpool in find_cargo_projects
fn find_cargo_projects_task(
    job: Job,
    results: Sender<anyhow::Result<ProjectTargetAnalysis>>,
    progress: Arc<NotifyRwLock<Progress>>,
) {
    let path = job.0;
    let job_sender = job.1;

    let read_dir = match path.read_dir() {
        Ok(it) => it,
        Err(_e) => {
            progress.write().scanned += 1;
            return;
        }
    };

    let (dirs, files): (Vec<_>, Vec<_>) = read_dir
        .filter_map(|it| it.ok())
        .partition(|it| it.file_type().is_ok_and(|t| t.is_dir()));
    let dirs: Vec<_> = dirs.iter().map(|it| it.path()).collect();
    let files: Vec<_> = files.iter().map(|it| it.path()).collect();

    let has_cargo_toml = files
        .iter()
        .any(|it| it.file_name().unwrap_or_default().to_string_lossy() == "Cargo.toml");

    // Iterate through the subdirectories of path, ignoring entries that caused errors
    for it in dirs {
        let filename = it.file_name().unwrap_or_default().to_string_lossy();
        match filename.as_ref() {
            // No need to search .git directories for cargo projects. Also skip .cargo directories
            // as there shouldn't be any target dirs in there. Even if there are valid target dirs,
            // they should probably not be deleted. See issue #2 (https://github.com/dnlmlr/cargo-clean-all/issues/2)
            ".git" | ".cargo" => (),
            // For directories queue a new job to search it with the threadpool
            _ => {
                job_sender
                    .send(Job(it.to_path_buf(), job_sender.clone()))
                    .unwrap();
                progress.write().total += 1;
            }
        }
    }

    // If path contains a Cargo.toml, it is a project directory
    if has_cargo_toml {
        results.send(ProjectTargetAnalysis::analyze(&path)).unwrap();
    }
    progress.write().scanned += 1;
}

#[derive(Clone, Debug)]
pub struct ProjectTargetAnalysis {
    pub id: Uuid,
    /// The path of the project without the `target` directory suffix
    pub project_path: PathBuf,
    /// Cargo project name
    pub project_name: Option<String>,
    /// The size in bytes that the target directory takes up
    pub size: u64,
    /// The timestamp of the last recently modified file in the target directory
    pub last_modified: SystemTime,
    /// Indicate that this target directory should be cleaned
    pub selected_for_cleanup: bool,
}

impl ProjectTargetAnalysis {
    /// Analyze a given project directories target directory
    pub fn analyze(path: &Path) -> anyhow::Result<Self> {
        let (size, last_modified) = Self::recursive_scan_target(path.join("target"));
        let cargo_manifest = Manifest::from_path(path.join("Cargo.toml"))?;
        Ok(Self {
            id: Uuid::new_v4(),
            project_path: path.to_owned(),
            project_name: cargo_manifest.package.map(|p| p.name),
            size,
            last_modified,
            selected_for_cleanup: false,
        })
    }

    // Recursively sum up the file sizes and find the last modified timestamp
    fn recursive_scan_target<T: AsRef<Path>>(path: T) -> (u64, SystemTime) {
        let path = path.as_ref();

        let default = (0, SystemTime::UNIX_EPOCH);

        if !path.exists() {
            return default;
        }

        match (path.is_file(), path.metadata()) {
            (true, Ok(md)) => (md.len(), md.modified().unwrap_or(default.1)),
            _ => path
                .read_dir()
                .map(|rd| {
                    rd.filter_map(|it| it.ok().map(|it| it.path()))
                        .map(Self::recursive_scan_target)
                        .fold(default, |a, b| (a.0 + b.0, a.1.max(b.1)))
                })
                .unwrap_or(default),
        }
    }
}
