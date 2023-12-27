use crate::new_db_change;
use std::fmt::{Debug, Display, Formatter};

pub enum AhessError {
    NewDbChangeError(new_db_change::Error),
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
        };

        write!(f, "Ahess Error: {}", s)
    }
}

impl std::error::Error for AhessError {}
