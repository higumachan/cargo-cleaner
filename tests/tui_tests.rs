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
        items.push(ProjectTargetAnalysis {
            project_path: std::path::PathBuf::from("/test/path"),
            project_name: Some("test-project".to_string()),
            size: 1024 * 1024 * 1024, // 1GB
            selected_for_cleanup: true,
            last_modified: SystemTime::now(),
            id: Uuid::new_v4(),
        });
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

    assert!(content_str.contains("Cargo Cleaner"));
    // TODO(devin): 上のケースを参考にして書き直す
    // assert!(buffer
    //     .content
    //     .iter()
    //     .any(|cell| cell.symbol().contains("Cargo Cleaner")));
    // assert!(buffer
    //     .content
    //     .iter()
    //     .any(|cell| cell.symbol().contains("(dry-run)")));
    //
    // // Verify table headers are present
    // assert!(buffer
    //     .content
    //     .iter()
    //     .any(|cell| cell.symbol().contains("Path")));
    // assert!(buffer
    //     .content
    //     .iter()
    //     .any(|cell| cell.symbol().contains("Last Modified")));
    // assert!(buffer
    //     .content
    //     .iter()
    //     .any(|cell| cell.symbol().contains("Size")));
    //
    // // Verify mock data is displayed
    // assert!(buffer
    //     .content
    //     .iter()
    //     .any(|cell| cell.symbol().contains("/test/path")));
    // assert!(buffer
    //     .content
    //     .iter()
    //     .any(|cell| cell.symbol().contains("1.00 MiB")));
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
            items.push(ProjectTargetAnalysis {
                project_path: std::path::PathBuf::from(format!("/test/path{}", i)),
                project_name: Some(format!("test-project-{}", i)),
                size: 1024 * 1024 * 1024, // 1GB
                selected_for_cleanup: true,
                last_modified: SystemTime::now(),
                id: Uuid::new_v4(),
            });
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

    // TODO(@devin): 上のケースを参考にして書き直す
    // // Test previous() navigation
    // app.previous();
    // assert_eq!(app.table_state.selected(), Some(2));
    // app.previous();
    // assert_eq!(app.table_state.selected(), Some(1));
    //
    // // Test after_move behavior in different modes
    // app.mode = CursorMode::Select;
    // after_move(&mut app);
    // {
    //     let items = app.items.read();
    //     assert!(app.selected_items.contains(&items[1].id));
    // }
    //
    // app.mode = CursorMode::Unselect;
    // app.next();
    // after_move(&mut app);
    // {
    //     let items = app.items.read();
    //     assert!(!app.selected_items.contains(&items[2].id));
    // }
}

// TODO(@devin): 上のケースを参考にして書き直す

/// Test cursor mode transitions and effects
// #[test]
// fn test_cursor_mode_transitions() {
//     let (tx, _rx) = sync_channel(1);
//     let scan_progress = Arc::new(NotifyRwLock::new(
//         tx.clone(),
//         Progress {
//             total: 0,
//             scanned: 0,
//         },
//     ));
//     let mut app = App::new(true, tx, scan_progress);
//
//     // Add a mock item
//     let item_id = {
//         let mut items = app.items.write();
//         let item = ProjectTargetAnalysis {
//             project_path: std::path::PathBuf::from("/test/path"),
//             project_name: Some("test-project".to_string()),
//             size: 1024 * 1024 * 1024, // 1GB
//             selected_for_cleanup: true,
//             last_modified: SystemTime::now(),
//             id: Uuid::new_v4(),
//         };
//         let id = item.id;
//         items.push(item);
//         id
//     };
//
//     // Test initial state
//     assert!(matches!(app.mode, CursorMode::Normal));
//     assert!(!app.selected_items.contains(&item_id));
//
//     // Test Select mode behavior
//     app.mode = CursorMode::Select;
//     after_move(&mut app);
//     assert!(app.selected_items.contains(&item_id));
//
//     // Test Unselect mode behavior
//     app.mode = CursorMode::Unselect;
//     after_move(&mut app);
//     assert!(!app.selected_items.contains(&item_id));
//
//     // Test Normal mode (shouldn't affect selection)
//     app.mode = CursorMode::Normal;
//     app.selected_items.insert(item_id);
//     after_move(&mut app);
//     assert!(app.selected_items.contains(&item_id));
// }

