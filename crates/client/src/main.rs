#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> Result<(), timetracker_client::ClientError> {
    timetracker_client::registration_ui::desktop::run()
}
