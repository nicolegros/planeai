use regex::Regex;
use std::collections::HashMap;

/// Render a template string with `{var}` and `{var:transform}` syntax.
pub fn render(template: &str, vars: &HashMap<&str, &str>) -> String {
    let re = Regex::new(r"\{(\w+)(?::(\w+))?\}").unwrap();
    re.replace_all(template, |caps: &regex::Captures| {
        let var = caps.get(1).unwrap().as_str();
        let value = vars.get(var).copied().unwrap_or("");
        match caps.get(2).map(|m| m.as_str()) {
            Some("lower") => value.to_lowercase(),
            Some("upper") => value.to_uppercase(),
            Some("slug") => slugify(value),
            _ => value.to_string(),
        }
    })
    .into_owned()
}

fn slugify(s: &str) -> String {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Shell-escape a string using single quotes.
pub fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Render a prompt_command template with a shell-escaped prompt value, appending to cmd.
/// Escapes the prompt text first, then substitutes into the template.
pub fn append_prompt(cmd: &mut String, prompt_command: &str, prompt_text: &str) {
    let escaped = shell_escape(prompt_text);
    let mut vars = HashMap::new();
    vars.insert("prompt", escaped.as_str());
    let rendered = render(prompt_command, &vars);
    *cmd = format!("{cmd} {rendered}");
}
