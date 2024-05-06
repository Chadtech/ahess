use crate::change_db;
use std::fmt::{Debug, Display, Formatter};

pub enum AhessError {
    NewDbChangeError(change_db::Error),
    MigrateDbError(change_db::Error),
    FailedToLoadEnv(dotenv::Error),
    FailedToLoadEnvVar { var: String, error: dotenv::Error },
    ConnectedToSqlxPool(sqlx::Error),
    WebServerError(std::io::Error),
    IcedRunError(iced::Error),
    Db { tag: String, error: sqlx::Error },
    UnknownJob(String),
    TonePlayError { tag: String, error: rodio::PlayError },
    ToneStreamError { tag: String, error: rodio::StreamError },
}

impl AhessError {
    pub fn db(tag: &str, error: sqlx::Error) -> Self {
        AhessError::Db {
            tag: tag.to_string(),
            error,
        }
    }
}

impl Debug for AhessError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

impl Display for AhessError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            AhessError::NewDbChangeError(sub_err) => {
                format!("New Db Change Error, {}", sub_err.to_string())
            }
            AhessError::MigrateDbError(sub_err) => {
                format!("Migrate Db Error, {}", sub_err.to_string())
            }
            AhessError::FailedToLoadEnv(sub_err) => {
                format!("Failed to load env, {}", sub_err.to_string())
            }
            AhessError::FailedToLoadEnvVar { var, error } => format!(
                "Failed to load env var: {}, error: {}",
                var,
                error.to_string()
            ),
            AhessError::ConnectedToSqlxPool(error) => {
                format!("Connected to sqlx pool, error: {}", error.to_string())
            }
            AhessError::WebServerError(err) => {
                format!("Web server error, error: {}", err.to_string())
            }
            AhessError::IcedRunError(err) => {
                format!("Iced run error, error: {}", err.to_string())
            }
            AhessError::Db { tag, error } => {
                format!("Db error, tag: {}, error: {}", tag, error.to_string())
            }
            AhessError::UnknownJob(job_name) => {
                format!("Unknown job: {}", job_name)
            }
            AhessError::TonePlayError { tag, error } => {
                format!("Tone play error, tag: {}, error: {}", tag, error.to_string())
            }
            AhessError::ToneStreamError { tag, error } => {
                format!("Tone stream error, tag: {}, error: {}", tag, error.to_string())
            }
        };

        write!(f, "Ahess Error: {}", s)
    }
}

impl std::error::Error for AhessError {}
