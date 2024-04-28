use sqlx::{PgPool, Postgres};
use sqlx::postgres::PgPoolOptions;
use crate::ahess_error::AhessError;
use crate::ahess_result::AhessResult;
use crate::db_pool;

#[derive(Clone)]
pub struct Worker {
    pub sqlx: sqlx::Pool<Postgres>,
}


impl Worker {
    pub async fn new() -> AhessResult<Self> {
        let sqlx_pool = {
            let postgres_conn_url = format!(
                "postgres://{}:{}@{}/ahess",
                "postgres", "postgres", "localhost"
            );

            PgPool::connect(postgres_conn_url.as_str()).await.map_err(AhessError::ConnectedToSqlxPool)?
            // PgPoolOptions::new()
            //     .max_connections(5)
            //     .connect(postgres_conn_url.as_str())
            //     .await
            //     .map_err(AhessError::ConnectedToSqlxPool)?
        };

        // let sqlx = db_pool::make().await?;


        Ok(Worker { sqlx: sqlx_pool })
    }
}