#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RdbtCommand {
    Quit { force: bool },
    Help,
    Safe(SafeModeCommand),
    Unsafe,
    Refresh,
    Schemas,
    Tables,
    Describe(Option<String>),
    Sample(Option<String>),
    Unknown(String),
    Empty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafeModeCommand {
    On,
    Off,
    Toggle,
}

pub fn parse(command: &str) -> RdbtCommand {
    let trimmed = command.trim();
    if trimmed.is_empty() || trimmed == ":" {
        return RdbtCommand::Empty;
    }

    let Some(command) = trimmed.strip_prefix(':') else {
        return RdbtCommand::Unknown(trimmed.to_string());
    };

    let mut parts = command.split_whitespace();
    let Some(name) = parts.next() else {
        return RdbtCommand::Empty;
    };

    match name {
        "q" | "quit" | "exit" => RdbtCommand::Quit { force: false },
        "q!" | "quit!" | "exit!" => RdbtCommand::Quit { force: true },
        "help" | "?" => RdbtCommand::Help,
        "safe" => match parts.next() {
            Some("on") => RdbtCommand::Safe(SafeModeCommand::On),
            Some("off") => RdbtCommand::Safe(SafeModeCommand::Off),
            Some("toggle") | None => RdbtCommand::Safe(SafeModeCommand::Toggle),
            Some(_) => RdbtCommand::Unknown("safe".to_string()),
        },
        "unsafe" => RdbtCommand::Unsafe,
        "refresh" => RdbtCommand::Refresh,
        "schemas" => RdbtCommand::Schemas,
        "tables" => RdbtCommand::Tables,
        "describe" | "desc" => RdbtCommand::Describe(parts.next().map(ToString::to_string)),
        "sample" | "select" => RdbtCommand::Sample(parts.next().map(ToString::to_string)),
        _ => RdbtCommand::Unknown(name.to_string()),
    }
}

pub fn normalize_client_command(command: &str) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::{RdbtCommand, SafeModeCommand, normalize_client_command, parse};

    #[test]
    fn parses_quit_aliases() {
        assert_eq!(parse(":q"), RdbtCommand::Quit { force: false });
        assert_eq!(parse(":quit!"), RdbtCommand::Quit { force: true });
        assert_eq!(normalize_client_command("\\q"), Some(":quit".to_string()));
    }

    #[test]
    fn parses_safe_mode_commands() {
        assert_eq!(parse(":safe"), RdbtCommand::Safe(SafeModeCommand::Toggle));
        assert_eq!(parse(":safe on"), RdbtCommand::Safe(SafeModeCommand::On));
        assert_eq!(parse(":unsafe"), RdbtCommand::Unsafe);
    }

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
