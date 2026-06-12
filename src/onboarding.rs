use std::{io::stdout, time::Duration};

use color_eyre::Result;
use crossterm::{
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers,
    },
    execute,
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::{
    args::{Config, Dbms, build_url, default_port},
    tui::{
        style::Role,
        theme::{Theme, ThemeKind},
    },
};

#[derive(Debug, Clone)]
pub struct OnboardingDefaults {
    pub dbms: Dbms,
    pub host: String,
    pub port: Option<u16>,
    pub user: String,
    pub password: String,
    pub database: Option<String>,
    pub safe_mode: bool,
}

pub fn run_onboarding(defaults: OnboardingDefaults) -> Result<Option<Config>> {
    let mut terminal = ratatui::init();
    if let Err(error) = execute!(stdout(), EnableBracketedPaste) {
        ratatui::restore();
        return Err(error.into());
    }

    let result = OnboardingApp::new(defaults).run(&mut terminal);
    let disable_result = execute!(stdout(), DisableBracketedPaste);
    ratatui::restore();
    disable_result?;
    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Connector,
    Host,
    Port,
    User,
    Password,
    Connect,
}

impl Field {
    fn next(self) -> Self {
        Self::from_index(self.index() + 1)
    }

    fn previous(self) -> Self {
        Self::from_index(self.index().saturating_sub(1))
    }

    fn index(self) -> usize {
        match self {
            Self::Connector => 0,
            Self::Host => 1,
            Self::Port => 2,
            Self::User => 3,
            Self::Password => 4,
            Self::Connect => 5,
        }
    }

