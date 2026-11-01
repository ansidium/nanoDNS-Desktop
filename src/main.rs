#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod background;
mod config;
mod dns;
mod fonts;
mod i18n;
mod instance;
mod platform;
mod ui;

fn main() -> std::process::ExitCode {
    match ui::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            if std::env::args_os().any(|arg| arg == "--headless") {
                eprintln!(
                    "{}",
                    i18n::message(&format!("Не удалось запустить nanoDNS: {error}"))
                );
            } else {
                platform::error_dialog(&i18n::message(&format!(
                    "Не удалось запустить nanoDNS.\n\n{error}"
                )));
            }
            std::process::ExitCode::FAILURE
        }
    }
}
