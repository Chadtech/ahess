use std;
use crate::ahess_error::AhessError;
use crate::ahess_result::AhessResult;
use crate::worker::Worker;


use sqlx;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use uuid;
use uuid::Timestamp;

pub enum Job {
    Beep
}


pub async fn check(s: &str) -> AhessResult<()> {
    let worker = Worker::new().await?;

    loop {
        println!("{}", s);
        let r = sqlx::query!(
            r"SELECT * FROM job").fetch_all(&worker.sqlx).await.map_err(|err| AhessError::db(
            "select job",
            err,
        ))?;

        for row in r {
            println!("{:?}", row);
        }

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

pub async fn insert(worker: &Worker, job: Job) -> AhessResult<()> {
    let name = match job {
        Job::Beep => "beep",
    };


    let uuid = uuid::Uuid::now_v7();

    let r = sqlx::query!(
        r#"INSERT INTO job (uuid, name) VALUES ($1::UUID, $2::TEXT)
        "#,
        uuid,
        name
    )
        .fetch_all(&worker.sqlx)
        .await.map_err(|err| AhessError::db(
        "select job",
        err,
    ))?;

    Ok(())
}