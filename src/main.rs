//! Glimmer entry point.
//!
//! The command line is parsed *before* the window opens, so `--help`,
//! `--version` and argument errors never flash an empty window.

// Release builds on Windows shouldn't pop up a console window next to the game.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use glimmer::app::{self, AppArgs, Command};
use macroquad::prelude::*;

fn window_conf() -> Conf {
    Conf {
        window_title: "Glimmer".to_owned(),
        window_width: 594,
        window_height: 792,
        high_dpi: true,
        sample_count: 4,
        window_resizable: true,
        icon: Some(glimmer::icon::window_icon()),
        ..Default::default()
    }
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    let args = match AppArgs::parse_command(&argv) {
        Ok(Command::Run(a)) => a,
        Ok(Command::Help) => {
            println!("{}", app::usage());
            return;
        }
        Ok(Command::Version) => {
            println!("glimmer {}", app::VERSION);
            return;
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    macroquad::Window::from_config(window_conf(), app::run(args));
}
