mod cpal_spike;
mod gpui_spike;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(author = "ct", version, about = "Ahess music composition spikes")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    #[command(name = "RunGpuiSpike")]
    RunGpuiSpike,

    #[command(name = "RunCpalSpike")]
    RunCpalSpike,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    match args.command {
        Command::RunGpuiSpike => gpui_spike::run(),
        Command::RunCpalSpike => cpal_spike::run(),
    }
}
