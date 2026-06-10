#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SafetyDecision {
    Allow,
    Deny(String),
}

impl SafetyDecision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allow)
    }
}

pub fn classify(sql: &str) -> SafetyDecision {
    let statements = normalized_statements(sql);
    if statements.is_empty() {
        return SafetyDecision::Deny("empty SQL".to_string());
    }

    for statement in statements {
        let first = first_keyword(&statement);
        if first != Some("select") && first != Some("with") {
            return SafetyDecision::Deny("safe mode only allows SELECT queries".to_string());
        }

        if let Some(keyword) = first_forbidden_keyword(&statement) {
            return SafetyDecision::Deny(format!("safe mode blocked keyword `{keyword}`"));
        }
    }

    SafetyDecision::Allow
}

pub fn returns_rows(sql: &str) -> bool {
    normalized_statements(sql)
        .first()
        .and_then(|statement| first_keyword(statement))
        .is_some_and(|keyword| {
            matches!(
                keyword,
                "select" | "with" | "show" | "describe" | "desc" | "explain"
            )
        })
}

fn normalized_statements(sql: &str) -> Vec<String> {
    strip_comments_and_literals(sql)
        .split(';')
        .map(str::trim)
        .filter(|statement| !statement.is_empty())
        .map(str::to_string)
        .collect()
}

fn first_keyword(statement: &str) -> Option<&str> {
    statement
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .find(|token| !token.is_empty())
}

fn first_forbidden_keyword(statement: &str) -> Option<&'static str> {
    const FORBIDDEN: &[&str] = &[
        "alter", "analyze", "call", "copy", "create", "delete", "drop", "execute", "grant",
        "insert", "load", "merge", "reindex", "replace", "revoke", "set", "truncate", "update",
        "vacuum",
    ];

    statement
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .find_map(|token| {
            let token = token.trim();
            FORBIDDEN.iter().copied().find(|keyword| *keyword == token)
        })
}

fn strip_comments_and_literals(sql: &str) -> String {
    #[derive(Clone, Copy)]
    enum State {
        Normal,
        LineComment,
        BlockComment,
        SingleQuote,
        DoubleQuote,
        Backtick,
    }

    let mut state = State::Normal;
    let mut output = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();

    while let Some(ch) = chars.next() {
        match state {
            State::Normal => match ch {
                '-' if chars.peek() == Some(&'-') => {
                    chars.next();
                    state = State::LineComment;
                    output.push(' ');
                }
                '/' if chars.peek() == Some(&'*') => {
                    chars.next();
                    state = State::BlockComment;
                    output.push(' ');
                }
                '\'' => {
                    state = State::SingleQuote;
                    output.push(' ');
                }
                '"' => {
                    state = State::DoubleQuote;
                    output.push(' ');
                }
                '`' => {
                    state = State::Backtick;
                    output.push(' ');
                }
                _ => output.push(ch.to_ascii_lowercase()),
            },
            State::LineComment => {
                if ch == '\n' {
                    state = State::Normal;
                    output.push('\n');
                }
            }
            State::BlockComment => {
                if ch == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    state = State::Normal;
                    output.push(' ');
                }
            }
            State::SingleQuote => {
                if ch == '\'' {
                    if chars.peek() == Some(&'\'') {
                        chars.next();
                    } else {
                        state = State::Normal;
                    }
                }
            }
            State::DoubleQuote => {
                if ch == '"' {
                    state = State::Normal;
                }
            }
            State::Backtick => {
                if ch == '`' {
                    state = State::Normal;
                }
            }
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::{SafetyDecision, classify};

    #[test]
    fn allows_select() {
        assert_eq!(classify("select * from users"), SafetyDecision::Allow);
    }

    #[test]
    fn blocks_writes() {
        assert!(!classify("update users set admin = true").is_allowed());
        assert!(!classify("select 1; delete from users").is_allowed());
    }

    #[test]
    fn ignores_literals_and_comments() {
        assert_eq!(
            classify("select 'delete from users' -- update\nfrom audit_log"),
            SafetyDecision::Allow
        );
    }

    #[test]
    fn blocks_write_ctes() {
        assert!(
            !classify("with x as (delete from users returning *) select * from x").is_allowed()
        );
    }
}
