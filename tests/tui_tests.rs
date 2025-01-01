use cargo_cleaner::{
    notify_rw_lock::NotifyRwLock,
    tui_app::{after_move, ui, App, CursorMode, DeleteState},
    Progress, ProjectTargetAnalysis,
};
use crossterm::event::KeyCode;
use itertools::Itertools;
use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};
use std::sync::mpsc::sync_channel;
use std::sync::Arc;
use std::time::SystemTime;
use uuid::Uuid;

fn make_project_target(
    name: &str,
    size: u64,
    selected_for_cleanup: bool,
    path: Option<String>,
) -> ProjectTargetAnalysis {
    ProjectTargetAnalysis {
        project_path: std::path::PathBuf::from(path.unwrap_or_else(|| "/test/path".to_string())),
        project_name: Some(name.to_string()),
        size,
        selected_for_cleanup,
        last_modified: SystemTime::now(),
        id: Uuid::new_v4(),
    }
}

/// Test the basic TUI rendering functionality and table content
#[test]
fn test_tui_rendering() {
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let (tx, _rx) = sync_channel(1);
    let scan_progress = Arc::new(NotifyRwLock::new(
        tx.clone(),
        Progress {
            total: 0,
            scanned: 0,
        },
    ));
    let mut app = App::new(true, tx, scan_progress); // true for dry-run mode in tests

    // Add some mock data to the items
    {
        let mut items = app.items.write();
        items.push(make_project_target(
            "test-project",
            GIB_SIZE, // 1GB
            true,
            None,
        ));
    }

    // Render the UI
    terminal
        .draw(|frame| {
            ui(frame, &mut app);
        })
        .unwrap();

    let buffer = terminal.backend().buffer().clone();

    // Basic assertions about the rendered UI

    let content_str = buffer_content_to_string(&buffer);

    // Verify title and mode
    assert!(content_str.contains("Cargo Cleaner"));
    assert!(content_str.contains("(dry-run)"));

    // Verify table headers are present
    assert!(content_str.contains("Project Path"));
    assert!(content_str.contains("Project Name"));
    assert!(content_str.contains("Size(GiB)"));

    // Verify mock data is displayed
    assert!(content_str.contains("/test/path"));
    assert!(content_str.contains("1.00 GiB")); // 1GB should be displayed as 1.00 GiB
    assert!(content_str.contains("test-project")); // Verify project name is displayed
}

/// Test navigation and selection behavior
#[test]
fn test_navigation_and_selection() {
    let (tx, _rx) = sync_channel(1);
    let scan_progress = Arc::new(NotifyRwLock::new(
        tx.clone(),
        Progress {
            total: 0,
            scanned: 0,
        },
    ));
    let mut app = App::new(true, tx, scan_progress);

    // Add mock items
    {
        let mut items = app.items.write();
        for i in 0..3 {
            items.push(make_project_target(
                &format!("test-project-{}", i),
                GIB_SIZE, // 1GB
                true,
                Some(format!("/test/path{}", i)),
            ));
        }
    }

    // Test initial state
    assert_eq!(app.table_state.selected(), None);

    // Test next() navigation
    app.handle_key(KeyCode::Char('j')); // Move down
    assert_eq!(app.table_state.selected(), Some(0));
    app.handle_key(KeyCode::Char('j')); // Move down
    assert_eq!(app.table_state.selected(), Some(1));
    app.handle_key(KeyCode::Char('j')); // Move down
    assert_eq!(app.table_state.selected(), Some(2));
    app.handle_key(KeyCode::Char('j')); // Move down
    assert_eq!(app.table_state.selected(), Some(2)); // No around

    // Test previous() navigation using 'k' key
    app.handle_key(KeyCode::Char('k')); // Move up
    assert_eq!(app.table_state.selected(), Some(1));
    app.handle_key(KeyCode::Char('k')); // Move up
    assert_eq!(app.table_state.selected(), Some(0));
    app.handle_key(KeyCode::Char('k')); // Move up
    assert_eq!(app.table_state.selected(), Some(0)); // Should stay at first item

    // Test after_move behavior in different modes
    app.mode = CursorMode::Select;
    after_move(&mut app);
    {
        let items = app.items.read();
        assert!(app.selected_items.contains(&items[0].id));
    }

    app.mode = CursorMode::Unselect;
    app.handle_key(KeyCode::Char('j')); // Move down
    after_move(&mut app);
    {
        let items = app.items.read();
        assert!(!app.selected_items.contains(&items[1].id));
    }
}

