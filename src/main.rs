mod ahess_error;
mod change_db;
mod db_pool;
mod generate_test;
mod web_server;

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
    Run,
    GenerateTest,
}

#[actix_web::main]
async fn main() -> Result<(), AhessError> {
    dotenv::dotenv().map_err(AhessError::FailedToLoadEnv)?;

    let args = Args::parse();

    let command = args.command.unwrap_or(Command::Run);

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
        Command::Run => {
            web_server::run().await?;
        }
        Command::GenerateTest => {
            generate_test::run()?;
        }
    }

    Ok(())
}