    fn from_index(index: usize) -> Self {
        match index.min(5) {
            0 => Self::Connector,
            1 => Self::Host,
            2 => Self::Port,
            3 => Self::User,
            4 => Self::Password,
            _ => Self::Connect,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostChoice {
    Localhost,
    Loopback,
    Manual,
}

impl HostChoice {
    fn from_host(host: &str) -> Self {
        match host {
            "localhost" => Self::Localhost,
            "127.0.0.1" => Self::Loopback,
            _ => Self::Manual,
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Localhost => Self::Loopback,
            Self::Loopback => Self::Manual,
            Self::Manual => Self::Localhost,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::Localhost => Self::Manual,
            Self::Loopback => Self::Localhost,
            Self::Manual => Self::Loopback,
        }
    }

    fn value(self, manual_host: &str) -> String {
        match self {
            Self::Localhost => "localhost".to_string(),
            Self::Loopback => "127.0.0.1".to_string(),
            Self::Manual => manual_host.trim().to_string(),
        }
    }
}

#[derive(Debug)]
struct OnboardingApp {
    dbms: Dbms,
    host_choice: HostChoice,
    manual_host: String,
    port: String,
    port_touched: bool,
    user: String,
    password: String,
    database: Option<String>,
    safe_mode: bool,
    focus: Field,
    command_mode: bool,
    command_input: String,
    status: String,
    result: Option<Option<Config>>,
}

impl OnboardingApp {
    fn new(defaults: OnboardingDefaults) -> Self {
        let host_choice = HostChoice::from_host(&defaults.host);
        let manual_host = if host_choice == HostChoice::Manual {
            defaults.host
        } else {
            String::new()
        };
        let port = defaults
            .port
            .unwrap_or_else(|| default_port(defaults.dbms))
            .to_string();
        Self {
            dbms: defaults.dbms,
            host_choice,
            manual_host,
            port,
            port_touched: defaults.port.is_some(),
            user: defaults.user,
            password: defaults.password,
            database: defaults.database,
            safe_mode: defaults.safe_mode,
            focus: Field::Connector,
            command_mode: false,
            command_input: String::new(),
            status: "choose connector and connection settings".to_string(),
            result: None,
        }
    }

    fn run(&mut self, terminal: &mut ratatui::DefaultTerminal) -> Result<Option<Config>> {
        while self.result.is_none() {
            terminal.draw(|frame| self.render(frame))?;

            if event::poll(Duration::from_millis(100))? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => self.handle_key(key),
                    Event::Paste(text) => self.append_text(&text),
                    _ => {}
                }
            }
        }

        Ok(self.result.take().flatten())
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if self.command_mode {
            self.handle_command_key(key);
            return;
        }

        match key.code {
            KeyCode::Char(':') if text_input_modifiers(key.modifiers) => self.enter_command_mode(),
            KeyCode::Esc => self.status = "use :q to quit".to_string(),
            KeyCode::Tab | KeyCode::Down => self.focus = self.focus.next(),
            KeyCode::BackTab | KeyCode::Up => self.focus = self.focus.previous(),
            KeyCode::Left => self.choose_previous(),
            KeyCode::Right => self.choose_next(),
            KeyCode::Enter => self.enter_or_advance(),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Char(character) if text_input_modifiers(key.modifiers) => {
                self.append_char(character)
            }
            _ => {}
        }
    }

    fn handle_command_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.cancel_command(),
            KeyCode::Enter => self.submit_command(),
            KeyCode::Backspace => {
                self.command_input.pop();
            }
            KeyCode::Char(character) if text_input_modifiers(key.modifiers) => {
                self.command_input.push(character);
            }
            _ => {}
        }
    }

    fn enter_command_mode(&mut self) {
        self.command_mode = true;
        self.command_input.clear();
        self.command_input.push(':');
        self.status = "command mode".to_string();
    }

    fn cancel_command(&mut self) {
        self.command_mode = false;
        self.command_input.clear();
        self.status = "command canceled".to_string();
    }

    fn submit_command(&mut self) {
        match self.command_input.trim() {
            ":q" | ":quit" | ":exit" | ":q!" | ":quit!" | ":exit!" => {
                self.result = Some(None);
            }
            ":help" | ":?" => {
                self.status =
                    "Tab moves fields, Left/Right changes selectors, Enter connects".to_string();
            }
            "" | ":" => self.status = "empty command".to_string(),
            command => self.status = format!("unknown command: {command}"),
        }

        self.command_mode = false;
        self.command_input.clear();
    }

    fn choose_next(&mut self) {
        match self.focus {
            Field::Connector => self.set_dbms(match self.dbms {
                Dbms::Postgres => Dbms::Mysql,
                Dbms::Mysql => Dbms::Postgres,
            }),
            Field::Host => self.host_choice = self.host_choice.next(),
            _ => {}
        }
    }

    fn choose_previous(&mut self) {
        match self.focus {
            Field::Connector => self.set_dbms(match self.dbms {
                Dbms::Postgres => Dbms::Mysql,
                Dbms::Mysql => Dbms::Postgres,
            }),
            Field::Host => self.host_choice = self.host_choice.previous(),
            _ => {}
        }
    }

    fn set_dbms(&mut self, dbms: Dbms) {
        self.dbms = dbms;
        if !self.port_touched {
            self.port = default_port(dbms).to_string();
        }
    }

    fn enter_or_advance(&mut self) {
        if self.focus == Field::Connect {
            self.submit();
        } else {
            self.focus = self.focus.next();
        }
    }

    fn submit(&mut self) {
        match self.to_config() {
            Ok(config) => self.result = Some(Some(config)),
            Err(message) => self.status = message,
        }
    }

    fn to_config(&self) -> std::result::Result<Config, String> {
        let host = self.host_choice.value(&self.manual_host);
        if host.is_empty() {
            return Err("host is required".to_string());
        }
        if self.user.trim().is_empty() {
            return Err("user is required".to_string());
        }
        let port = self
            .port
            .parse::<u16>()
            .map_err(|_| "port must be a valid TCP port".to_string())?;
        let password = if self.password.is_empty() {
            None
        } else {
            Some(self.password.clone())
        };
        let database = self.database.as_deref();
        let url = build_url(
            self.dbms,
            &host,
            port,
            Some(self.user.trim()),
            password,
            database,
        )
        .map_err(|error| error.to_string())?;

        Ok(Config {
            dbms: self.dbms,
            url,
            database: self.database.clone(),
            safe_mode: self.safe_mode,
        })
    }

    fn backspace(&mut self) {
        match self.focus {
            Field::Host if self.host_choice == HostChoice::Manual => {
                self.manual_host.pop();
            }
            Field::Port => {
                self.port.pop();
                self.port_touched = true;
            }
            Field::User => {
                self.user.pop();
            }
            Field::Password => {
                self.password.pop();
            }
            _ => {}
        }
    }

    fn append_char(&mut self, character: char) {
        if character.is_control() {
            return;
        }

        match self.focus {
            Field::Host if self.host_choice == HostChoice::Manual => {
                self.manual_host.push(character)
            }
            Field::Port if character.is_ascii_digit() => {
                self.port.push(character);
                self.port_touched = true;
            }
            Field::User => self.user.push(character),
            Field::Password => self.password.push(character),
            _ => {}
        }
    }

    fn append_text(&mut self, text: &str) {
        for character in text.chars() {
            if character != '\n' && character != '\r' {
                self.append_char(character);
            }
        }
    }

    fn render(&self, frame: &mut Frame<'_>) {
        let theme = ThemeKind::Safe.theme();
        frame.render_widget(Clear, frame.area());
        frame.render_widget(
            Block::new().style(theme.style(Role::AppBackground)),
            frame.area(),
        );

        let area = centered_rect(frame.area(), 70, 72);
        let block = Block::new()
            .title("Connection")
            .borders(Borders::ALL)
            .border_style(theme.style(Role::PanelFocused))
            .style(theme.style(Role::Panel));
        frame.render_widget(block, area);

        let inner = block_inner(area);
        let [header, form, footer] = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(10),
                Constraint::Length(4),
            ])
            .areas(inner);

        self.render_header(frame, header, &theme);
        self.render_form(frame, form, &theme);
        self.render_footer(frame, footer, &theme);
    }

    fn render_header(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let lines = vec![
            Line::from(vec![
                Span::styled(" rdbt ", theme.style(Role::HeaderBrand)),
                Span::raw(" "),
                Span::styled("database connection", theme.style(Role::Header)),
            ]),
            Line::from(Span::styled(
                "Schema/database selection happens in the browser after connect.",
                theme.style(Role::TextMuted),
            )),
        ];
        frame.render_widget(
            Paragraph::new(lines)
                .style(theme.style(Role::Header))
                .wrap(Wrap { trim: false }),
            area,
        );
    }

    fn render_form(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let lines = vec![
            self.connector_line(theme),
            self.host_line(theme),
            self.input_line("Port", &self.port, Field::Port, theme, false),
            self.input_line("User", &self.user, Field::User, theme, false),
            self.input_line(
                "Password",
                &masked(&self.password),
                Field::Password,
                theme,
                true,
            ),
            Line::from(""),
            self.connect_line(theme),
        ];
        frame.render_widget(
            Paragraph::new(lines)
                .style(theme.style(Role::Panel))
                .wrap(Wrap { trim: false }),
            area,
        );
    }

    fn connector_line(&self, theme: &Theme) -> Line<'static> {
        Line::from(vec![
            self.label("Connector", theme),
            self.choice("postgres", self.dbms == Dbms::Postgres, theme),
            Span::raw("  "),
            self.choice("mysql", self.dbms == Dbms::Mysql, theme),
        ])
    }

    fn host_line(&self, theme: &Theme) -> Line<'static> {
        Line::from(vec![
            self.label("Host", theme),
            self.choice(
                "localhost",
                self.host_choice == HostChoice::Localhost,
                theme,
            ),
            Span::raw("  "),
            self.choice("127.0.0.1", self.host_choice == HostChoice::Loopback, theme),
            Span::raw("  "),
            self.choice(
                &format!("manual: {}", self.manual_host),
                self.host_choice == HostChoice::Manual,
                theme,
            ),
        ])
    }

    fn input_line(
        &self,
        label: &'static str,
        value: &str,
        field: Field,
        theme: &Theme,
        secret: bool,
    ) -> Line<'static> {
        let value = if secret && value.is_empty() {
            "(optional)".to_string()
        } else {
            value.to_string()
        };
        let style = if self.focus == field {
            theme.style(Role::InputFocused)
        } else {
            theme.style(Role::Input)
        };
        Line::from(vec![self.label(label, theme), Span::styled(value, style)])
    }

    fn connect_line(&self, theme: &Theme) -> Line<'static> {
        let style = if self.focus == Field::Connect {
            theme.style(Role::HeaderBrand)
        } else {
            theme.style(Role::Badge)
        };
        Line::from(vec![
            self.label("", theme),
            Span::styled(" Connect ", style),
        ])
    }

    fn render_footer(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let prompt = if self.command_mode {
            Line::from(vec![
                Span::styled("cmd ", theme.style(Role::PromptPrefix)),
                Span::styled(self.command_input.clone(), theme.style(Role::PromptInput)),
            ])
        } else {
            Line::from(vec![
                Span::styled(self.status.clone(), theme.style(Role::Status)),
                Span::raw("  "),
                Span::styled(":q quit", theme.style(Role::Badge)),
            ])
        };
        let keys = Line::from(Span::styled(
            "Tab/Shift-Tab fields  Left/Right select  Enter next/connect  paste supported",
            theme.style(Role::TextMuted),
        ));
        frame.render_widget(
            Paragraph::new(vec![prompt, keys]).style(theme.style(Role::Footer)),
            area,
        );
    }

    fn label(&self, value: &'static str, theme: &Theme) -> Span<'static> {
        Span::styled(format!("{value:<12}"), theme.style(Role::TextMuted))
    }

    fn choice(&self, value: &str, selected: bool, theme: &Theme) -> Span<'static> {
        let value = if selected {
            format!("[{value}]")
        } else {
            format!(" {value} ")
        };
        let style = if selected {
            theme.style(Role::Badge)
        } else {
            theme.style(Role::Text)
        };
        Span::styled(value, style)
    }
}

