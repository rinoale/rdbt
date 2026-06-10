use std::time::Duration;

use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, List, ListItem, Paragraph, Row, Table, Wrap},
};

use crate::{
    args::Config,
    database::{
        DatabaseClient, DatabaseStrategy, MetadataCache, QueryOutput, TableRef, strategy_for,
    },
    safety,
};

const SAMPLE_LIMIT: u16 = 100;

pub async fn run(mut app: App) -> Result<()> {
    let mut terminal = ratatui::init();
    let result = app.run(&mut terminal).await;
    ratatui::restore();
    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Browser,
    Prompt,
}

#[derive(Debug, Clone)]
struct Theme {
    accent: Color,
    accent_dark: Color,
    background: Color,
    panel: Color,
    border: Color,
    selected: Color,
    text: Color,
    muted: Color,
    danger: Color,
}

impl Theme {
    fn safe() -> Self {
        Self {
            accent: Color::Green,
            accent_dark: Color::Rgb(0, 75, 45),
            background: Color::Rgb(4, 18, 12),
            panel: Color::Rgb(8, 32, 22),
            border: Color::Rgb(24, 130, 82),
            selected: Color::Rgb(26, 91, 62),
            text: Color::Rgb(225, 247, 235),
            muted: Color::Rgb(131, 179, 154),
            danger: Color::LightRed,
        }
    }

    fn unsafe_mode() -> Self {
        Self {
            accent: Color::Red,
            accent_dark: Color::Rgb(93, 22, 25),
            background: Color::Rgb(25, 8, 10),
            panel: Color::Rgb(49, 15, 19),
            border: Color::Rgb(183, 53, 59),
            selected: Color::Rgb(103, 31, 36),
            text: Color::Rgb(255, 230, 230),
            muted: Color::Rgb(207, 143, 145),
            danger: Color::LightYellow,
        }
    }
}

pub struct App {
    config: Config,
    client: DatabaseClient,
    strategy: Box<dyn DatabaseStrategy>,
    metadata: MetadataCache,
    output: QueryOutput,
    input: String,
    status: String,
    history: Vec<String>,
    history_cursor: Option<usize>,
    focus: Focus,
    selected_table: usize,
    should_quit: bool,
}

impl App {
    pub fn new(config: Config, client: DatabaseClient) -> Self {
        let strategy = strategy_for(config.dbms);
        let db_name = config
            .database
            .clone()
            .unwrap_or_else(|| "database".to_string());
        Self {
            config,
            client,
            strategy,
            metadata: MetadataCache::default(),
            output: QueryOutput::message(format!("Connected to {db_name}. Loading metadata...")),
            input: String::new(),
            status: "Connected".to_string(),
            history: Vec::new(),
            history_cursor: None,
            focus: Focus::Prompt,
            selected_table: 0,
            should_quit: false,
        }
    }

    async fn run(&mut self, terminal: &mut ratatui::DefaultTerminal) -> Result<()> {
        self.load_metadata_if_needed().await;

        while !self.should_quit {
            terminal.draw(|frame| self.render(frame))?;

            if event::poll(Duration::from_millis(100))?
                && let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
            {
                self.handle_key(key).await;
            }
        }

        Ok(())
    }

