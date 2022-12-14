use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::HashMap, fs, io};
use thiserror::Error;

use crate::{
    database::{Database, DatabaseError},
    query_parser::{Condition, Operator, SelectCols},
    types::{DataType, DataTypesErr},
    utils::{get_db_path, get_schema_path, get_table_path},
};

pub type TableEntries = Vec<HashMap<String, String>>;

pub struct Table<'a> {
    pub db: &'a str,
    pub table_name: &'a str,
}

#[derive(Debug, Error)]
pub enum TableError {
    #[error("DB Error")]
    DBErr(#[from] DatabaseError),
    #[error("IO Error")]
    IoErr(#[from] io::Error),
    #[error("Invalid JSON")]
    SerializationErr(#[from] serde_json::Error),
    #[error("Table not found")]
    TableNotFond(String),
    #[error("Column not found")]
    ColNotFound(String),
    #[error("Column type  not found")]
    ColTypeNotFound(String),
    #[error("Number of columns doesn't match number of vlaues")]
    NumberMismatch(String),
    #[error("Types error")]
    TypeErr(#[from] DataTypesErr),
    #[error("Column already exist")]
    ColAlreadyExist(String),
}

type TableResult<T> = Result<T, TableError>;

impl<'a> Table<'a> {
    pub fn new(db: &'a str, table_name: &'a str) -> TableResult<Self> {
        Database::exists_or_err(db)?;
        Ok(Self { db, table_name })
    }

    pub fn create(&self, cols: Vec<String>, types: Vec<DataType>) -> TableResult<()> {
        let schema = json!({ "cols": cols, "types": types });
        let schema = serde_json::to_string_pretty(&schema)?;

        Database::exists_or_err(self.db)?;

        let db_path = get_db_path(self.db);
        let schema_file = db_path.join(format!("{}.schema.json", self.table_name));
        let table_file = db_path.join(format!("{}.json", self.table_name));
        fs::write(schema_file, schema.as_bytes())?;
        fs::write(table_file, "[]")?;
        Ok(())
    }

    pub fn insert(&self, cols: SelectCols, values: Vec<Vec<String>>) -> TableResult<()> {
        Database::exists_or_err(self.db)?;

        let schema = self.read_schema()?;
        let cols = match cols {
            SelectCols::Cols(cols) => cols,
            SelectCols::All => schema.cols.clone(),
        };

        let mut col_type_map = HashMap::new();
        for col in &cols {
            let col_pos = match schema.cols.iter().position(|c| c == col) {
                Some(pos) => pos,
                None => return Err(TableError::ColNotFound(col.to_string())),
            };

            let dtype = match schema.types.get(col_pos) {
                Some(dtype) => dtype,
                None => return Err(TableError::ColTypeNotFound(col.to_string())),
            };

            col_type_map.insert(col, dtype);
        }

        let mut new_entries = Vec::new();
        for (idx, row) in values.iter().enumerate() {
            if row.len() != cols.len() {
                return Err(TableError::NumberMismatch(format!(
                    "[row = {}][{:?}] validation expr {} != {}",
                    idx,
                    row,
                    row.len(),
                    cols.len()
                )));
            }

            let mut map = HashMap::new();
            for (col, val) in cols.iter().zip(row) {
                col_type_map[col].is_valid(val)?;
                map.insert(col.clone(), val.clone());
            }

            new_entries.push(map);
        }

        let mut all_entries = self.read()?;
        all_entries.extend(new_entries);
        println!(
            "[{}@{}] {:?} entries",
            self.table_name,
            self.db,
            all_entries.len()
        );
        self.write(&all_entries)?;
        Ok(())
    }

    pub fn select(
        &self,
        cols: SelectCols,
        condition: Option<Condition>,
    ) -> TableResult<TableEntries> {
        let all_entries = self.read()?;

        let entries = all_entries
            .into_iter()
            .filter(|e| Table::match_query(&condition, e))
            .map(|entry| match &cols {
                SelectCols::All => entry,
                SelectCols::Cols(selectd_cols) => {
                    let mut map = HashMap::new();
                    selectd_cols.into_iter().for_each(|col| {
                        map.insert(col.clone(), entry.get(col.trim()).unwrap().clone());
                    });
                    map
                }
            })
            .collect::<Vec<HashMap<_, _>>>();

        Ok(entries)
    }

    pub fn delete(&self, condition: Condition) -> TableResult<()> {
        let all_entries = self.read()?;
        let condition = Some(condition);

        let entries = all_entries
            .into_iter()
            .filter(|e| !Table::match_query(&condition, e))
            .collect::<Vec<HashMap<_, _>>>();

        self.write(&entries)?;
        Ok(())
    }

    pub fn alter(&self, col_name: &str, datatype: DataType) -> TableResult<()> {
        // Todo: Update the actual table
        // Update schema
        self.exists_or_err()?;
        let mut schema = self.read_schema()?;
        let p = schema.cols.iter().position(|c| c == &col_name.to_string());

        match p {
            None => Err(TableError::ColNotFound(col_name.into())),
            Some(pos) => match schema.types.get(pos) {
                None => Err(TableError::ColTypeNotFound(col_name.into())),
                Some(_) => {
                    schema.types[pos] = datatype;
                    self.write_schema(schema)?;

                    Ok(())
                }
            },
        }
    }

    pub fn drop(&self) -> TableResult<()> {
        self.exists_or_err()?;

        let schema = get_schema_path(self);
        let table = get_table_path(self);

        fs::remove_file(schema)?;
        fs::remove_file(table)?;

        Ok(())
    }

    pub fn truncate(&self) -> Result<(), TableError> {
        self.write(&vec![])?;
        Ok(())
    }

    pub fn add_col(&self, col_name: &str, datatype: DataType) -> TableResult<()> {
        // todo: Every column should be unique
        // TODO: Add the new column to the data with the default value of this type
        let mut schema = self.read_schema()?;

        if self.col_exist(&schema, col_name) {
            Err(TableError::ColAlreadyExist(col_name.into()))
        } else if schema.cols.len() != schema.types.len() {
            Err(TableError::NumberMismatch(format!(
                "cols = {}, types = {}",
                schema.cols.len(),
                schema.types.len()
            )))
        } else {
            schema.cols.push(col_name.into());
            schema.types.push(datatype.clone());

            let all_entries = self.read()?;
            let new_entries = all_entries
                .into_iter()
                .map(|mut entry| {
                    entry.insert(col_name.into(), datatype.default());

                    entry
                })
                .collect::<Vec<HashMap<_, _>>>();

            self.write(&new_entries)?;
            self.write_schema(schema)?;
            Ok(())
        }
    }

    pub fn remove_col<T: Into<String> + Copy>(&self, col_name: T) -> TableResult<()> {
        // Todo: Col should be removed from the table
        let mut schema = self.read_schema()?;
        let pos = schema.cols.iter().position(|c| c == &col_name.into());

        match pos {
            Some(pos) => {
                schema.cols.remove(pos);
                schema.types.remove(pos);

                debug_assert_eq!(schema.cols.len(), schema.types.len());
                self.write_schema(schema)?;
                Ok(())
            }
            None => Err(TableError::ColNotFound(col_name.into())),
        }
    }

    fn read(&self) -> Result<TableEntries, TableError> {
        self.exists_or_err()?;
        let table = get_table_path(self);

        let content = fs::read_to_string(table)?;

        Ok(serde_json::from_str(&content)?)
    }

    fn write(&self, entries: &TableEntries) -> TableResult<()> {
        self.exists_or_err()?;
        let table = get_table_path(self);
        let entries = json!(entries);
        fs::write(table, entries.to_string())?;
        Ok(())
    }

    fn read_schema(&self) -> TableResult<Schema> {
        self.exists_or_err()?;
        let schema = get_schema_path(self);

        let content = fs::read_to_string(schema)?;

        Ok(serde_json::from_str(&content)?)
    }

    fn write_schema(&self, schema: Schema) -> TableResult<()> {
        self.exists_or_err()?;
        let path = get_schema_path(self);
        let schema = json!(schema);
        fs::write(path, serde_json::to_string_pretty(&schema)?)?;
        Ok(())
    }

    fn exist(&self) -> bool {
        let schema = get_schema_path(self);
        let table = get_table_path(self);
        schema.exists() && table.exists()
    }

    fn exists_or_err(&self) -> TableResult<()> {
        Database::exists_or_err(self.db)?;

        if !self.exist() {
            Err(TableError::TableNotFond(self.table_name.to_string()))
        } else {
            Ok(())
        }
    }

    fn match_query(condition: &Option<Condition>, entry: &HashMap<String, String>) -> bool {
        if condition.is_none() {
            return true;
        }

        let Condition {
            key,
            value,
            operator,
        } = condition.as_ref().unwrap();

        match entry.get(key) {
            None => false,
            Some(v) => match operator {
                Operator::Eq => v == value,
                Operator::NotEq => v != value,
                Operator::Gt => v > value,
                Operator::Lt => v < value,
                Operator::GtEq => v >= value,
                Operator::LtEq => v <= value,
            },
        }
    }

    fn get_col_pos(&self, schema: &Schema, col_name: &str) -> Option<usize> {
        schema.cols.iter().position(|c| c == col_name)
    }

    fn col_exist(&self, schema: &Schema, col_name: &str) -> bool {
        self.get_col_pos(schema, col_name).is_some()
    }

    fn col_exist_or_err(&self, schema: &Schema, col_name: &str) -> TableResult<()> {
        if !self.col_exist(schema, col_name) {
            Err(TableError::ColNotFound(col_name.to_string()))
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Schema {
    cols: Vec<String>,
    types: Vec<DataType>,
}
