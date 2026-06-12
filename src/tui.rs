use std::{io::stdout, time::Duration};

mod command;
mod keymap;
mod menu;
pub(crate) mod theme;

use color_eyre::Result;
use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        MouseButton, MouseEvent, MouseEventKind,
    },
    execute,
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, List, ListItem, Paragraph, Row, Table, Wrap},
};
use rustui::{runtime::spawn_event_reader, style::Role};
use tokio::{sync::mpsc, time};

use crate::{
    args::Config,
    database::{
        DatabaseClient, DatabaseStrategy, MetadataCache, QueryOutput, SampleOrder, TableRef,
        strategy_for,
    },
    safety,
};

use self::{
    command::{RdbtCommand, SafeModeCommand},
    keymap::{Intent, Keymap, text_input_modifiers},
    theme::{Theme, ThemeKind},
};

const COMMAND_SAMPLE_LIMIT: u16 = 100;
const DEFAULT_PREVIEW_LIMIT: u16 = 10;
const LIMIT_OPTIONS: &[u16] = &[10, 25, 50, 100];
const SCROLL_STEP: isize = 3;

type TaskResult<T> = std::result::Result<T, String>;

pub async fn run(mut app: App) -> Result<()> {
    let mut terminal = ratatui::init();
    if let Err(error) = execute!(stdout(), EnableMouseCapture) {
        ratatui::restore();
        return Err(error.into());
    }
    let result = app.run(&mut terminal).await;
    let disable_result = execute!(stdout(), DisableMouseCapture);
    ratatui::restore();
    disable_result?;
    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Browser,
    Prompt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DropdownKind {
    Limit,
    Order,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewOrder {
    Natural,
    FirstColumnAsc,
    FirstColumnDesc,
}

impl PreviewOrder {
    fn all() -> &'static [Self] {
        &[Self::Natural, Self::FirstColumnAsc, Self::FirstColumnDesc]
    }

    fn label(self, column: Option<&str>) -> String {
        match self {
            Self::Natural => "natural".to_string(),
            Self::FirstColumnAsc => column
                .map(|column| format!("{column} asc"))
                .unwrap_or_else(|| "first column asc".to_string()),
            Self::FirstColumnDesc => column
                .map(|column| format!("{column} desc"))
                .unwrap_or_else(|| "first column desc".to_string()),
        }
    }

    fn to_sample_order(self, column: Option<&str>) -> SampleOrder {
        match (self, column) {
            (Self::Natural, _) | (_, None) => SampleOrder::Natural,
            (Self::FirstColumnAsc, Some(column)) => SampleOrder::Asc(column.to_string()),
            (Self::FirstColumnDesc, Some(column)) => SampleOrder::Desc(column.to_string()),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct PreviewOptions {
    limit: u16,
    order: PreviewOrder,
}

impl Default for PreviewOptions {
    fn default() -> Self {
        Self {
            limit: DEFAULT_PREVIEW_LIMIT,
            order: PreviewOrder::Natural,
        }
    }
}

#[derive(Debug, Clone)]
struct TableDetail {
    table: TableRef,
    columns: QueryOutput,
    rows: QueryOutput,
    order_column: Option<String>,
    options: PreviewOptions,
}

#[derive(Debug, Clone)]
enum OutputView {
    Query(QueryOutput),
    Detail(TableDetail),
}

impl OutputView {
    fn message(message: impl Into<String>) -> Self {
        Self::Query(QueryOutput::message(message))
    }
}

#[derive(Debug, Clone, Copy)]
struct DropdownArea {
    kind: DropdownKind,
    rect: Rect,
}

#[derive(Debug, Clone, Copy)]
struct UiLayout {
    browser: Rect,
    output: Rect,
    output_rows: Rect,
    prompt: Rect,
    limit_control: Rect,
    order_control: Rect,
    dropdown: Option<DropdownArea>,
}

impl Default for UiLayout {
    fn default() -> Self {
        Self {
            browser: Rect::new(0, 0, 0, 0),
            output: Rect::new(0, 0, 0, 0),
            output_rows: Rect::new(0, 0, 0, 0),
            prompt: Rect::new(0, 0, 0, 0),
            limit_control: Rect::new(0, 0, 0, 0),
            order_control: Rect::new(0, 0, 0, 0),
            dropdown: None,
        }
    }
}

enum AppEvent {
    MetadataLoaded {
        task_id: u64,
        schemas: TaskResult<QueryOutput>,
        tables: TaskResult<QueryOutput>,
    },
    QueryFinished {
        task_id: u64,
        result: TaskResult<QueryOutput>,
        success_status: String,
        failure_status: String,
    },
    TableDetailLoaded {
        task_id: u64,
        result: TaskResult<TableDetail>,
    },
}

pub struct App {
    config: Config,
    client: DatabaseClient,
    strategy: Box<dyn DatabaseStrategy>,
    metadata: MetadataCache,
    view: OutputView,
    input: String,
    status: String,
    history: Vec<String>,
    history_cursor: Option<usize>,
    focus: Focus,
    selected_table: usize,
    browser_scroll: usize,
    output_scroll: usize,
    preview_options: PreviewOptions,
    active_dropdown: Option<DropdownKind>,
    layout: UiLayout,
    keymap: Keymap,
    task_tx: mpsc::UnboundedSender<AppEvent>,
    task_rx: mpsc::UnboundedReceiver<AppEvent>,
    next_task_id: u64,
    active_view_task: Option<u64>,
    metadata_task: Option<u64>,
    should_quit: bool,
}

impl App {
    pub fn new(config: Config, client: DatabaseClient) -> Self {
        let strategy = strategy_for(config.dbms);
        let db_name = config
            .database
            .clone()
            .unwrap_or_else(|| "database".to_string());
        let (task_tx, task_rx) = mpsc::unbounded_channel();
        Self {
            config,
            client,
            strategy,
            metadata: MetadataCache::default(),
            view: OutputView::message(format!("Connected to {db_name}. Loading metadata...")),
            input: String::new(),
            status: "Connected".to_string(),
            history: Vec::new(),
            history_cursor: None,
            focus: Focus::Prompt,
            selected_table: 0,
            browser_scroll: 0,
            output_scroll: 0,
            preview_options: PreviewOptions::default(),
            active_dropdown: None,
            layout: UiLayout::default(),
            keymap: Keymap::default(),
            task_tx,
            task_rx,
            next_task_id: 0,
            active_view_task: None,
            metadata_task: None,
            should_quit: false,
        }
    }

    async fn run(&mut self, terminal: &mut ratatui::DefaultTerminal) -> Result<()> {
        self.load_metadata_if_needed();
        let mut events = spawn_event_reader();
        let mut tick = time::interval(Duration::from_millis(100));

        while !self.should_quit {
            self.poll_task_events();
            terminal.draw(|frame| self.render(frame))?;

            tokio::select! {
                maybe_event = events.recv() => {
                    match maybe_event {
                        Some(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                            self.handle_key(key);
                        }
                        Some(Event::Mouse(mouse)) => self.handle_mouse(mouse),
                        Some(_) => {}
                        None => break,
                    }
                }
                _ = tick.tick() => {}
            }
        }

        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if self.handle_prompt_text_key(key) {
            return;
        }

        if let Some(intent) = self.keymap.intent_for(key) {
            self.handle_intent(intent);
        }
    }

    fn handle_prompt_text_key(&mut self, key: KeyEvent) -> bool {
        if self.focus != Focus::Prompt || !text_input_modifiers(key.modifiers) {
            return false;
        }

        match key.code {
            KeyCode::Backspace => {
                self.input.pop();
                true
            }
            KeyCode::Char(ch) => {
                self.input.push(ch);
                self.history_cursor = None;
                true
            }
            _ => false,
        }
    }

    fn handle_intent(&mut self, intent: Intent) {
        match intent {
            Intent::EnterCommandMode => self.enter_command_mode(),
            Intent::Cancel => self.cancel_transient_input(),
            Intent::Help => self.show_help(),
            Intent::ToggleSafeMode => self.toggle_safe_mode(),
            Intent::RefreshMetadata => self.refresh_metadata(),
            Intent::ToggleFocus => self.toggle_focus(),
            Intent::Submit => self.submit(),
            Intent::Previous => self.move_up(),
            Intent::Next => self.move_down(),
        }
    }

    fn toggle_focus(&mut self) {
        self.active_dropdown = None;
        self.focus = if self.focus == Focus::Prompt {
            Focus::Browser
        } else {
            Focus::Prompt
        };
    }

    fn cancel_transient_input(&mut self) {
        if self.active_dropdown.take().is_some() {
            return;
        }

        if self.focus == Focus::Prompt && self.input.starts_with(':') {
            self.input.clear();
            self.focus = Focus::Browser;
            self.history_cursor = None;
            self.status = "command canceled".to_string();
        }
    }

    fn enter_command_mode(&mut self) {
        self.focus = Focus::Prompt;
        self.active_dropdown = None;
        self.history_cursor = None;
        self.input.clear();
        self.input.push(':');
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::ScrollUp => self.scroll_at(mouse.column, mouse.row, -SCROLL_STEP),
            MouseEventKind::ScrollDown => self.scroll_at(mouse.column, mouse.row, SCROLL_STEP),
            MouseEventKind::Down(MouseButton::Left) => {
                self.click_at(mouse.column, mouse.row);
            }
            _ => {}
        }
    }

    fn scroll_at(&mut self, x: u16, y: u16, delta: isize) {
        self.active_dropdown = None;
        if contains(self.layout.browser, x, y) {
            self.focus = Focus::Browser;
            self.browser_scroll = scrolled(
                self.browser_scroll,
                delta,
                self.metadata.tables.len(),
                block_inner(self.layout.browser).height as usize,
            );
        } else if contains(self.layout.output, x, y) {
            self.output_scroll = scrolled(
                self.output_scroll,
                delta,
                self.output_row_count(),
                self.layout.output_rows.height.saturating_sub(1) as usize,
            );
        }
    }

    fn click_at(&mut self, x: u16, y: u16) {
        if let Some(dropdown) = self.layout.dropdown
            && contains(dropdown.rect, x, y)
        {
            self.select_dropdown_value(dropdown.kind, y);
            return;
        }

        self.active_dropdown = None;

        if contains(self.layout.limit_control, x, y) {
            self.active_dropdown = Some(DropdownKind::Limit);
            return;
        }

        if contains(self.layout.order_control, x, y) {
            self.active_dropdown = Some(DropdownKind::Order);
            return;
        }

        let browser_inner = block_inner(self.layout.browser);
        if contains(browser_inner, x, y) {
            self.focus = Focus::Browser;
            let row = usize::from(y.saturating_sub(browser_inner.y));
            let index = self.browser_scroll + row;
            if let Some(table) = self.metadata.tables.get(index).cloned() {
                self.selected_table = index;
                self.load_table_detail(&table);
            }
            return;
        }

        if contains(self.layout.prompt, x, y) {
            self.focus = Focus::Prompt;
        }
    }

    fn select_dropdown_value(&mut self, kind: DropdownKind, y: u16) {
        let Some(dropdown) = self.layout.dropdown else {
            return;
        };
        if y <= dropdown.rect.y || y >= dropdown.rect.y + dropdown.rect.height.saturating_sub(1) {
            self.active_dropdown = None;
            return;
        }
        let row = usize::from(y.saturating_sub(dropdown.rect.y + 1));

        match kind {
            DropdownKind::Limit => {
                let Some(limit) = LIMIT_OPTIONS.get(row).copied() else {
                    self.active_dropdown = None;
                    return;
                };
                self.preview_options.limit = limit;
            }
            DropdownKind::Order => {
                let Some(order) = PreviewOrder::all().get(row).copied() else {
                    self.active_dropdown = None;
                    return;
                };
                self.preview_options.order = order;
            }
        }

        self.active_dropdown = None;
        if let OutputView::Detail(detail) = &self.view {
            let table = detail.table.clone();
            self.load_table_detail(&table);
        }
    }

    fn move_up(&mut self) {
        match self.focus {
            Focus::Browser => {
                self.selected_table = self.selected_table.saturating_sub(1);
                self.ensure_selected_table_visible();
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
                self.ensure_selected_table_visible();
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

    fn submit(&mut self) {
        let command = self.input.trim().to_string();
        if command.is_empty() {
            if let Some(table) = self.metadata.tables.get(self.selected_table).cloned() {
                self.load_table_detail(&table);
            }
            return;
        }

        self.history.push(command.clone());
        self.history_cursor = None;
        self.input.clear();

        if let Some(command) = command::normalize_client_command(&command) {
            self.run_rdbt_command(&command);
        } else {
            self.run_sql(&command);
        }
    }

    fn run_rdbt_command(&mut self, command: &str) {
        match command::parse(command) {
            RdbtCommand::Quit { force: _ } => self.should_quit = true,
            RdbtCommand::Help => self.show_help(),
            RdbtCommand::Safe(SafeModeCommand::Off) => self.set_safe_mode(false),
            RdbtCommand::Safe(SafeModeCommand::On) => self.set_safe_mode(true),
            RdbtCommand::Safe(SafeModeCommand::Toggle) => self.toggle_safe_mode(),
            RdbtCommand::Unsafe => self.set_safe_mode(false),
            RdbtCommand::Refresh => self.refresh_metadata(),
            RdbtCommand::Schemas => {
                self.run_strategy_query(self.strategy.list_schemas_sql(), "schemas");
            }
            RdbtCommand::Tables => {
                self.run_strategy_query(self.strategy.list_tables_sql(), "tables");
            }
            RdbtCommand::Describe(table_name) => {
                if let Some(table) = self.resolve_table(table_name.as_deref()) {
                    let sql = self.strategy.describe_table_sql(&table);
                    self.run_strategy_query(sql, "describe");
                } else {
                    self.status = "usage: :describe schema.table".to_string();
                }
            }
            RdbtCommand::Sample(table_name) => {
                if let Some(table) = self.resolve_table(table_name.as_deref()) {
                    self.sample_table(&table);
                } else {
                    self.status = "usage: :sample schema.table".to_string();
                }
            }
            RdbtCommand::Unknown(name) => {
                if name == "safe" {
                    self.status = "usage: :safe [on|off|toggle]".to_string();
                } else {
                    self.status = format!("unknown rdbt command: :{name}");
                }
            }
            RdbtCommand::Empty => {}
        }
    }

    fn run_sql(&mut self, sql: &str) {
        if self.config.safe_mode {
            let decision = safety::classify(sql);
            if !decision.is_allowed() {
                self.view = OutputView::message(match decision {
                    safety::SafetyDecision::Allow => "allowed".to_string(),
                    safety::SafetyDecision::Deny(reason) => reason,
                });
                self.status = "blocked by safe mode".to_string();
                return;
            }
        }

        let task_id = self.next_task_id();
        self.active_view_task = Some(task_id);
        self.status = "running SQL in background".to_string();
        self.view = OutputView::message("Running SQL...");

        let client = self.client.clone();
        let sql = sql.to_string();
        let returns_rows = safety::returns_rows(&sql);
        let sender = self.task_tx.clone();
        tokio::spawn(async move {
            let result = if returns_rows {
                client.query(&sql).await
            } else {
                client.execute(&sql).await
            }
            .map_err(|error| error.to_string());

            let _ = sender.send(AppEvent::QueryFinished {
                task_id,
                result,
                success_status: "SQL complete".to_string(),
                failure_status: "SQL failed".to_string(),
            });
        });
    }

    fn run_strategy_query(&mut self, sql: String, label: &str) {
        let task_id = self.next_task_id();
        self.active_view_task = Some(task_id);
        self.status = format!("running {label} in background");
        self.view = OutputView::message(format!("Running {label}..."));

        let client = self.client.clone();
        let label = label.to_string();
        let sender = self.task_tx.clone();
        tokio::spawn(async move {
            let result = client.query(&sql).await.map_err(|error| error.to_string());

            let _ = sender.send(AppEvent::QueryFinished {
                task_id,
                result,
                success_status: label,
                failure_status: "SQL failed".to_string(),
            });
        });
    }

    fn sample_table(&mut self, table: &TableRef) {
        let sql = self
            .strategy
            .sample_rows_sql(table, COMMAND_SAMPLE_LIMIT, &SampleOrder::Natural);
        self.run_strategy_query(sql, &format!("sample {}", table.display_name()));
    }

    fn load_table_detail(&mut self, table: &TableRef) {
        // Mouse-driven table inspection is deliberately read-only: it only runs
        // metadata SELECTs and SELECT samples built by the database strategy.
        let task_id = self.next_task_id();
        self.active_view_task = Some(task_id);
        self.status = format!("loading {} in background", table.display_name());
        self.view = OutputView::message(format!("Loading {}...", table.display_name()));
        self.output_scroll = 0;

        let client = self.client.clone();
        let dbms = self.config.dbms;
        let table = table.clone();
        let options = self.preview_options;
        let sender = self.task_tx.clone();
        tokio::spawn(async move {
            let strategy = strategy_for(dbms);
            let result = async {
                let columns = client
                    .query(&strategy.describe_table_sql(&table))
                    .await
                    .map_err(|error| format!("describe failed: {error}"))?;

                let order_column = first_column_name(&columns);
                let order = options.order.to_sample_order(order_column.as_deref());
                let rows_sql = strategy.sample_rows_sql(&table, options.limit, &order);
                let rows = client
                    .query(&rows_sql)
                    .await
                    .map_err(|error| format!("sample failed: {error}"))?;

                Ok(TableDetail {
                    table,
                    columns,
                    rows,
                    order_column,
                    options,
                })
            }
            .await;

            let _ = sender.send(AppEvent::TableDetailLoaded { task_id, result });
        });
    }

    fn refresh_metadata(&mut self) {
        self.metadata.loaded = false;
        self.metadata.schemas.clear();
        self.metadata.tables.clear();
        self.browser_scroll = 0;
        self.output_scroll = 0;
        self.view = OutputView::message("Refreshing metadata...");
        self.load_metadata_if_needed();
    }

    fn load_metadata_if_needed(&mut self) {
        if self.metadata.loaded {
            return;
        }

        let task_id = self.next_task_id();
        self.active_view_task = Some(task_id);
        self.metadata_task = Some(task_id);
        self.status = "loading metadata in background".to_string();

        let client = self.client.clone();
        let dbms = self.config.dbms;
        let sender = self.task_tx.clone();
        tokio::spawn(async move {
            let strategy = strategy_for(dbms);
            let schemas = client
                .query(&strategy.list_schemas_sql())
                .await
                .map_err(|error| error.to_string());
            let tables = client
                .query(&strategy.list_tables_sql())
                .await
                .map_err(|error| error.to_string());

            let _ = sender.send(AppEvent::MetadataLoaded {
                task_id,
                schemas,
                tables,
            });
        });
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

    fn poll_task_events(&mut self) {
        while let Ok(event) = self.task_rx.try_recv() {
            self.handle_app_event(event);
        }
    }

    fn handle_app_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::MetadataLoaded {
                task_id,
                schemas,
                tables,
            } => self.finish_metadata_load(task_id, schemas, tables),
            AppEvent::QueryFinished {
                task_id,
                result,
                success_status,
                failure_status,
            } => {
                if self.active_view_task == Some(task_id) {
                    self.active_view_task = None;
                    self.set_output_result(result, success_status, failure_status);
                }
            }
            AppEvent::TableDetailLoaded { task_id, result } => {
                if self.active_view_task == Some(task_id) {
                    self.active_view_task = None;
                    self.finish_table_detail_load(result);
                }
            }
        }
    }

    fn finish_metadata_load(
        &mut self,
        task_id: u64,
        schemas: TaskResult<QueryOutput>,
        tables: TaskResult<QueryOutput>,
    ) {
        if self.metadata_task != Some(task_id) {
            return;
        }
        self.metadata_task = None;

        match (schemas, tables) {
            (Ok(schemas), Ok(tables)) => {
                self.metadata.schemas = rows_by_column(&schemas, "schema");
                self.metadata.tables = table_refs(&tables);
                self.metadata.loaded = true;
                self.selected_table = self
                    .selected_table
                    .min(self.metadata.tables.len().saturating_sub(1));

                if self.active_view_task == Some(task_id) {
                    self.active_view_task = None;
                    self.view = OutputView::Query(tables);
                    self.output_scroll = 0;
                    self.status = format!(
                        "loaded {} schema(s), {} table(s)",
                        self.metadata.schemas.len(),
                        self.metadata.tables.len()
                    );
                }
            }
            (Err(error), _) | (_, Err(error)) => {
                if self.active_view_task == Some(task_id) {
                    self.active_view_task = None;
                    self.view = OutputView::message(format!("metadata load failed: {error}"));
                    self.output_scroll = 0;
                    self.status = "metadata load failed".to_string();
                }
            }
        }
    }

    fn finish_table_detail_load(&mut self, result: TaskResult<TableDetail>) {
        match result {
            Ok(detail) => {
                let order_label = detail.options.order.label(detail.order_column.as_deref());
                self.status = format!(
                    "{} preview: {} row limit, {} order",
                    detail.table.display_name(),
                    detail.options.limit,
                    order_label
                );
                self.view = OutputView::Detail(detail);
                self.output_scroll = 0;
            }
            Err(error) => {
                let status = if error.starts_with("describe failed") {
                    "describe failed"
                } else if error.starts_with("sample failed") {
                    "sample failed"
                } else {
                    "preview failed"
                };
                self.view = OutputView::message(error);
                self.output_scroll = 0;
                self.status = status.to_string();
            }
        }
    }

    fn set_output_result(
        &mut self,
        result: TaskResult<QueryOutput>,
        success_status: impl Into<String>,
        failure_status: impl Into<String>,
    ) {
        match result {
            Ok(output) => {
                self.view = OutputView::Query(output);
                self.output_scroll = 0;
                self.status = success_status.into();
            }
            Err(error) => {
                self.view = OutputView::message(error);
                self.output_scroll = 0;
                self.status = failure_status.into();
            }
        }
    }

    fn next_task_id(&mut self) -> u64 {
        self.next_task_id = self.next_task_id.saturating_add(1);
        self.next_task_id
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
        self.view = OutputView::Query(menu::help_output());
        self.output_scroll = 0;
        self.status = "help".to_string();
    }

    fn output_row_count(&self) -> usize {
        match &self.view {
            OutputView::Query(output) => output.rows.len(),
            OutputView::Detail(detail) => detail.rows.rows.len(),
        }
    }

    fn clamp_scrolls(&mut self) {
        let browser_height = block_inner(self.layout.browser).height as usize;
        self.browser_scroll = clamp_scroll(
            self.browser_scroll,
            self.metadata.tables.len(),
            browser_height,
        );
        self.clamp_output_scroll(self.output_row_count());
    }

    fn clamp_output_scroll(&mut self, row_count: usize) {
        self.output_scroll = clamp_scroll(
            self.output_scroll,
            row_count,
            self.layout.output_rows.height.saturating_sub(1) as usize,
        );
    }

    fn ensure_selected_table_visible(&mut self) {
        let visible_rows = block_inner(self.layout.browser).height as usize;
        if visible_rows == 0 {
            return;
        }

        if self.selected_table < self.browser_scroll {
            self.browser_scroll = self.selected_table;
        } else if self.selected_table >= self.browser_scroll + visible_rows {
            self.browser_scroll = self.selected_table + 1 - visible_rows;
        }
    }

    fn render(&mut self, frame: &mut Frame) {
        let theme = ThemeKind::from_safe_mode(self.config.safe_mode).theme();

        frame.render_widget(Clear, frame.area());
        frame.render_widget(
            Block::new().style(theme.style(Role::AppBackground)),
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
        self.layout = UiLayout {
            browser,
            output: main,
            prompt: bottom,
            ..UiLayout::default()
        };
        self.clamp_scrolls();

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
            Span::styled(" rdbt ", theme.style(Role::HeaderBrand)),
            Span::raw(" "),
            Span::styled(self.strategy.name(), theme.style(Role::Header)),
            Span::raw(" "),
            Span::styled(mode, theme.style(Role::HeaderMode)),
            Span::raw(menu::top_hint(&self.keymap)),
        ]);
        let block = Block::new()
            .borders(Borders::ALL)
            .border_style(theme.style(Role::Panel))
            .style(theme.style(Role::Header));
        frame.render_widget(Paragraph::new(title).block(block), area);
    }

    fn render_browser(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let items = if self.metadata.tables.is_empty() {
            vec![ListItem::new(Line::from(Span::styled(
                "no tables loaded",
                theme.style(Role::TextMuted),
            )))]
        } else {
            self.metadata
                .tables
                .iter()
                .enumerate()
                .skip(self.browser_scroll)
                .map(|(index, table)| {
                    let style = if index == self.selected_table {
                        theme.style(Role::ListItemSelected)
                    } else {
                        theme.style(Role::ListItem)
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(table.schema.clone(), theme.selector("browser.schema")),
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
            .border_style(if self.focus == Focus::Browser {
                theme.style(Role::PanelFocused)
            } else {
                theme.style(Role::Panel)
            })
            .style(theme.style(Role::Panel));
        let list = List::new(items)
            .block(block)
            .style(theme.style(Role::Panel));

        frame.render_widget(list, area);
    }

    fn render_output(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        self.layout.limit_control = Rect::new(0, 0, 0, 0);
        self.layout.order_control = Rect::new(0, 0, 0, 0);
        self.layout.dropdown = None;

        let view = self.view.clone();
        match view {
            OutputView::Query(output) => {
                self.layout.output_rows = block_inner(area);
                self.clamp_output_scroll(output.rows.len());
                self.render_table_output(frame, area, theme, &output, "Output", self.output_scroll);
            }
            OutputView::Detail(detail) => self.render_table_detail(frame, area, theme, &detail),
        }
    }

    fn render_table_detail(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        theme: &Theme,
        detail: &TableDetail,
    ) {
        let column_height =
            (detail.columns.rows.len() as u16 + 3).clamp(6, area.height.saturating_sub(8).max(6));
        let [controls, columns, rows] = Layout::vertical([
            Constraint::Length(4),
            Constraint::Length(column_height),
            Constraint::Min(5),
        ])
        .areas(area);

        self.render_preview_controls(frame, controls, theme, detail);
        self.render_table_output(
            frame,
            columns,
            theme,
            &detail.columns,
            &format!("Columns - {}", detail.table.display_name()),
            0,
        );

        self.layout.output_rows = block_inner(rows);
        self.clamp_output_scroll(detail.rows.rows.len());
        self.render_table_output(
            frame,
            rows,
            theme,
            &detail.rows,
            &format!(
                "Rows - limit {} - {}",
                detail.options.limit,
                detail.options.order.label(detail.order_column.as_deref())
            ),
            self.output_scroll,
        );

        self.render_dropdown(frame, theme, detail.order_column.as_deref());
    }

    fn render_preview_controls(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        theme: &Theme,
        detail: &TableDetail,
    ) {
        let block = Block::new()
            .title("Table Preview")
            .borders(Borders::ALL)
            .border_style(theme.style(Role::Panel))
            .style(theme.style(Role::Panel));
        frame.render_widget(block, area);

        let inner = block_inner(area);
        if inner.height == 0 {
            return;
        }

        let limit_width = 18.min(inner.width);
        let limit = Rect::new(inner.x, inner.y, limit_width, inner.height.min(3));
        let order_x = limit.x.saturating_add(limit.width).saturating_add(1);
        let order_width = inner
            .x
            .saturating_add(inner.width)
            .saturating_sub(order_x)
            .min(40);
        let order = Rect::new(order_x, inner.y, order_width, inner.height.min(3));

        self.layout.limit_control = limit;
        self.layout.order_control = order;

        self.render_select(
            frame,
            limit,
            theme,
            "Limit",
            &detail.options.limit.to_string(),
            self.active_dropdown == Some(DropdownKind::Limit),
        );
        self.render_select(
            frame,
            order,
            theme,
            "Order",
            &detail.options.order.label(detail.order_column.as_deref()),
            self.active_dropdown == Some(DropdownKind::Order),
        );
    }

    fn render_select(
        &self,
        frame: &mut Frame,
        area: Rect,
        theme: &Theme,
        title: &'static str,
        value: &str,
        active: bool,
    ) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let border = if active {
            theme.selector("select.active.border")
        } else {
            theme.selector("select.border")
        };
        let block = Block::new()
            .title(title)
            .borders(Borders::ALL)
            .border_style(border)
            .style(if active {
                theme.style(Role::InputFocused)
            } else {
                theme.style(Role::Input)
            });
        let value = format!(
            "{} v",
            truncate(value, area.width.saturating_sub(4) as usize)
        );
        frame.render_widget(
            Paragraph::new(value)
                .block(block)
                .style(theme.style(Role::Input)),
            area,
        );
    }

    fn render_dropdown(&mut self, frame: &mut Frame, theme: &Theme, order_column: Option<&str>) {
        let Some(kind) = self.active_dropdown else {
            return;
        };

        let (control, items) = match kind {
            DropdownKind::Limit => (
                self.layout.limit_control,
                LIMIT_OPTIONS
                    .iter()
                    .map(|limit| limit.to_string())
                    .collect::<Vec<_>>(),
            ),
            DropdownKind::Order => (
                self.layout.order_control,
                PreviewOrder::all()
                    .iter()
                    .map(|order| order.label(order_column))
                    .collect::<Vec<_>>(),
            ),
        };

        if control.width == 0 || control.height == 0 {
            return;
        }

        let available_height = frame
            .area()
            .height
            .saturating_sub(control.y + control.height);
        let height = (items.len() as u16 + 2).min(available_height).max(1);
        let rect = Rect::new(control.x, control.y + control.height, control.width, height);
        self.layout.dropdown = Some(DropdownArea { kind, rect });

        let list =
            List::new(items.into_iter().map(|item| {
                ListItem::new(Line::from(Span::styled(item, theme.style(Role::Text))))
            }))
            .block(
                Block::new()
                    .borders(Borders::ALL)
                    .border_style(theme.selector("dropdown.border"))
                    .style(theme.style(Role::Panel)),
            );
        frame.render_widget(Clear, rect);
        frame.render_widget(list, rect);
    }

    fn render_table_output(
        &self,
        frame: &mut Frame,
        area: Rect,
        theme: &Theme,
        output: &QueryOutput,
        title: &str,
        row_offset: usize,
    ) {
        if output.columns.is_empty() {
            let block = Block::new()
                .title(title)
                .borders(Borders::ALL)
                .border_style(theme.style(Role::Panel))
                .style(theme.style(Role::Panel));
            let paragraph = Paragraph::new(output.message.clone())
                .block(block)
                .style(theme.style(Role::Text))
                .wrap(Wrap { trim: false });
            frame.render_widget(paragraph, area);
            return;
        }

        let widths = table_widths(output, area.width.saturating_sub(4));
        let constraints = widths
            .iter()
            .copied()
            .map(Constraint::Length)
            .collect::<Vec<_>>();
        let header = Row::new(
            output
                .columns
                .iter()
                .map(|column| Cell::from(column.clone()).style(theme.style(Role::TableHeader))),
        );
        let rows =
            output.rows.iter().skip(row_offset).map(|row| {
                Row::new(row.iter().map(|value| {
                    Cell::from(truncate(value, 64)).style(theme.style(Role::TableCell))
                }))
            });
        let table = Table::new(rows, constraints)
            .header(header)
            .block(
                Block::new()
                    .title(format!("{title} - {}", output.message))
                    .borders(Borders::ALL)
                    .border_style(theme.style(Role::Panel))
                    .style(theme.style(Role::Panel)),
            )
            .column_spacing(1)
            .style(theme.style(Role::Panel));

        frame.render_widget(table, area);
    }

    fn render_prompt(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let status_style = if self.config.safe_mode {
            theme.style(Role::Status)
        } else {
            theme.style(Role::StatusDanger)
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
                theme.style(Role::PromptPrefix),
            ),
            Span::styled(self.input.clone(), theme.style(Role::PromptInput)),
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
                theme.style(Role::Badge),
            ),
        ]);
        let text = vec![prompt, footer];
        let block = Block::new()
            .title(title)
            .borders(Borders::ALL)
            .border_style(if self.focus == Focus::Prompt {
                theme.style(Role::PanelFocused)
            } else {
                theme.style(Role::Panel)
            })
            .style(theme.style(Role::Panel));
        frame.render_widget(
            Paragraph::new(text)
                .block(block)
                .style(theme.style(Role::Footer)),
            area,
        );
    }
}

fn contains(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

fn block_inner(area: Rect) -> Rect {
    Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(1),
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    )
}

fn scrolled(current: usize, delta: isize, row_count: usize, visible_rows: usize) -> usize {
    let next = if delta.is_negative() {
        current.saturating_sub(delta.unsigned_abs())
    } else {
        current.saturating_add(delta as usize)
    };
    clamp_scroll(next, row_count, visible_rows)
}

fn clamp_scroll(current: usize, row_count: usize, visible_rows: usize) -> usize {
    current.min(row_count.saturating_sub(visible_rows.max(1)))
}

fn first_column_name(output: &QueryOutput) -> Option<String> {
    let index = column_index(output, "column")?;
    output.rows.first()?.get(index).cloned()
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
    use ratatui::layout::Rect;

    use super::{PreviewOrder, block_inner, clamp_scroll, contains};

    #[test]
    fn clamps_scroll_to_available_rows() {
        assert_eq!(clamp_scroll(99, 10, 4), 6);
        assert_eq!(clamp_scroll(3, 10, 4), 3);
        assert_eq!(clamp_scroll(3, 2, 4), 0);
    }

    #[test]
    fn detects_rect_hits_with_exclusive_end() {
        let rect = Rect::new(2, 3, 4, 5);
        assert!(contains(rect, 2, 3));
        assert!(contains(rect, 5, 7));
        assert!(!contains(rect, 6, 7));
        assert!(!contains(rect, 5, 8));
    }

    #[test]
    fn block_inner_saturates_small_rects() {
        assert_eq!(block_inner(Rect::new(0, 0, 1, 1)), Rect::new(1, 1, 0, 0));
    }

    #[test]
    fn preview_order_without_column_is_natural() {
        assert_eq!(
            PreviewOrder::FirstColumnDesc.to_sample_order(None),
            crate::database::SampleOrder::Natural
        );
    }
}