    async fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Esc => self.should_quit = true,
            KeyCode::F(2) => self.toggle_safe_mode(),
            KeyCode::F(5) => self.refresh_metadata().await,
            KeyCode::Tab => {
                self.focus = if self.focus == Focus::Prompt {
                    Focus::Browser
                } else {
                    Focus::Prompt
                }
            }
            KeyCode::Enter => self.submit().await,
            KeyCode::Backspace if self.focus == Focus::Prompt => {
                self.input.pop();
            }
            KeyCode::Char(ch) if self.focus == Focus::Prompt => {
                self.input.push(ch);
                self.history_cursor = None;
            }
            KeyCode::Up => self.move_up(),
            KeyCode::Down => self.move_down(),
            _ => {}
        }
    }

    fn move_up(&mut self) {
        match self.focus {
            Focus::Browser => {
                self.selected_table = self.selected_table.saturating_sub(1);
            }
            Focus::Prompt => {
                if self.history.is_empty() {
                    return;
                }
                let index = self
                    .history_cursor
                    .map_or(self.history.len().saturating_sub(1), |cursor| {
                        cursor.saturating_sub(1)
                    });
                self.history_cursor = Some(index);
                self.input = self.history[index].clone();
            }
        }
    }

    fn move_down(&mut self) {
        match self.focus {
            Focus::Browser => {
                if self.selected_table + 1 < self.metadata.tables.len() {
                    self.selected_table += 1;
                }
            }
            Focus::Prompt => {
                let Some(cursor) = self.history_cursor else {
                    return;
                };
                if cursor + 1 < self.history.len() {
                    self.history_cursor = Some(cursor + 1);
                    self.input = self.history[cursor + 1].clone();
                } else {
                    self.history_cursor = None;
                    self.input.clear();
                }
            }
        }
    }

    async fn submit(&mut self) {
        let command = self.input.trim().to_string();
        if command.is_empty() {
            if let Some(table) = self.metadata.tables.get(self.selected_table).cloned() {
                self.sample_table(&table).await;
            }
            return;
        }

        self.history.push(command.clone());
        self.history_cursor = None;
        self.input.clear();

        if let Some(command) = normalize_client_command(&command) {
            self.run_rdbt_command(&command).await;
        } else {
            self.run_sql(&command).await;
        }
    }

    async fn run_rdbt_command(&mut self, command: &str) {
        let mut parts = command[1..].split_whitespace();
        let Some(name) = parts.next() else {
            return;
        };

        match name {
            "q" | "quit" | "exit" => self.should_quit = true,
            "help" | "?" => self.show_help(),
            "safe" => match parts.next() {
                Some("off") => self.set_safe_mode(false),
                Some("on") => self.set_safe_mode(true),
                Some("toggle") | None => self.toggle_safe_mode(),
                Some(_) => self.status = "usage: :safe [on|off|toggle]".to_string(),
            },
            "unsafe" => self.set_safe_mode(false),
            "refresh" => self.refresh_metadata().await,
            "schemas" => {
                self.run_strategy_query(self.strategy.list_schemas_sql(), "schemas")
                    .await
            }
            "tables" => {
                self.run_strategy_query(self.strategy.list_tables_sql(), "tables")
                    .await
            }
            "describe" | "desc" => {
                if let Some(table) = self.resolve_table(parts.next()) {
                    let sql = self.strategy.describe_table_sql(&table);
                    self.run_strategy_query(sql, "describe").await;
                } else {
                    self.status = "usage: :describe schema.table".to_string();
                }
            }
            "sample" | "select" => {
                if let Some(table) = self.resolve_table(parts.next()) {
                    self.sample_table(&table).await;
                } else {
                    self.status = "usage: :sample schema.table".to_string();
                }
            }
            _ => self.status = format!("unknown rdbt command: :{name}"),
        }
    }

    async fn run_sql(&mut self, sql: &str) {
        if self.config.safe_mode {
            let decision = safety::classify(sql);
            if !decision.is_allowed() {
                self.output = QueryOutput::message(match decision {
                    safety::SafetyDecision::Allow => "allowed".to_string(),
                    safety::SafetyDecision::Deny(reason) => reason,
                });
                self.status = "blocked by safe mode".to_string();
                return;
            }
        }

        self.status = "running SQL".to_string();
        let result = if safety::returns_rows(sql) {
            self.client.query(sql).await
        } else {
            self.client.execute(sql).await
        };

        self.set_output_result(result, "SQL complete");
    }

    async fn run_strategy_query(&mut self, sql: String, label: &str) {
        self.status = format!("running {label}");
        let result = self.client.query(&sql).await;
        self.set_output_result(result, label);
    }

    async fn sample_table(&mut self, table: &TableRef) {
        let sql = self.strategy.sample_rows_sql(table, SAMPLE_LIMIT);
        self.run_strategy_query(sql, &format!("sample {}", table.display_name()))
            .await;
    }

    async fn refresh_metadata(&mut self) {
        self.metadata.loaded = false;
        self.metadata.schemas.clear();
        self.metadata.tables.clear();
        self.output = QueryOutput::message("Refreshing metadata...");
        self.load_metadata_if_needed().await;
    }

    async fn load_metadata_if_needed(&mut self) {
        if self.metadata.loaded {
            return;
        }

        let schemas = self.client.query(&self.strategy.list_schemas_sql()).await;
        let tables = self.client.query(&self.strategy.list_tables_sql()).await;

        match (schemas, tables) {
            (Ok(schemas), Ok(tables)) => {
                self.metadata.schemas = rows_by_column(&schemas, "schema");
                self.metadata.tables = table_refs(&tables);
                self.metadata.loaded = true;
                self.selected_table = self
                    .selected_table
                    .min(self.metadata.tables.len().saturating_sub(1));
                self.output = tables;
                self.status = format!(
                    "loaded {} schema(s), {} table(s)",
                    self.metadata.schemas.len(),
                    self.metadata.tables.len()
                );
            }
            (Err(error), _) | (_, Err(error)) => {
                self.output = QueryOutput::message(format!("metadata load failed: {error}"));
                self.status = "metadata load failed".to_string();
            }
        }
    }

    fn resolve_table(&self, value: Option<&str>) -> Option<TableRef> {
        let Some(value) = value else {
            return self.metadata.tables.get(self.selected_table).cloned();
        };

        let (schema, name) = value
            .split_once('.')
            .map_or(("", value), |(schema, name)| (schema, name));

        self.metadata
            .tables
            .iter()
            .find(|table| {
                if schema.is_empty() {
                    table.name == name
                } else {
                    table.schema == schema && table.name == name
                }
            })
            .cloned()
            .or_else(|| {
                if schema.is_empty() {
                    None
                } else {
                    Some(TableRef {
                        schema: schema.to_string(),
                        name: name.to_string(),
                        kind: "TABLE".to_string(),
                    })
                }
            })
    }

    fn set_output_result(&mut self, result: Result<QueryOutput>, status: impl Into<String>) {
        match result {
            Ok(output) => {
                self.output = output;
                self.status = status.into();
            }
            Err(error) => {
                self.output = QueryOutput::message(error.to_string());
                self.status = "SQL failed".to_string();
            }
        }
    }

    fn set_safe_mode(&mut self, safe_mode: bool) {
        self.config.safe_mode = safe_mode;
        self.status = if safe_mode {
            "safe mode enabled".to_string()
        } else {
            "unsafe mode enabled".to_string()
        };
    }

    fn toggle_safe_mode(&mut self) {
        self.set_safe_mode(!self.config.safe_mode);
    }

    fn show_help(&mut self) {
        self.output = QueryOutput {
            columns: vec!["command".to_string(), "description".to_string()],
            rows: vec![
                vec![":schemas".to_string(), "list schemas/databases".to_string()],
                vec![":tables".to_string(), "list tables".to_string()],
                vec![
                    ":describe schema.table".to_string(),
                    "show table columns".to_string(),
                ],
                vec![
                    ":sample schema.table".to_string(),
                    "show first 100 rows".to_string(),
                ],
                vec![
                    "\\dt, show tables".to_string(),
                    "list tables through the strategy layer".to_string(),
                ],
                vec![
                    "\\d table, desc table".to_string(),
                    "describe a table through the strategy layer".to_string(),
                ],
                vec![":refresh".to_string(), "reload metadata".to_string()],
                vec![
                    ":safe [on|off|toggle]".to_string(),
                    "change safe mode".to_string(),
                ],
                vec![":quit".to_string(), "exit rdbt".to_string()],
            ],
            message: "rdbt commands".to_string(),
        };
        self.status = "help".to_string();
    }

    fn render(&self, frame: &mut Frame) {
        let theme = if self.config.safe_mode {
            Theme::safe()
        } else {
            Theme::unsafe_mode()
        };

        frame.render_widget(Clear, frame.area());
        frame.render_widget(
            Block::new().style(Style::default().bg(theme.background)),
            frame.area(),
        );

        let [top, body, bottom] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .areas(frame.area());

        let [browser, main] =
            Layout::horizontal([Constraint::Percentage(30), Constraint::Percentage(70)])
                .areas(body);

        self.render_top(frame, top, &theme);
        self.render_browser(frame, browser, &theme);
        self.render_output(frame, main, &theme);
        self.render_prompt(frame, bottom, &theme);
    }

    fn render_top(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let mode = if self.config.safe_mode {
            "SAFE"
        } else {
            "UNSAFE"
        };
        let title = Line::from(vec![
            Span::styled(
                " rdbt ",
                Style::default().fg(Color::Black).bg(theme.accent).bold(),
            ),
            Span::raw(" "),
            Span::styled(self.strategy.name(), Style::default().fg(theme.text).bold()),
            Span::raw(" "),
            Span::styled(
                mode,
                Style::default().fg(theme.text).bg(theme.accent_dark).bold(),
            ),
            Span::raw("  F2 mode  F5 refresh  Tab focus  Esc quit"),
        ]);
        let block = Block::new()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .style(Style::default().bg(theme.panel));
        frame.render_widget(Paragraph::new(title).block(block), area);
    }

    fn render_browser(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let items = if self.metadata.tables.is_empty() {
            vec![ListItem::new(Line::from(Span::styled(
                "no tables loaded",
                Style::default().fg(theme.muted),
            )))]
        } else {
            self.metadata
                .tables
                .iter()
                .enumerate()
                .map(|(index, table)| {
                    let style = if index == self.selected_table {
                        Style::default().fg(theme.text).bg(theme.selected).bold()
                    } else {
                        Style::default().fg(theme.text)
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(table.schema.clone(), Style::default().fg(theme.muted)),
                        Span::raw("."),
                        Span::styled(table.name.clone(), style),
                    ]))
                })
                .collect()
        };

        let title = format!(
            "Browser {}",
            if self.focus == Focus::Browser {
                "[focus]"
            } else {
                ""
            }
        );
        let block = Block::new()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .style(Style::default().bg(theme.panel));
        let list = List::new(items)
            .block(block)
            .style(Style::default().bg(theme.panel));

        frame.render_widget(list, area);
    }

    fn render_output(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if self.output.columns.is_empty() {
            let block = Block::new()
                .title("Output")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border))
                .style(Style::default().bg(theme.panel));
            let paragraph = Paragraph::new(self.output.message.clone())
                .block(block)
                .style(Style::default().fg(theme.text).bg(theme.panel))
                .wrap(Wrap { trim: false });
            frame.render_widget(paragraph, area);
            return;
        }

        let widths = table_widths(&self.output, area.width.saturating_sub(4));
        let constraints = widths
            .iter()
            .copied()
            .map(Constraint::Length)
            .collect::<Vec<_>>();
        let header = Row::new(self.output.columns.iter().map(|column| {
            Cell::from(column.clone())
                .style(Style::default().fg(Color::Black).bg(theme.accent).bold())
        }));
        let rows = self.output.rows.iter().map(|row| {
            Row::new(row.iter().map(|value| {
                Cell::from(truncate(value, 64))
                    .style(Style::default().fg(theme.text).bg(theme.panel))
            }))
        });
        let table = Table::new(rows, constraints)
            .header(header)
            .block(
                Block::new()
                    .title(format!("Output - {}", self.output.message))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border))
                    .style(Style::default().bg(theme.panel)),
            )
            .column_spacing(1)
            .style(Style::default().bg(theme.panel));

        frame.render_widget(table, area);
    }

    fn render_prompt(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let status_style = if self.config.safe_mode {
            Style::default().fg(theme.muted)
        } else {
            Style::default().fg(theme.danger).bold()
        };
        let title = format!(
            "SQL {}",
            if self.focus == Focus::Prompt {
                "[focus]"
            } else {
                ""
            }
        );
        let prompt = Line::from(vec![
            Span::styled(
                format!("{}> ", self.config.dbms),
                Style::default().fg(theme.accent).bold(),
            ),
            Span::styled(self.input.clone(), Style::default().fg(theme.text)),
        ]);
        let footer = Line::from(vec![
            Span::styled(self.status.clone(), status_style),
            Span::raw("  "),
            Span::styled(
                if self.config.safe_mode {
                    "writes blocked"
                } else {
                    "writes allowed"
                },
                Style::default().fg(theme.text).bg(theme.accent_dark),
            ),
        ]);
        let text = vec![prompt, footer];
        let block = Block::new()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .style(Style::default().bg(theme.panel));
        frame.render_widget(
            Paragraph::new(text)
                .block(block)
                .style(Style::default().fg(theme.text).bg(theme.panel)),
            area,
        );
    }
}