/// Test cursor mode transitions and effects
#[test]
fn test_cursor_mode_transitions() {
    let (tx, _rx) = sync_channel(1);
    let scan_progress = Arc::new(NotifyRwLock::new(
        tx.clone(),
        Progress {
            total: 0,
            scanned: 0,
        },
    ));
    let mut app = App::new(true, tx, scan_progress);

    // Add a mock item
    let item_id = {
        let mut items = app.items.write();
        let item = make_project_target(
            "test-project",
            GIB_SIZE, // 1GB
            true,
            None,
        );
        let id = item.id;
        items.push(item);
        id
    };

    // Test initial state
    assert!(matches!(app.mode, CursorMode::Normal));
    assert!(!app.selected_items.contains(&item_id));

    // Position cursor on the item
    app.table_state.select(Some(0));

    // Test Select mode behavior
    app.mode = CursorMode::Select;
    after_move(&mut app);
    assert!(app.selected_items.contains(&item_id));

    // Test Unselect mode behavior
    app.mode = CursorMode::Unselect;
    after_move(&mut app);
    assert!(!app.selected_items.contains(&item_id));

    // Test Normal mode (shouldn't affect selection)
    app.mode = CursorMode::Normal;
    app.selected_items.insert(item_id);
    after_move(&mut app);
    assert!(app.selected_items.contains(&item_id));
}

/// Test help popup visibility and content
#[test]
fn test_help_popup_toggle() {
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let (tx, _rx) = sync_channel(1);
    let scan_progress = Arc::new(NotifyRwLock::new(
        tx.clone(),
        Progress {
            total: 0,
            scanned: 0,
        },
    ));
    let mut app = App::new(true, tx, scan_progress);

    // Initially help popup should be hidden
    assert!(!app.show_help_popup);
    terminal
        .draw(|frame| {
            ui(frame, &mut app);
        })
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let content = buffer_content_to_string(&buffer);
    assert!(!content.contains("Help"));

    // Toggle help popup on
    app.show_help_popup = true;
    terminal
        .draw(|frame| {
            ui(frame, &mut app);
        })
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let content = buffer_content_to_string(&buffer);
    // Verify help content is visible
    assert!(content.contains("Help"));
    assert!(content.contains("h      : toggle help"));
    assert!(content.contains("space  : toggle select"));

    // Toggle help popup off
    app.show_help_popup = false;
    terminal
        .draw(|frame| {
            ui(frame, &mut app);
        })
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let content = buffer_content_to_string(&buffer);
    assert!(!content.contains("Help"));
}
//
// /// Test delete popup rendering and state transitions
#[test]
fn test_delete_popup() {
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let (tx, _rx) = sync_channel(1);
    let scan_progress = Arc::new(NotifyRwLock::new(
        tx.clone(),
        Progress {
            total: 0,
            scanned: 0,
        },
    ));
    let mut app = App::new(true, tx, scan_progress);

    // Initially no delete popup
    assert!(app.delete_state.is_none());
    terminal
        .draw(|frame| {
            ui(frame, &mut app);
        })
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let content = buffer_content_to_string(&buffer);
    assert!(!content.contains("Are you sure"));

    // Add some items to delete
    let item_id = Uuid::new_v4();
    app.selected_items.insert(item_id);

    // Show delete confirmation
    app.delete_state = Some(DeleteState::Confirm);
    terminal
        .draw(|frame| {
            ui(frame, &mut app);
        })
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let content = buffer_content_to_string(&buffer);
    assert!(content.contains("Are you sure you want to delete"));
}

/// Test that delete popup doesn't appear when no items are selected
#[test]
fn test_no_delete_popup_when_empty_selection() {
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let (tx, _rx) = sync_channel(1);
    let scan_progress = Arc::new(NotifyRwLock::new(
        tx.clone(),
        Progress {
            total: 0,
            scanned: 0,
        },
    ));
    let mut app = App::new(true, tx, scan_progress);

    // Add some items but don't select any
    {
        let mut items = app.items.write();
        items.push(make_project_target("test-project", GIB_SIZE, false, None));
    }

    // Verify no delete popup initially
    assert!(app.delete_state.is_none());

    // Try to trigger delete popup with 'd' key when no items selected
    app.handle_key(KeyCode::Char('d'));

    // Verify delete popup did not appear
    assert!(app.delete_state.is_none());
    terminal
        .draw(|frame| {
            ui(frame, &mut app);
        })
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let content = buffer_content_to_string(&buffer);
    assert!(!content.contains("Are you sure"));
}

