// This crate carried a blanket `#![allow(dead_code)]` through M2, when the
// safety core existed before any screen could reach it and the lib target
// produced 49 expected `dead_code` warnings. It was removed at M3, when the
// Clean screen wired `scan` and `remove` to real commands.
//
// Removing it immediately surfaced a real finding the noise had been hiding —
// a `Candidate` field written as a constant zero and read by nobody — which is
// precisely what the blanket allow was predicted to cost. **Do not reintroduce
// one.** The few items that genuinely have no caller yet carry their own
// narrowly scoped `#[allow(dead_code)]` naming the milestone that consumes
// them, so a warning here now means what it says.
mod analyze;
mod apps;
mod associate;
mod backups;
mod catalog;
mod commands;
mod escalate;
mod exclude;
mod health;
mod history;
mod lipo;
mod optimize;
mod orphans;
mod paths;
mod permissions;
mod proc;
mod receipts;
mod remove;
mod scan;
mod smoke;
mod startup;
mod volume;

// The updater plugin is registered at M7, not here. It reads
// plugins.updater.pubkey at init and panics without it, so it cannot be
// added before the signing key exists.
pub fn run() {
    // The smoke gate runs before any window exists and then exits. It is a
    // read-only pass over everything a release depends on being true of a
    // real Mac — see `smoke.rs`.
    if smoke::requested() {
        smoke::run();
        return;
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .invoke_handler(tauri::generate_handler![
            commands::clean_categories,
            commands::clean_scan,
            commands::clean_execute,
            commands::uninstall_list,
            commands::uninstall_inspect,
            commands::uninstall_execute,
            commands::leftovers_scan,
            commands::leftovers_remove,
            analyze::analyze_children,
            analyze::analyze_root,
            analyze::reveal_in_finder,
            backups::backups_list,
            backups::backups_remove,
            lipo::lipo_candidates,
            lipo::lipo_strip,
            exclude::exclusions_list,
            exclude::exclusions_add,
            exclude::exclusions_remove,
            history::history_read,
            history::history_clear,
            receipts::receipts_list,
            health::health_report,
            optimize::optimize_plan,
            optimize::optimize_execute,
            startup::startup_list,
            startup::startup_set_enabled,
            startup::startup_remove,
            startup::open_login_items_settings,
            permissions::fda_status,
            permissions::open_privacy_settings
        ])
        .run(tauri::generate_context!())
        .expect("error while running Spiral Clean");
}
