pub mod acoustics;
mod app;
mod audio_build;
pub mod convolution;
mod cpal_spike;
mod gamelan_metallophone;
mod gpui_spike;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
mod mts_esp;
mod noitech_bell_a;
mod noitech_bell_b;
mod palette;
pub mod part;
pub mod pitch_system;
mod playback;
pub mod project;
mod recovered_voice;
pub mod seed;
mod style;
#[cfg(target_os = "macos")]
mod surge_xt;
pub mod tuning_system;
mod view;
pub mod voice;
pub mod voice_name;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(author = "ct", version, about = "Ahess music composition spikes")]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Eq, PartialEq, Subcommand)]
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
        None | Some(Command::Ui) => app::run(),
        Some(Command::RunGpuiSpike) => gpui_spike::run(),
        Some(Command::RunCpalSpike) => cpal_spike::run(),
    }
}

#[cfg(test)]
mod tests {
    use super::{Args, Command};
    use clap::Parser;

    #[test]
    fn launches_ui_when_no_command_is_given() {
        let args = Args::try_parse_from(["ahess"]).unwrap();

        assert_eq!(args.command, None);
    }

    #[test]
    fn accepts_explicit_ui_command() {
        let args = Args::try_parse_from(["ahess", "ui"]).unwrap();

        assert_eq!(args.command, Some(Command::Ui));
    }
}