/// Test status bar rendering
#[test]
fn test_status_bar() {
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let (tx, _rx) = sync_channel(1);
    let scan_progress = Arc::new(NotifyRwLock::new(
        tx.clone(),
        Progress {
            total: 0,
            scanned: 0,
        },
    ));
    let mut app = App::new(true, tx, scan_progress);

    // Test initial status
    terminal
        .draw(|frame| {
            ui(frame, &mut app);
        })
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let content = buffer_content_to_string(&buffer);
    assert!(content.contains("Total:"));
    assert!(content.contains("Selected:"));
    assert!(content.contains("h: help"));

    // Test mode display
    app.mode = CursorMode::Select;
    terminal
        .draw(|frame| {
            ui(frame, &mut app);
        })
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let content = buffer_content_to_string(&buffer);
    assert!(content.contains("Select"));

    app.mode = CursorMode::Unselect;
    terminal
        .draw(|frame| {
            ui(frame, &mut app);
        })
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let content = buffer_content_to_string(&buffer);
    assert!(content.contains("Unselect"));
}

/// Test clean operation in dry-run mode
#[test]
fn test_clean_operation() {
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let (tx, _rx) = sync_channel(1);
    let scan_progress = Arc::new(NotifyRwLock::new(
        tx.clone(),
        Progress {
            total: 0,
            scanned: 0,
        },
    ));
    let mut app = App::new(true, tx, scan_progress); // true for dry-run mode

    // Add mock items to clean
    {
        let mut items = app.items.write();
        items.push(make_project_target(
            "test-project-1",
            GIB_SIZE, // 1GB
            true,
            Some("/test/path1".to_string()),
        ));
        items.push(make_project_target(
            "test-project-2",
            2 * GIB_SIZE, // 2GB
            true,
            Some("/test/path2".to_string()),
        ));
    }

    // Initial render to verify items are present
    terminal
        .draw(|frame| {
            ui(frame, &mut app);
        })
        .unwrap();

    let initial_buffer = terminal.backend().buffer().clone();
    let initial_content = buffer_content_to_string(&initial_buffer);

    // Verify both items are initially visible
    assert!(initial_content.contains("/test/path1"));
    assert!(initial_content.contains("test-project-1"));
    assert!(initial_content.contains("/test/path2"));
    assert!(initial_content.contains("test-project-2"));

    // Select items for cleanup
    {
        let items = app.items.read();
        for item in items.iter() {
            app.selected_items.insert(item.id);
        }
    }

    // Trigger clean operation
    app.handle_key(KeyCode::Char('d')); // Open delete confirmation
    assert!(matches!(app.delete_state, Some(DeleteState::Confirm)));

    app.handle_key(KeyCode::Char('Y')); // Confirm deletion with uppercase Y

    // Wait for delete operation to start
    assert!(matches!(app.delete_state, Some(DeleteState::Deleting(_))));

    // Get the progress handle and complete the operation
    if let Some(DeleteState::Deleting(progress)) = &app.delete_state {
        let total = progress.read().total;
        progress.write().scanned = total;

        // Trigger state update by pressing 'd'
        app.handle_key(KeyCode::Char('d'));

        // Verify operation completed
        assert!(app.delete_state.is_none());
        assert!(app.selected_items.is_empty());

        // Verify items were removed from the list
        assert_eq!(app.items.read().len(), 0);
    } else {
        panic!("Delete operation did not transition to Deleting state");
    }

    // Re-render UI after clean operation
    terminal
        .draw(|frame| {
            ui(frame, &mut app);
        })
        .unwrap();

    let final_buffer = terminal.backend().buffer().clone();
    let final_content = buffer_content_to_string(&final_buffer);

    // Verify items are no longer visible after clean operation
    assert!(!final_content.contains("/test/path1"));
    assert!(!final_content.contains("test-project-1"));
    assert!(!final_content.contains("/test/path2"));
    assert!(!final_content.contains("test-project-2"));

    // Verify clean operation completed
    assert!(app.delete_state.is_none());
    assert!(app.selected_items.is_empty());
}

fn buffer_content_to_string(buffer: &Buffer) -> String {
    buffer.content().iter().map(|cell| cell.symbol()).join("")
}
