mod ahess_error;
mod change_db;

use crate::ahess_error::AhessError;
use actix_web::{web, App, HttpRequest};
use clap::{Parser, Subcommand};
use sqlx::postgres::PgPoolOptions;
use std::env;

async fn index(req: HttpRequest) -> &'static str {
    println!("REQ: {req:?}");
    "Hello world!"
}

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
}

fn get_env_var(key_name: &str) -> Result<String, AhessError> {
    dotenv::var(key_name).map_err(|err| AhessError::FailedToLoadEnvVar {
        var: key_name.to_owned(),
        error: err,
    })
}

async fn make_db_pool() -> Result<sqlx::Pool<sqlx::Postgres>, AhessError> {
    let database_url = get_env_var("DATABASE_URL")?;
    let database_user = get_env_var("DATABASE_USER")?;
    let database_password = get_env_var("DATABASE_PASSWORD")?;
    let database_name = get_env_var("DATABASE_NAME")?;

    for var in env::vars() {
        println!("{:?}", var);
    }

    let sqlx_pool = {
        let postgres_conn_url = format!(
            "postgres://{}:{}@{}/{}",
            database_user, database_password, database_url, database_name
        );

        PgPoolOptions::new()
            .max_connections(5)
            .connect(postgres_conn_url.as_str())
            .await
            .map_err(AhessError::ConnectedToSqlxPool)?
    };

    Ok(sqlx_pool)
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
            let sqlx = make_db_pool().await?;

            change_db::migrate_db(sqlx)
                .await
                .map_err(|err| AhessError::MigrateDbError(err))?;
        }
        Command::Run => {
            println!("Running");
        }
    }

    // std::thread::spawn(move || loop {
    //     std::thread::sleep(std::time::Duration::from_secs(1));
    // });
    //
    // HttpServer::new(|| {
    //     App::new()
    //         .service(web::resource("/index.html").to(|| async { "Hello world!" }))
    //         .service(web::resource("/").to(index))
    // })
    // .bind(("127.0.0.1", 9841))?
    // .run()
    // .await

    Ok(())
}

#[cfg(test)]
mod tests {
    use actix_web::{body::to_bytes, dev::Service, http, test, web, App, Error};

    use super::*;

    #[actix_web::test]
    async fn test_index() -> Result<(), Error> {
        let app = App::new().route("/", web::get().to(index));
        let app = test::init_service(app).await;

        let req = test::TestRequest::get().uri("/").to_request();
        let resp = app.call(req).await?;

        assert_eq!(resp.status(), http::StatusCode::OK);

        let response_body = resp.into_body();
        assert_eq!(to_bytes(response_body).await?, r##"Hello world!"##);

        Ok(())
    }
}
