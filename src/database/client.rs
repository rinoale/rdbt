use color_eyre::eyre::Result;
use sqlx::{
    Column, MySqlPool, PgPool, Row, TypeInfo, ValueRef,
    mysql::{MySqlPoolOptions, MySqlRow},
    postgres::{PgPoolOptions, PgRow},
    types::time::{Date, OffsetDateTime, PrimitiveDateTime, Time},
};

use crate::args::{Config, Dbms};

#[derive(Debug, Clone, Default)]
pub struct QueryOutput {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub message: String,
}

impl QueryOutput {
    pub fn message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            ..Self::default()
        }
    }
}

pub enum DatabaseClient {
    Postgres(PgPool),
    Mysql(MySqlPool),
}

impl DatabaseClient {
    pub async fn connect(config: &Config) -> Result<Self> {
        match config.dbms {
            Dbms::Postgres => {
                let pool = PgPoolOptions::new()
                    .max_connections(5)
                    .connect(&config.url)
                    .await?;
                Ok(Self::Postgres(pool))
            }
            Dbms::Mysql => {
                let pool = MySqlPoolOptions::new()
                    .max_connections(5)
                    .connect(&config.url)
                    .await?;
                Ok(Self::Mysql(pool))
            }
        }
    }

    pub async fn query(&self, sql: &str) -> Result<QueryOutput> {
        match self {
            Self::Postgres(pool) => {
                let rows = sqlx::query(sql).fetch_all(pool).await?;
                Ok(postgres_rows(rows))
            }
            Self::Mysql(pool) => {
                let rows = sqlx::raw_sql(sql).fetch_all(pool).await?;
                Ok(mysql_rows(rows))
            }
        }
    }

    pub async fn execute(&self, sql: &str) -> Result<QueryOutput> {
        match self {
            Self::Postgres(pool) => {
                let result = sqlx::query(sql).execute(pool).await?;
                Ok(QueryOutput::message(format!(
                    "OK, {} row(s) affected",
                    result.rows_affected()
                )))
            }
            Self::Mysql(pool) => {
                let result = sqlx::query(sql).execute(pool).await?;
                Ok(QueryOutput::message(format!(
                    "OK, {} row(s) affected",
                    result.rows_affected()
                )))
            }
        }
    }
}

fn postgres_rows(rows: Vec<PgRow>) -> QueryOutput {
    let columns = rows
        .first()
        .map(|row| {
            row.columns()
                .iter()
                .map(|column| column.name().to_string())
                .collect()
        })
        .unwrap_or_default();

    let row_count = rows.len();
    let rows = rows
        .iter()
        .map(|row| {
            (0..row.columns().len())
                .map(|index| postgres_value(row, index))
                .collect()
        })
        .collect();

    QueryOutput {
        columns,
        rows,
        message: format!("{row_count} row(s)"),
    }
}

fn mysql_rows(rows: Vec<MySqlRow>) -> QueryOutput {
    let columns = rows
        .first()
        .map(|row| {
            row.columns()
                .iter()
                .map(|column| column.name().to_string())
                .collect()
        })
        .unwrap_or_default();

    let row_count = rows.len();
    let rows = rows
        .iter()
        .map(|row| {
            (0..row.columns().len())
                .map(|index| mysql_value(row, index))
                .collect()
        })
        .collect();

    QueryOutput {
        columns,
        rows,
        message: format!("{row_count} row(s)"),
    }
}

fn postgres_value(row: &PgRow, index: usize) -> String {
    match row.try_get_raw(index) {
        Ok(value) if value.is_null() => "NULL".to_string(),
        Ok(value) => {
            let type_name = value.type_info().name().to_ascii_lowercase();
            decode_postgres_by_type(row, index, &type_name)
                .or_else(|| decode_text(row, index))
                .unwrap_or_else(|| format!("<{type_name}>"))
        }
        Err(error) => format!("<decode error: {error}>"),
    }
}

fn mysql_value(row: &MySqlRow, index: usize) -> String {
    match row.try_get_raw(index) {
        Ok(value) if value.is_null() => "NULL".to_string(),
        Ok(value) => {
            let type_name = value.type_info().name().to_ascii_lowercase();
            decode_mysql_by_type(row, index, &type_name)
                .or_else(|| decode_text(row, index))
                .unwrap_or_else(|| format!("<{type_name}>"))
        }
        Err(error) => format!("<decode error: {error}>"),
    }
}

