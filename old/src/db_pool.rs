use crate::ahess_error::AhessError;
use sqlx::postgres::PgPoolOptions;
use std::env;

pub async fn make() -> Result<sqlx::Pool<sqlx::Postgres>, AhessError> {
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

fn get_env_var(key_name: &str) -> Result<String, AhessError> {
    dotenv::var(key_name).map_err(|err| AhessError::FailedToLoadEnvVar {
        var: key_name.to_owned(),
        error: err,
    })
}