/// Test help popup visibility and content
// #[test]
// fn test_help_popup_toggle() {
//     let backend = TestBackend::new(100, 30);
//     let mut terminal = Terminal::new(backend).unwrap();
//     let (tx, _rx) = sync_channel(1);
//     let scan_progress = Arc::new(NotifyRwLock::new(
//         tx.clone(),
//         Progress {
//             total: 0,
//             scanned: 0,
//         },
//     ));
//     let mut app = App::new(true, tx, scan_progress);
//
//     // Initially help popup should be hidden
//     assert!(!app.show_help_popup);
//     terminal
//         .draw(|frame| {
//             ui(frame, &mut app);
//         })
//         .unwrap();
//     let buffer = terminal.backend().buffer().clone();
//     assert!(!buffer
//         .content
//         .iter()
//         .any(|cell| cell.symbol().contains("Help")));
//
//     // Toggle help popup on
//     app.show_help_popup = true;
//     terminal
//         .draw(|frame| {
//             ui(frame, &mut app);
//         })
//         .unwrap();
//     let buffer = terminal.backend().buffer().clone();
//     // Verify help content is visible
//     assert!(buffer
//         .content
//         .iter()
//         .any(|cell| cell.symbol().contains("Help")));
//     assert!(buffer
//         .content
//         .iter()
//         .any(|cell| cell.symbol().contains("h      : toggle help")));
//     assert!(buffer
//         .content
//         .iter()
//         .any(|cell| cell.symbol().contains("space  : toggle select")));
//
//     // Toggle help popup off
//     app.show_help_popup = false;
//     terminal
//         .draw(|frame| {
//             ui(frame, &mut app);
//         })
//         .unwrap();
//     let buffer = terminal.backend().buffer().clone();
//     assert!(!buffer
//         .content
//         .iter()
//         .any(|cell| cell.symbol().contains("Help")));
// }
//
// /// Test delete popup rendering and state transitions
// #[test]
// fn test_delete_popup() {
//     let backend = TestBackend::new(100, 30);
//     let mut terminal = Terminal::new(backend).unwrap();
//     let (tx, _rx) = sync_channel(1);
//     let scan_progress = Arc::new(NotifyRwLock::new(
//         tx.clone(),
//         Progress {
//             total: 0,
//             scanned: 0,
//         },
//     ));
//     let mut app = App::new(true, tx, scan_progress);
//
//     // Initially no delete popup
//     assert!(app.delete_state.is_none());
//     terminal
//         .draw(|frame| {
//             ui(frame, &mut app);
//         })
//         .unwrap();
//     let buffer = terminal.backend().buffer().clone();
//     assert!(!buffer
//         .content
//         .iter()
//         .any(|cell| cell.symbol().contains("Are you sure")));
//
//     // Add some items to delete
//     let item_id = Uuid::new_v4();
//     app.selected_items.insert(item_id);
//
//     // Show delete confirmation
//     app.delete_state = Some(DeleteState::Confirm);
//     terminal
//         .draw(|frame| {
//             ui(frame, &mut app);
//         })
//         .unwrap();
//     let buffer = terminal.backend().buffer().clone();
//     assert!(buffer
//         .content
//         .iter()
//         .any(|cell| cell.symbol().contains("Are you sure you want to delete")));
// }

// /// Test status bar rendering
// #[test]
// fn test_status_bar() {
//     let backend = TestBackend::new(100, 30);
//     let mut terminal = Terminal::new(backend).unwrap();
//     let (tx, _rx) = sync_channel(1);
//     let scan_progress = Arc::new(NotifyRwLock::new(
//         tx.clone(),
//         Progress {
//             total: 0,
//             scanned: 0,
//         },
//     ));
//     let mut app = App::new(true, tx, scan_progress);
//
//     // Test initial status
//     terminal
//         .draw(|frame| {
//             ui(frame, &mut app);
//         })
//         .unwrap();
//     let buffer = terminal.backend().buffer().clone();
//     assert!(buffer
//         .content
//         .iter()
//         .any(|cell| cell.symbol().contains("Total:")));
//     assert!(buffer
//         .content
//         .iter()
//         .any(|cell| cell.symbol().contains("Selected:")));
//     assert!(buffer
//         .content
//         .iter()
//         .any(|cell| cell.symbol().contains("h: help")));
//
//     // Test mode display
//     app.mode = CursorMode::Select;
//     terminal
//         .draw(|frame| {
//             ui(frame, &mut app);
//         })
//         .unwrap();
//     let buffer = terminal.backend().buffer().clone();
//     assert!(buffer
//         .content
//         .iter()
//         .any(|cell| cell.symbol().contains("Select")));
//
//     app.mode = CursorMode::Unselect;
//     terminal
//         .draw(|frame| {
//             ui(frame, &mut app);
//         })
//         .unwrap();
//     let buffer = terminal.backend().buffer().clone();
//     assert!(buffer
//         .content
//         .iter()
//         .any(|cell| cell.symbol().contains("Unselect")));
// }

fn buffer_content_to_string(buffer: &Buffer) -> String {
    buffer.content().iter().map(|cell| cell.symbol()).join("")
}