fn decode_text<'r, R>(row: &'r R, index: usize) -> Option<String>
where
    R: Row,
    usize: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database>,
{
    row.try_get_unchecked::<String, _>(index).ok()
}

fn decode_postgres_by_type(row: &PgRow, index: usize, type_name: &str) -> Option<String> {
    match type_name {
        "int2" => row
            .try_get::<i16, _>(index)
            .ok()
            .map(|value| value.to_string()),
        "int4" => row
            .try_get::<i32, _>(index)
            .ok()
            .map(|value| value.to_string()),
        "int8" => row
            .try_get::<i64, _>(index)
            .ok()
            .map(|value| value.to_string()),
        "float4" => row
            .try_get::<f32, _>(index)
            .ok()
            .map(|value| value.to_string()),
        "float8" => row
            .try_get::<f64, _>(index)
            .ok()
            .map(|value| value.to_string()),
        "numeric" => row.try_get::<String, _>(index).ok().or_else(|| {
            row.try_get::<f64, _>(index)
                .ok()
                .map(|value| value.to_string())
        }),
        "bool" => row
            .try_get::<bool, _>(index)
            .ok()
            .map(|value| value.to_string()),
        "text" | "varchar" | "bpchar" | "name" => row.try_get::<String, _>(index).ok(),
        "date" => row
            .try_get::<Date, _>(index)
            .ok()
            .map(|value| value.to_string()),
        "time" => row
            .try_get::<Time, _>(index)
            .ok()
            .map(|value| value.to_string()),
        "timestamp" => row
            .try_get::<PrimitiveDateTime, _>(index)
            .ok()
            .map(|value| value.to_string()),
        "timestamptz" => row
            .try_get::<OffsetDateTime, _>(index)
            .ok()
            .map(|value| value.to_string()),
        _ => None,
    }
}

fn decode_mysql_by_type(row: &MySqlRow, index: usize, type_name: &str) -> Option<String> {
    match type_name {
        "boolean" => decode_mysql_unsigned_integer(row, index)
            .or_else(|| decode_mysql_signed_integer(row, index)),
        "tinyint" | "smallint" | "mediumint" | "int" | "integer" | "bigint" => {
            decode_mysql_signed_integer(row, index)
        }
        "tinyint unsigned" | "smallint unsigned" | "mediumint unsigned" | "int unsigned"
        | "integer unsigned" | "bigint unsigned" => decode_mysql_unsigned_integer(row, index),
        "float" => row
            .try_get::<f32, _>(index)
            .ok()
            .map(|value| value.to_string()),
        "double" | "decimal" => row
            .try_get::<f64, _>(index)
            .ok()
            .map(|value| value.to_string()),
        "char" | "varchar" | "text" | "tinytext" | "mediumtext" | "longtext" => {
            row.try_get::<String, _>(index).ok()
        }
        "date" => row
            .try_get::<Date, _>(index)
            .ok()
            .map(|value| value.to_string()),
        "time" => row
            .try_get::<Time, _>(index)
            .ok()
            .map(|value| value.to_string()),
        "datetime" => row
            .try_get::<PrimitiveDateTime, _>(index)
            .ok()
            .map(|value| value.to_string()),
        "timestamp" => row
            .try_get::<OffsetDateTime, _>(index)
            .ok()
            .map(|value| value.to_string()),
        _ => None,
    }
}

fn decode_mysql_signed_integer(row: &MySqlRow, index: usize) -> Option<String> {
    row.try_get_unchecked::<i64, _>(index)
        .ok()
        .or_else(|| row.try_get_unchecked::<i32, _>(index).ok().map(i64::from))
        .or_else(|| row.try_get_unchecked::<i16, _>(index).ok().map(i64::from))
        .or_else(|| row.try_get_unchecked::<i8, _>(index).ok().map(i64::from))
        .map(|value| value.to_string())
}

fn decode_mysql_unsigned_integer(row: &MySqlRow, index: usize) -> Option<String> {
    row.try_get_unchecked::<u64, _>(index)
        .ok()
        .or_else(|| row.try_get_unchecked::<u32, _>(index).ok().map(u64::from))
        .or_else(|| row.try_get_unchecked::<u16, _>(index).ok().map(u64::from))
        .or_else(|| row.try_get_unchecked::<u8, _>(index).ok().map(u64::from))
        .map(|value| value.to_string())
}
