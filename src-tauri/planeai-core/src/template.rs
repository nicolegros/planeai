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
