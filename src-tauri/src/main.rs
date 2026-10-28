// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Avviato da Claude come server MCP: niente finestra.
    if let Some(code) = postamon_lib::run_mcp() {
        std::process::exit(code);
    }
    postamon_lib::run()
}