fn normalize_client_command(command: &str) -> Option<String> {
    let trimmed = command.trim();
    if trimmed.starts_with(':') {
        return Some(trimmed.to_string());
    }

    match trimmed {
        "\\q" => return Some(":quit".to_string()),
        "\\?" => return Some(":help".to_string()),
        "\\dn" => return Some(":schemas".to_string()),
        "\\dt" => return Some(":tables".to_string()),
        _ => {}
    }

    if let Some(table) = trimmed.strip_prefix("\\d ") {
        let table = table.trim();
        if !table.is_empty() {
            return Some(format!(":describe {table}"));
        }
    }

    let without_semicolon = trimmed.trim_end_matches(';').trim();
    if without_semicolon.eq_ignore_ascii_case("show schemas")
        || without_semicolon.eq_ignore_ascii_case("show databases")
    {
        return Some(":schemas".to_string());
    }

    if without_semicolon.eq_ignore_ascii_case("show tables") {
        return Some(":tables".to_string());
    }

    for prefix in ["describe ", "desc "] {
        if without_semicolon
            .get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
        {
            let table = without_semicolon[prefix.len()..].trim();
            if !table.is_empty() {
                return Some(format!(":describe {table}"));
            }
        }
    }

    None
}

fn rows_by_column(output: &QueryOutput, name: &str) -> Vec<String> {
    let Some(index) = column_index(output, name) else {
        return Vec::new();
    };

    output
        .rows
        .iter()
        .filter_map(|row| row.get(index).cloned())
        .collect()
}

