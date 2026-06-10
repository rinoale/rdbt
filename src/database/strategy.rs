use crate::args::Dbms;

#[derive(Debug, Clone, Default)]
pub struct MetadataCache {
    pub loaded: bool,
    pub schemas: Vec<String>,
    pub tables: Vec<TableRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRef {
    pub schema: String,
    pub name: String,
    pub kind: String,
}

impl TableRef {
    pub fn display_name(&self) -> String {
        format!("{}.{}", self.schema, self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SampleOrder {
    Natural,
    Asc(String),
    Desc(String),
}

pub trait DatabaseStrategy: Send + Sync {
    fn name(&self) -> &'static str;
    fn list_schemas_sql(&self) -> String;
    fn list_tables_sql(&self) -> String;
    fn describe_table_sql(&self, table: &TableRef) -> String;
    fn sample_rows_sql(&self, table: &TableRef, limit: u16, order: &SampleOrder) -> String;
}

pub fn strategy_for(dbms: Dbms) -> Box<dyn DatabaseStrategy> {
    match dbms {
        Dbms::Postgres => Box::new(PostgresStrategy),
        Dbms::Mysql => Box::new(MySqlStrategy),
    }
}

struct PostgresStrategy;
struct MySqlStrategy;

impl DatabaseStrategy for PostgresStrategy {
    fn name(&self) -> &'static str {
        "PostgreSQL"
    }

    fn list_schemas_sql(&self) -> String {
        r#"
select schema_name as schema
from information_schema.schemata
where schema_name not in ('pg_catalog', 'information_schema')
order by schema_name
"#
        .trim()
        .to_string()
    }

    fn list_tables_sql(&self) -> String {
        r#"
select table_schema as schema, table_name as table, table_type as type
from information_schema.tables
where table_schema not in ('pg_catalog', 'information_schema')
order by table_schema, table_name
"#
        .trim()
        .to_string()
    }

    fn describe_table_sql(&self, table: &TableRef) -> String {
        format!(
            r#"
select ordinal_position as ordinal,
       column_name as column,
       data_type as type,
       is_nullable as nullable,
       column_default as default
from information_schema.columns
where table_schema = {}
  and table_name = {}
order by ordinal_position
"#,
            postgres_literal(&table.schema),
            postgres_literal(&table.name)
        )
        .trim()
        .to_string()
    }

    fn sample_rows_sql(&self, table: &TableRef, limit: u16, order: &SampleOrder) -> String {
        format!(
            "select * from {}.{}{} limit {}",
            postgres_ident(&table.schema),
            postgres_ident(&table.name),
            postgres_order(order),
            limit
        )
    }
}

impl DatabaseStrategy for MySqlStrategy {
    fn name(&self) -> &'static str {
        "MySQL"
    }

    fn list_schemas_sql(&self) -> String {
        r#"
select schema_name as `schema`
from information_schema.schemata
where schema_name not in ('information_schema', 'mysql', 'performance_schema', 'sys')
order by schema_name
"#
        .trim()
        .to_string()
    }

    fn list_tables_sql(&self) -> String {
        r#"
select table_schema as `schema`, table_name as `table`, table_type as `type`
from information_schema.tables
where table_schema not in ('information_schema', 'mysql', 'performance_schema', 'sys')
order by table_schema, table_name
"#
        .trim()
        .to_string()
    }

    fn describe_table_sql(&self, table: &TableRef) -> String {
        format!(
            r#"
select ordinal_position as ordinal,
       column_name as `column`,
       column_type as `type`,
       is_nullable as nullable,
       column_default as `default`
from information_schema.columns
where table_schema = {}
  and table_name = {}
order by ordinal_position
"#,
            mysql_literal(&table.schema),
            mysql_literal(&table.name)
        )
        .trim()
        .to_string()
    }

    fn sample_rows_sql(&self, table: &TableRef, limit: u16, order: &SampleOrder) -> String {
        format!(
            "select * from {}.{}{} limit {}",
            mysql_ident(&table.schema),
            mysql_ident(&table.name),
            mysql_order(order),
            limit
        )
    }
}

fn postgres_ident(value: &str) -> String {
    format!(r#""{}""#, value.replace('"', r#""""#))
}

fn mysql_ident(value: &str) -> String {
    format!("`{}`", value.replace('`', "``"))
}

fn postgres_order(order: &SampleOrder) -> String {
    match order {
        SampleOrder::Natural => String::new(),
        SampleOrder::Asc(column) => format!(" order by {} asc", postgres_ident(column)),
        SampleOrder::Desc(column) => format!(" order by {} desc", postgres_ident(column)),
    }
}

fn mysql_order(order: &SampleOrder) -> String {
    match order {
        SampleOrder::Natural => String::new(),
        SampleOrder::Asc(column) => format!(" order by {} asc", mysql_ident(column)),
        SampleOrder::Desc(column) => format!(" order by {} desc", mysql_ident(column)),
    }
}

fn postgres_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn mysql_literal(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

#[cfg(test)]
mod tests {
    use super::{SampleOrder, TableRef, strategy_for};
    use crate::args::Dbms;

    #[test]
    fn postgres_uses_information_schema() {
        let strategy = strategy_for(Dbms::Postgres);
        assert!(
            strategy
                .list_tables_sql()
                .contains("information_schema.tables")
        );
    }

    #[test]
    fn mysql_uses_information_schema() {
        let strategy = strategy_for(Dbms::Mysql);
        assert!(
            strategy
                .list_tables_sql()
                .contains("information_schema.tables")
        );
    }

    #[test]
    fn quotes_postgres_sample_table() {
        let strategy = strategy_for(Dbms::Postgres);
        let sql = strategy.sample_rows_sql(
            &TableRef {
                schema: "public".to_string(),
                name: "user events".to_string(),
                kind: "BASE TABLE".to_string(),
            },
            100,
            &SampleOrder::Natural,
        );
        assert_eq!(sql, r#"select * from "public"."user events" limit 100"#);
    }

    #[test]
    fn quotes_postgres_order_column() {
        let strategy = strategy_for(Dbms::Postgres);
        let sql = strategy.sample_rows_sql(
            &TableRef {
                schema: "public".to_string(),
                name: "events".to_string(),
                kind: "BASE TABLE".to_string(),
            },
            10,
            &SampleOrder::Desc("created at".to_string()),
        );
        assert_eq!(
            sql,
            r#"select * from "public"."events" order by "created at" desc limit 10"#
        );
    }
}
