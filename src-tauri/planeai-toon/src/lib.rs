//! Minimal TOON encoder for AXI output.
//!
//! Implements the subset of TOON v3.3 needed for agent-facing CLI output:
//! - Object fields (key: value)
//! - Tabular arrays (key[N]{fields}: rows)
//! - Primitive arrays (key[N]: values)
//! - Nested objects (indented)
//! - Help arrays (as primitive arrays)

/// A TOON value that can be rendered.
pub enum Value {
    /// A string or primitive value.
    Str(String),
    /// An integer.
    Int(i64),
    /// A boolean.
    Bool(bool),
    /// Null.
    Null,
    /// A nested object (ordered fields).
    Object(Vec<Field>),
    /// A tabular array of rows, each row being comma-delimited values.
    Table {
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    /// A primitive array (inline comma-delimited values).
    Array(Vec<String>),
    /// A list array (multiline, one item per line with `- ` prefix).
    List(Vec<String>),
}

/// A key-value field in a TOON document.
pub struct Field {
    pub key: String,
    pub value: Value,
}

/// Render a TOON document from a list of fields.
pub fn render(fields: &[Field]) -> String {
    let mut out = String::new();
    render_fields(&mut out, fields, 0);
    out
}

fn render_fields(out: &mut String, fields: &[Field], indent: usize) {
    for field in fields {
        render_field(out, field, indent);
    }
}

fn render_field(out: &mut String, field: &Field, indent: usize) {
    let pad = " ".repeat(indent);
    match &field.value {
        Value::Str(s) => {
            out.push_str(&pad);
            out.push_str(&field.key);
            out.push_str(": ");
            out.push_str(&quote_if_needed(s));
            out.push('\n');
        }
        Value::Int(n) => {
            out.push_str(&pad);
            out.push_str(&field.key);
            out.push_str(": ");
            out.push_str(&n.to_string());
            out.push('\n');
        }
        Value::Bool(b) => {
            out.push_str(&pad);
            out.push_str(&field.key);
            out.push_str(": ");
            out.push_str(if *b { "true" } else { "false" });
            out.push('\n');
        }
        Value::Null => {
            out.push_str(&pad);
            out.push_str(&field.key);
            out.push_str(": null");
            out.push('\n');
        }
        Value::Object(inner) => {
            out.push_str(&pad);
            out.push_str(&field.key);
            out.push_str(":\n");
            render_fields(out, inner, indent + 2);
        }
        Value::Table { columns, rows } => {
            out.push_str(&pad);
            out.push_str(&field.key);
            out.push_str(&format!("[{}]{{{}}}:\n", rows.len(), columns.join(",")));
            for row in rows {
                out.push_str(&pad);
                out.push_str("  ");
                out.push_str(
                    &row.iter()
                        .map(|v| quote_table_cell(v))
                        .collect::<Vec<_>>()
                        .join(","),
                );
                out.push('\n');
            }
        }
        Value::Array(items) => {
            out.push_str(&pad);
            out.push_str(&field.key);
            out.push_str(&format!("[{}]: ", items.len()));
            out.push_str(
                &items
                    .iter()
                    .map(|v| quote_if_needed(v))
                    .collect::<Vec<_>>()
                    .join(","),
            );
            out.push('\n');
        }
        Value::List(items) => {
            out.push_str(&pad);
            out.push_str(&field.key);
            out.push_str(&format!("[{}]:\n", items.len()));
            for item in items {
                out.push_str(&pad);
                out.push_str("  - ");
                out.push_str(&quote_if_needed(item));
                out.push('\n');
            }
        }
    }
}

fn quote_if_needed(s: &str) -> String {
    if s.is_empty()
        || s == "true"
        || s == "false"
        || s == "null"
        || s.starts_with(' ')
        || s.ends_with(' ')
        || s.contains(',')
        || s.contains(':')
        || s.contains('"')
        || s.contains('\\')
        || s.contains('[')
        || s.contains(']')
        || s.contains('{')
        || s.contains('}')
        || s.starts_with('-')
        || looks_like_number(s)
        || s.chars().any(|c| c.is_control())
    {
        format!("\"{}\"", escape(s))
    } else {
        s.to_string()
    }
}

/// Quote a table cell value. In tabular arrays, empty strings remain empty
/// (positionally delimited), and numeric-looking values stay unquoted
/// (they ARE data values, not ambiguous strings).
fn quote_table_cell(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    if s.contains(',')
        || s.contains('"')
        || s.contains('\\')
        || s.starts_with(' ')
        || s.ends_with(' ')
        || s.chars().any(|c| c.is_control())
    {
        format!("\"{}\"", escape(s))
    } else {
        s.to_string()
    }
}

fn looks_like_number(s: &str) -> bool {
    s.parse::<f64>().is_ok()
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                out.push_str(&format!("\\u{:04X}", c as u32));
            }
            _ => out.push(c),
        }
    }
    out
}

