use sqlx::{PgPool, Postgres};
use crate::ahess_error::AhessError;
use crate::ahess_result::AhessResult;

#[derive(Clone)]
pub struct Worker {
    pub sqlx: sqlx::Pool<Postgres>,
}


impl Worker {
    pub async fn new() -> AhessResult<Self> {
        let sqlx_pool =
            PgPool::connect("postgres://postgres:postgres@127.0.0.1:5432/ahess")
                .await
                .map_err(AhessError::ConnectedToSqlxPool)?;

        Ok(Worker { sqlx: sqlx_pool })
    }
}