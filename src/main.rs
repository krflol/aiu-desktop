#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
mod app;
mod process;
mod protocol;
mod tray;
mod wake;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(version, about = "AIU native account frontend")]
struct Args {
    #[arg(long)]
    fixture: Option<PathBuf>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    app::run(args.fixture)
}
