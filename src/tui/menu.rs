use crate::database::QueryOutput;

use super::keymap::Keymap;

pub fn top_hint(keymap: &Keymap) -> String {
    let labels = keymap
        .bindings()
        .iter()
        .filter(|binding| matches!(binding.label, "F2" | "F5" | "Tab"))
        .map(|binding| format!("{} {}", binding.label, binding.description))
        .collect::<Vec<_>>()
        .join("  ");

    format!("  {labels}  :q quit")
}

pub fn help_output() -> QueryOutput {
    QueryOutput {
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
            vec![":q, :quit, :q!".to_string(), "exit rdbt".to_string()],
        ],
        message: "rdbt commands".to_string(),
    }
}
