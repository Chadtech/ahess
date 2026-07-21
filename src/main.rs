pub mod acoustics;
mod app;
pub mod convolution;
mod cpal_spike;
mod gpui_spike;
mod palette;
pub mod part;
pub mod pitch_system;
mod playback;
pub mod project;
pub mod seed;
mod style;
pub mod tuning_system;
mod view;
pub mod voice;
pub mod voice_name;

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