fn table_refs(output: &QueryOutput) -> Vec<TableRef> {
    let Some(schema_index) = column_index(output, "schema") else {
        return Vec::new();
    };
    let Some(table_index) = column_index(output, "table") else {
        return Vec::new();
    };
    let type_index = column_index(output, "type");

    output
        .rows
        .iter()
        .filter_map(|row| {
            Some(TableRef {
                schema: row.get(schema_index)?.clone(),
                name: row.get(table_index)?.clone(),
                kind: type_index
                    .and_then(|index| row.get(index).cloned())
                    .unwrap_or_else(|| "TABLE".to_string()),
            })
        })
        .collect()
}

fn column_index(output: &QueryOutput, name: &str) -> Option<usize> {
    output
        .columns
        .iter()
        .position(|column| column.eq_ignore_ascii_case(name))
}

fn table_widths(output: &QueryOutput, max_width: u16) -> Vec<u16> {
    if output.columns.is_empty() {
        return Vec::new();
    }

    let mut widths = output
        .columns
        .iter()
        .enumerate()
        .map(|(index, column)| {
            let row_width = output
                .rows
                .iter()
                .filter_map(|row| row.get(index))
                .map(|value| value.chars().count())
                .max()
                .unwrap_or(0);
            column.chars().count().max(row_width).clamp(6, 32) as u16
        })
        .collect::<Vec<_>>();

    while widths.iter().sum::<u16>() + widths.len().saturating_sub(1) as u16 > max_width {
        if let Some((index, width)) = widths
            .iter_mut()
            .enumerate()
            .max_by_key(|(_, width)| **width)
        {
            if *width <= 8 {
                break;
            }
            let _ = index;
            *width -= 1;
        } else {
            break;
        }
    }

    widths
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }

    let mut truncated = value
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    truncated.push_str("...");
    truncated
}

#[cfg(test)]
mod tests {
    use super::normalize_client_command;

    #[test]
    fn normalizes_psql_table_alias() {
        assert_eq!(
            normalize_client_command("\\dt"),
            Some(":tables".to_string())
        );
        assert_eq!(
            normalize_client_command("\\d public.users"),
            Some(":describe public.users".to_string())
        );
    }

    #[test]
    fn normalizes_mysql_table_alias() {
        assert_eq!(
            normalize_client_command("show tables;"),
            Some(":tables".to_string())
        );
        assert_eq!(
            normalize_client_command("DESC users;"),
            Some(":describe users".to_string())
        );
    }
}
