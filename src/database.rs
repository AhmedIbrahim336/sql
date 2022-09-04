// use crate::{utils::get_db_path, DB_DIR};
use std::{fs, io, path::Path};
use thiserror::Error;

use crate::utils::get_db_path;

pub const DB_DIR: &str = "./sql";
pub const CURR_DB: &str = "curr_db";

#[derive(Error, Debug)]
pub enum DatabaseError {
    #[error("Duplicated database")]
    DuplicatedDB(String),
    #[error("IO Error")]
    IoError(#[from] io::Error),
    #[error("Database not found")]
    NotFound(String),
}

type DBResult<T> = Result<T, DatabaseError>;

pub struct Database;
impl Database {
    pub fn new(name: &str) -> DBResult<()> {
        let base_dir = Path::new(DB_DIR);
        let db_dir = base_dir.join(name);
        if db_dir.exists() {
            return Err(DatabaseError::DuplicatedDB(name.to_string()));
        }

        fs::create_dir_all(db_dir)?;
        Ok(())
    }

    pub fn drop(name: &str) -> DBResult<()> {
        let base_dir = Path::new(DB_DIR);
        let db_dir = base_dir.join(name);

        if !db_dir.exists() {
            return Err(DatabaseError::NotFound(name.to_string()));
        }
        fs::remove_dir(db_dir)?;
        Ok(())
    }

    pub fn use_db(name: &str) -> DBResult<()> {
        let base_dir = Path::new(DB_DIR);
        Database::exists_or_err(name)?;
        let curr_db = base_dir.join(CURR_DB);
        fs::write(curr_db, name)?;
        Ok(())
    }

    pub fn get_curr_db() -> DBResult<String> {
        let base_dir = Path::new(DB_DIR);
        let curr_db = base_dir.join(CURR_DB);
        let db = fs::read_to_string(curr_db)?;
        Database::exists_or_err(&db)?;
        Ok(db)
    }

    pub fn get_dbs() -> DBResult<Vec<String>> {
        Ok(vec![])
    }

    pub fn exists(name: &str) -> bool {
        let path = get_db_path(name);
        path.exists()
    }

    pub fn exists_or_err(name: &str) -> DBResult<()> {
        if !Database::exists(name) {
            Err(DatabaseError::NotFound(name.to_string()))
        } else {
            Ok(())
        }
    }
}
