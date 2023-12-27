use actix_web::web::Path;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::PathBuf;

pub enum Error {
    CouldNotFindDbChangesDir,
    EntryHasNoExtension,
    EntryExtensionIsNotStr,
    EntryExtensionIsNotSql,
    FilenameIsNotStr,
    FilenameDoesNotExist,
    FilenameDoesNotHaveDelimiter,
    FilenamePrefixIsNotInt,
    CannotReadDbChangesDir(std::io::Error),
    CannotReadEntry(String),
    CouldNotWriteFile(std::io::Error),
}

pub fn run(change_name: String) -> Result<(), Error> {
    let db_changes_dir = std::path::Path::new("db/changes");

    if db_changes_dir.is_dir() {
        let mut max_num = 0;

        for entry in
            fs::read_dir(db_changes_dir).map_err(|err| Error::CannotReadDbChangesDir(err))?
        {
            let entry = entry.map_err(|err| Error::CannotReadEntry(err.to_string()))?;
            let path = entry.path();

            let extension = match path.extension() {
                None => Err(Error::EntryHasNoExtension)?,
                Some(e) => match e.to_str() {
                    None => Err(Error::EntryExtensionIsNotStr)?,
                    Some(s) => s,
                },
            };

            if extension == "sql" {
                let file_name = match path.file_name() {
                    None => Err(Error::FilenameDoesNotExist)?,
                    Some(f) => match f.to_str() {
                        None => Err(Error::FilenameIsNotStr)?,
                        Some(s) => s,
                    },
                };

                match file_name.split_once("__") {
                    None => Err(Error::FilenameDoesNotHaveDelimiter)?,
                    Some((prefix, _)) => {
                        let num = match prefix.parse::<u32>() {
                            Err(_) => Err(Error::FilenamePrefixIsNotInt)?,
                            Ok(n) => n,
                        };

                        max_num = std::cmp::max(max_num, num);
                    }
                }
            } else {
                Err(Error::EntryExtensionIsNotSql)?
            }
        }

        let db_change_file_name = format!("{}__{}.sql", max_num, change_name);

        let db_change_path = PathBuf::from("./db/changes/").join(db_change_file_name);

        fs::write(
            db_change_path,
            r#"-- Put your SQL here
            
            
            
            
        "#,
        )
        .map_err(|err| Error::CouldNotWriteFile(err))?;
    } else {
        Err(Error::CouldNotFindDbChangesDir)?
    }

    Ok(())
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Error::CouldNotFindDbChangesDir => "Could not find db changes dir".to_string(),
            Error::EntryHasNoExtension => "Entry has no extension".to_string(),
            Error::EntryExtensionIsNotStr => "Entry extension is not str".to_string(),
            Error::EntryExtensionIsNotSql => "Entry extension is not sql".to_string(),
            Error::FilenameIsNotStr => "Filename is not str".to_string(),
            Error::FilenameDoesNotExist => "Filename does not exist".to_string(),
            Error::FilenameDoesNotHaveDelimiter => "Filename does not have delimiter".to_string(),
            Error::FilenamePrefixIsNotInt => "Filename prefix is not int".to_string(),
            Error::CannotReadDbChangesDir(err) => {
                format!("Cannot read db changes dir, {}", err.to_string())
            }
            Error::CannotReadEntry(err) => format!("Cannot read entry, {}", err),
            Error::CouldNotWriteFile(err) => {
                format!("Could not write file, {}", err.to_string())
            }
        };

        write!(f, "{}", s)
    }
}
