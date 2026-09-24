pub mod application;
pub mod commands;
pub mod domain;
pub mod infrastructure;

use std::path::PathBuf;

use commands::build_app_state;
use infrastructure::settings_repository::SettingsPaths;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let config_directory = app.path().app_config_dir()?;
            app.manage(build_app_state(production_settings_paths(config_directory)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::activity::activity_list,
            commands::activity::activity_record_external,
            commands::activity::activity_execute,
            commands::activity::activity_rollback,
            commands::repository::repository_watch_snapshot,
            commands::repository::repository_watch_stop,
            commands::ai::ai_start_review,
            commands::ai::ai_chat,
            commands::ai::ai_review_skill_status,
            commands::ai::ai_start_commit_message,
            commands::ai::ai_start_conflict_suggestion,
            commands::ai::ai_cancel,
            commands::ai::ai_test_connection,
            commands::changes::changes_snapshot,
            commands::changes::changes_file_diff,
            commands::changes::changes_stage_file,
            commands::changes::changes_stage_files,
            commands::changes::changes_unstage_files,
            commands::changes::changes_unstage_file,
            commands::changes::changes_stage_hunk,
            commands::changes::changes_unstage_hunk,
            commands::changes::changes_stage_lines,
            commands::changes::changes_unstage_lines,
            commands::changes::changes_discard_file,
            commands::changes::changes_scan_noise,
            commands::changes::changes_line_stats,
            commands::changes::changes_restore_noise,
            commands::changes::changes_commit,
            commands::conflicts::conflicts_snapshot,
            commands::conflicts::conflicts_detail,
            commands::conflicts::conflicts_resolve,
            commands::conflicts::conflicts_continue,
            commands::console::console_start,
            commands::console::console_cancel,
            commands::terminal::terminal_start,
            commands::terminal::terminal_write,
            commands::terminal::terminal_resize,
            commands::terminal::terminal_terminate,
            commands::terminal::terminal_ack,
            commands::terminal::terminal_complete,
            commands::files::files_list,
            commands::files::files_open,
            commands::files::files_inspect,
            commands::files::files_ignore_preview,
            commands::files::files_ignore,
            commands::files::files_untrack,
            commands::files::files_lfs_track,
            commands::files::files_preview,
            commands::files::files_prepare,
            commands::files::files_execute,
            commands::history::history_page,
            commands::history::history_detail,
            commands::history::history_file_diff,
            commands::history::history_checkout,
            commands::history::history_reset,
            commands::history::history_cherry_pick,
            commands::history::history_revert,
            commands::history::history_squash_preview,
            commands::history::history_squash,
            commands::stash::stash_snapshot,
            commands::stash::stash_detail,
            commands::stash::stash_file_diff,
            commands::stash::stash_create,
            commands::stash::stash_apply,
            commands::stash::stash_pop,
            commands::refs::refs_snapshot,
            commands::task_branches::task_branches_snapshot,
            commands::task_branches::task_branches_unlink,
            commands::task_branches::task_branches_create,
            commands::task_branches::task_branches_run,
            commands::refs::refs_create_branch,
            commands::refs::refs_switch_branch,
            commands::refs::refs_delete_branch,
            commands::refs::refs_merge,
            commands::refs::refs_rebase,
            commands::refs::refs_abort,
            commands::remotes::remotes_snapshot,
            commands::remotes::remote_start_fetch,
            commands::remotes::remote_start_pull,
            commands::remotes::remote_start_push,
            commands::remotes::git_run_cancel,
            commands::repository::repository_open,
            commands::repository::repository_init,
            commands::repository::repository_clone,
            commands::repository::repository_refresh,
            commands::settings::settings_load,
            commands::settings::settings_fonts,
            commands::settings::settings_save,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build HQ Git")
        .run({
            use application::console_exit::{ConsoleExitGate, ExitAction};
            let gate = std::sync::Arc::new(ConsoleExitGate::default());
            move |app, event| {
                if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                    match gate.request() {
                        ExitAction::AllowExit => {}
                        ExitAction::PreventExit => api.prevent_exit(),
                        ExitAction::BeginCleanup => {
                            api.prevent_exit();
                            let console = app.state::<commands::AppState>().console.clone();
                            let terminal = app.state::<commands::AppState>().terminal.clone();
                            let app = app.clone();
                            let gate = gate.clone();
                            tauri::async_runtime::spawn(async move {
                                console.shutdown().await;
                                // Retry OS cleanup failures while the terminal retains
                                // its repository lock. Never exit claiming cleanup succeeded.
                                while let Err(error) = terminal.shutdown().await {
                                    eprintln!("terminal shutdown: {error}");
                                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                                }
                                gate.complete();
                                app.exit(code.unwrap_or(0));
                            });
                        }
                    }
                }
            }
        });
}

fn production_settings_paths(config_directory: PathBuf) -> SettingsPaths {
    let mut legacy_candidates = Vec::new();
    if let Some(app_data) = std::env::var_os("APPDATA") {
        legacy_candidates.push(
            PathBuf::from(app_data)
                .join("hq-git")
                .join("hq-git-settings.json"),
        );
    }
    if let Ok(executable) = std::env::current_exe()
        && let Some(directory) = executable.parent()
    {
        legacy_candidates.push(directory.join("userData").join("hq-git-settings.json"));
    }

    SettingsPaths {
        current: config_directory.join("settings.json"),
        migration_marker: config_directory.join("settings-migration-v1.complete"),
        legacy_candidates,
    }
}
