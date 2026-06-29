mod ahess_error;
mod change_db;
mod db_pool;
mod generate_test;
mod run_ui;
mod style;
mod job;
mod ahess_result;
mod worker;
mod tone;

use crate::ahess_error::AhessError;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[clap(author = "ct", version = "0.1", about = "Audio Generation")]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    NewDbChange { change_name: String },
    MigrateDb,
    GenerateTest,
    RunUi,
}

#[tokio::main]
async fn main() -> Result<(), AhessError> {
    dotenv::dotenv().map_err(AhessError::FailedToLoadEnv)?;

    let args = Args::parse();

    let command = args.command.unwrap_or(Command::RunUi);

    match command {
        Command::NewDbChange { change_name } => {
            change_db::new_change(change_name).map_err(|err| AhessError::NewDbChangeError(err))?;
        }
        Command::MigrateDb => {
            let sqlx = db_pool::make().await?;

            change_db::migrate_db(sqlx)
                .await
                .map_err(|err| AhessError::MigrateDbError(err))?;
        }
        Command::GenerateTest => {
            generate_test::run()?;
        }
        Command::RunUi => {
            let job_checker = tokio::spawn(job::check());

            let (run_ui_result, join_result) = tokio::join!(run_ui::run(), job_checker);

            run_ui_result?;

            match join_result {
                Ok(job_check_result) => {
                    job_check_result?;
                }
                Err(err) => {
                    Err(AhessError::JoinError(err))?;
                }
            }
        }
    }

    Ok(())
}
