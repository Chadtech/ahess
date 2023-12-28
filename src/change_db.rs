use std::fmt::{Display, Formatter};
use std::fs;
use std::path::PathBuf;

#[derive(Debug)]
pub enum Error {
    CouldNotFindDbChangesDir,
    CannotReadEntry(String),
    EntryHasNoExtension,
    EntryExtensionIsNotStr,
    FilenameIsNotStr,
    FilenameDoesNotExist,
    CannotReadDbChangesDir(std::io::Error),
    FilenameDoesNotHaveDelimiter,
    FilenamePrefixIsNotInt,
    EntryExtensionIsNotSql,
    CannotReadFile(String),
    InconsistentNumbering,
    CouldNotWriteFile(std::io::Error),
    CannotReadMigrationScript(String),
    CouldNotWriteMigrationScript(std::io::Error),
}

pub fn migrate_db() -> Result<(), Error> {
    //--------
    sqlx::query_file!("./db/changes/1__jobs-table.sql", &[]);
    sqlx::query_file!("./db/changes/2__test.sql", &[]);
    sqlx::query_file!("./db/changes/3__test-2.sql", &[]);
    sqlx::query_file!("./db/changes/4__test-2.sql", &[]);
    sqlx::query_file!("./db/changes/5__test-2.sql", &[]);
    sqlx::query_file!("./db/changes/6__test-2.sql", &[]);
    sqlx::query_file!("./db/changes/7__test-2.sql", &[]);
    sqlx::query_file!("./db/changes/8__test-2.sql", &[]);

//--------


    Ok(())
}

pub fn new_change(change_name: String) -> Result<(), Error> {
    let mut changes = get_changes()?;

    let db_change_file_name = format!("{}__{}.sql", changes.len() + 1, change_name);

    let db_change_path = PathBuf::from("./db/changes/").join(db_change_file_name.clone());

    fs::write(
        db_change_path,
        r#"-- Put your SQL here
            
            
            
            
        "#,
    )
    .map_err(|err| Error::CouldNotWriteFile(err))?;

    changes.push(db_change_file_name);

    // Modify migration script

    let migration_script_path = PathBuf::from("./src/change_db.rs");

    let migration_script = fs::read_to_string(migration_script_path.clone())
        .map_err(|err| Error::CannotReadMigrationScript(err.to_string()))?;

    let (beginning, remainder) = match migration_script.split_once("//--------") {
        None => Err(Error::CannotReadMigrationScript(
            "Could not find first '//--------' delimiter in migration script".to_string(),
        ))?,
        Some((b, r)) => (b, r),
    };

    let end = match remainder.split_once("//--------") {
        None => Err(Error::CannotReadMigrationScript(
            "Could not find second '//--------' delimiter in migration script".to_string(),
        ))?,
        Some((_, e)) => e,
    };

    let mut middle = String::new();

    for change in changes {
        middle.push_str(&format!(
            "    sqlx::query_file!(\"./db/changes/{}\", &[]);\n",
            change
        ));
    }

    let new_migration_script = format!("{}//--------\n{}\n//--------\n{}", beginning, middle, end);

    fs::write(migration_script_path, new_migration_script)
        .map_err(|err| Error::CouldNotWriteMigrationScript(err))?;

    Ok(())
}

fn get_changes() -> Result<Vec<String>, Error> {
    let db_changes_dir = std::path::Path::new("db/changes");

    if db_changes_dir.is_dir() {
        let mut max_num = 0;

        let mut changes: Vec<(u32, String)> = vec![];

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

                        changes.push((num, file_name.to_string()));
                    }
                }
            } else {
                Err(Error::EntryExtensionIsNotSql)?
            }
        }
        changes.sort_by(|a, b| a.0.cmp(&b.0));

        let highest_num = changes.last().map(|c| c.0);

        if Some(changes.len() as u32) == highest_num && Some(max_num) == highest_num {
            Ok(changes
                .into_iter()
                .map(|(_, file_name)| file_name)
                .collect::<Vec<String>>())
        } else {
            Err(Error::InconsistentNumbering)
        }
    } else {
        Err(Error::CouldNotFindDbChangesDir)
    }
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
            Error::CannotReadFile(err) => {
                format!("Cannot read file, {}", err)
            }
            Error::InconsistentNumbering => "Inconsistent numbering".to_string(),
            Error::CouldNotWriteFile(err) => {
                format!("Could not write file, {}", err.to_string())
            }
            Error::CannotReadMigrationScript(err) => {
                format!("Cannot read migration script, {}", err.to_string())
            }
            Error::CouldNotWriteMigrationScript(err) => {
                format!("Could not write migration script, {}", err.to_string())
            }
        };

        write!(f, "{}", s)
    }
}

#[cfg(test)]
mod change_db_tests {
    use crate::change_db::{get_changes, Error};

    #[test]
    fn can_get_changes() -> Result<(), Error> {
        let changes = get_changes()?;

        Ok(())
    }
}
