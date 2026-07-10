mod action;
mod capture;
mod cli;
mod config;
mod fzf;
mod index;
mod motion;
mod preview;
mod search;
mod tmux;
mod types;
mod util;

fn main() {
    if let Err(err) = cli::run() {
        eprintln!("tmux-history-finder: {err:#}");
        std::process::exit(1);
    }
}
