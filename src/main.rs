mod app;
mod cpal_spike;
mod gpui_spike;
mod new_project;
mod palette;
pub mod project;
pub mod seed;
mod style;
mod view;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(author = "ct", version, about = "Ahess music composition spikes")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    #[command(name = "ui")]
    Ui,

    #[command(name = "RunGpuiSpike")]
    RunGpuiSpike,

    #[command(name = "RunCpalSpike")]
    RunCpalSpike,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    match args.command {
        Command::Ui => app::run(),
        Command::RunGpuiSpike => gpui_spike::run(),
        Command::RunCpalSpike => cpal_spike::run(),
    }
}
