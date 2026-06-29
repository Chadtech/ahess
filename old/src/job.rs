use std;
use crate::ahess_error::AhessError;
use crate::ahess_result::AhessResult;
use crate::worker::Worker;


use sqlx;
use uuid;
use crate::tone::Tone;

pub enum Job {
    Beep
}


const BEEP: &str = "beep";


pub async fn check() -> AhessResult<()> {
    let worker = Worker::new().await?;

    loop {
        let r = sqlx::query!(
            r#"
                SELECT * FROM job
                WHERE finished_at IS NULL
                ORDER BY created_at ASC;
            "#).fetch_all(&worker.sqlx).await.map_err(|err| AhessError::db(
            "select job",
            err,
        ))?;

        println!("Jobs: {:?}", r.len());
        for row in r {
            let name = row.name;

            println!("Job name: {}", name);

            let job = match name.as_str() {
                BEEP => {
                    Job::Beep
                }
                _ => {
                    Err(AhessError::UnknownJob(name))?
                }
            };


            match job {
                Job::Beep => {
                    Tone::new(800.0).run("beep job")?;
                }
            }

            let job_uuid = row.uuid;

            sqlx::query!(
                r#"
                    UPDATE job
                    SET finished_at = NOW()
                    WHERE uuid = $1::UUID;
                "#,
                job_uuid
            ).execute(&worker.sqlx).await.map_err(|err| AhessError::db(
                "update finished job",
                err,
            ))?;
        }

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

pub async fn insert(worker: &Worker, job: Job) -> AhessResult<()> {
    let name = match job {
        Job::Beep => BEEP,
    };

    let uuid = uuid::Uuid::now_v7();

    sqlx::query!(
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