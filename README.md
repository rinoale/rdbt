# rdbt

`rdbt` is a Rust/Ratatui terminal database client for PostgreSQL and MySQL.
It keeps the familiar SQL prompt workflow from `psql` and `mysql`, but adds a
schema/table browser, cached metadata, query output tables, and a visible
safe-mode theme.

## Toolchain

- Rust: `1.96.0`
- Ratatui: `0.30.1`

## Usage

Connect with a URL:

```sh
rdbt postgres --url postgres://user:password@localhost:5432/app
rdbt mysql --url mysql://user:password@localhost:3306/app
```

Or connect with client-style flags:

```sh
rdbt postgres -u postgres -p -d app --host localhost -P 5432
rdbt mysql -u root -p -d app --host localhost -P 3306
```

By default, `rdbt` starts in safe mode. Safe mode only permits SQL statements
that are conservatively classified as read-only `SELECT` queries. Use
`--unsafe-mode`, `F2`, or `:unsafe` when you intentionally want to allow writes.

## Keys

- `Enter`: execute the current SQL command, or sample selected table when the prompt is empty
- `F2`: toggle safe mode
- `F5`: refresh schema/table metadata
- `Tab`: switch focus between browser and SQL prompt
- `Up`/`Down`: move table selection or query history
- `Ctrl-C`, `Esc`: quit

## rdbt Commands

The `:` commands are database-neutral and use a strategy implementation for the
connected DBMS.

- `:schemas`: list schemas/databases
- `:tables`: list tables
- `:describe schema.table`: show columns for a table
- `:sample schema.table`: show the first 100 rows
- `:refresh`: clear and reload metadata
- `:safe`, `:unsafe`, `:safe toggle`: change safe mode
- `:help`: show command help
- `:quit`: exit

Compatibility aliases are routed through the same strategy layer:

- `\dn`, `show schemas`, `show databases` -> `:schemas`
- `\dt`, `show tables` -> `:tables`
- `\d schema.table`, `desc schema.table`, `describe schema.table` -> `:describe schema.table`
- `\q` -> `:quit`