fn text_input_modifiers(mut modifiers: KeyModifiers) -> bool {
    modifiers.remove(KeyModifiers::SHIFT);
    modifiers.is_empty()
}

fn masked(value: &str) -> String {
    "*".repeat(value.chars().count())
}

fn centered_rect(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1]);
    horizontal[1]
}

fn block_inner(area: Rect) -> Rect {
    Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(1),
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    )
}

#[cfg(test)]
mod tests {
    use super::{HostChoice, OnboardingApp, OnboardingDefaults};
    use crate::args::Dbms;

    fn defaults() -> OnboardingDefaults {
        OnboardingDefaults {
            dbms: Dbms::Postgres,
            host: "localhost".to_string(),
            port: None,
            user: "alice".to_string(),
            password: "secret".to_string(),
            database: None,
            safe_mode: true,
        }
    }

    #[test]
    fn connector_selection_updates_default_port() {
        let mut app = OnboardingApp::new(defaults());
        assert_eq!(app.port, "5432");

        app.set_dbms(Dbms::Mysql);

        assert_eq!(app.port, "3306");
    }

    #[test]
    fn explicit_port_is_not_replaced_by_connector_selection() {
        let mut defaults = defaults();
        defaults.port = Some(15432);
        let mut app = OnboardingApp::new(defaults);

        app.set_dbms(Dbms::Mysql);

        assert_eq!(app.port, "15432");
    }

    #[test]
    fn host_defaults_to_dropdown_choices() {
        assert_eq!(HostChoice::from_host("localhost"), HostChoice::Localhost);
        assert_eq!(HostChoice::from_host("127.0.0.1"), HostChoice::Loopback);
        assert_eq!(HostChoice::from_host("db.internal"), HostChoice::Manual);
    }

    #[test]
    fn builds_connection_url_without_prompted_database() {
        let app = OnboardingApp::new(defaults());
        let config = app.to_config().expect("valid defaults should build config");

        assert_eq!(config.url, "postgres://alice:secret@localhost:5432/");
        assert_eq!(config.database, None);
    }
}