/// Helper to build a Field quickly.
pub fn field(key: &str, value: Value) -> Field {
    Field {
        key: key.to_string(),
        value,
    }
}

/// Helper to build a string Value.
pub fn str_val(s: &str) -> Value {
    Value::Str(s.to_string())
}

/// Helper to build an int Value.
pub fn int_val(n: i64) -> Value {
    Value::Int(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_simple_fields() {
        let doc = render(&[
            field("bin", str_val("~/.local/bin/planeai-cli")),
            field(
                "description",
                str_val("Orchestrate parallel AI coding agents"),
            ),
        ]);
        assert_eq!(
            doc,
            "bin: ~/.local/bin/planeai-cli\ndescription: Orchestrate parallel AI coding agents\n"
        );
    }

    #[test]
    fn renders_tabular_array() {
        let doc = render(&[field(
            "tasks",
            Value::Table {
                columns: vec!["key".into(), "title".into(), "status".into()],
                rows: vec![
                    vec!["PLA-1".into(), "Fix bug".into(), "todo".into()],
                    vec!["PLA-2".into(), "Add feature".into(), "done".into()],
                ],
            },
        )]);
        assert_eq!(
            doc,
            "tasks[2]{key,title,status}:\n  PLA-1,Fix bug,todo\n  PLA-2,Add feature,done\n"
        );
    }

    #[test]
    fn renders_nested_object() {
        let doc = render(&[field(
            "task",
            Value::Object(vec![
                field("key", str_val("PLA-1")),
                field("title", str_val("Fix bug")),
                field("priority", int_val(1)),
            ]),
        )]);
        assert_eq!(
            doc,
            "task:\n  key: PLA-1\n  title: Fix bug\n  priority: 1\n"
        );
    }

    #[test]
    fn renders_primitive_array() {
        let doc = render(&[field(
            "help",
            Value::Array(vec![
                "Run `planeai-cli axi task show <key>`".into(),
                "Run `planeai-cli axi task add`".into(),
            ]),
        )]);
        assert_eq!(
            doc,
            "help[2]: Run `planeai-cli axi task show <key>`,Run `planeai-cli axi task add`\n"
        );
    }

    #[test]
    fn quotes_strings_with_commas() {
        let doc = render(&[field("note", str_val("hello, world"))]);
        assert_eq!(doc, "note: \"hello, world\"\n");
    }

    #[test]
    fn quotes_empty_string() {
        let doc = render(&[field("name", str_val(""))]);
        assert_eq!(doc, "name: \"\"\n");
    }

    #[test]
    fn quotes_numeric_looking_string() {
        let doc = render(&[field("version", str_val("123"))]);
        assert_eq!(doc, "version: \"123\"\n");
    }

    #[test]
    fn renders_bool_and_null() {
        let doc = render(&[
            field("active", Value::Bool(true)),
            field("parent", Value::Null),
        ]);
        assert_eq!(doc, "active: true\nparent: null\n");
    }

    #[test]
    fn renders_help_as_multiline_list() {
        let doc = render(&[field(
            "help",
            Value::List(vec![
                "Run `planeai-cli axi task show <key>` for details".into(),
                "Run `planeai-cli axi task add` to create a task".into(),
            ]),
        )]);
        assert_eq!(
            doc,
            concat!(
                "help[2]:\n",
                "  - Run `planeai-cli axi task show <key>` for details\n",
                "  - Run `planeai-cli axi task add` to create a task\n",
            )
        );
    }
}
