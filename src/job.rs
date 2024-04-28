use crate::ahess_error::AhessError;
use crate::ahess_result::AhessResult;
use crate::worker::Worker;

pub enum Job {
    Beep
}


pub async fn insert(worker: &Worker, job: Job) -> AhessResult<()> {
    let name = match job {
        Job::Beep => "beep",
    };
    let r = sqlx::query!("INSERT INTO job (name) VALUES ($1::TEXT)", name)
        .execute(&worker.sqlx)
        .await
        .map_err(|err| AhessError::db(
            "insert job",
            err,
        ))?;

    Ok(())
}