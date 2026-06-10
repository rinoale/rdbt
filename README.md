# rdbt

`rdbt` is a Rust/Ratatui terminal database client for PostgreSQL and MySQL.
It keeps the familiar SQL prompt workflow from `psql` and `mysql`, but adds a
schema/table browser, cached metadata, query output tables, and a visible
safe-mode theme.

## Toolchain

- Rust: `1.96.0`
- Ratatui: `0.30.1`

## Usage

Start onboarding prompts:

```sh
rdbt
```

The onboarding flow asks for connector, host, port, user, password, and optional
schema/database before the TUI starts. Paste works in these prompts, including
the hidden password entry.

Connect with a URL:

```sh
rdbt --url postgres://user:password@localhost:5432/app
rdbt --url mysql://user:password@localhost:3306/app
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
- `:`: focus SQL prompt and start an rdbt command from any pane
- `Up`/`Down`: move table selection or query history
- Mouse wheel: scroll the browser or output under the pointer
- Browser click: open a read-only table detail view with columns and 10 sampled rows
- Preview dropdowns: adjust the table detail row limit and first-column order
- `Ctrl-C`, `Esc`: quit

Mouse interaction is read-only. It can inspect metadata, sample rows, scroll, and
change preview options, but it never updates column values or table definitions.

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
