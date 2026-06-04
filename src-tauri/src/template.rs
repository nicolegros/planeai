use regex::Regex;
use std::collections::HashMap;

/// Render a template string with `{var}` and `{var:transform}` syntax.
/// Supported transforms: `slug`, `lower`, `upper`. Default is raw (no transform).
pub fn render(template: &str, vars: &HashMap<&str, &str>) -> String {
    let re = Regex::new(r"\{(\w+)(?::(\w+))?\}").unwrap();
    re.replace_all(template, |caps: &regex::Captures| {
        let var = caps.get(1).unwrap().as_str();
        let value = vars.get(var).copied().unwrap_or("");
        match caps.get(2).map(|m| m.as_str()) {
            Some(t) => apply_transform(value, t),
            None => value.to_string(),
        }
    })
    .into_owned()
}

fn apply_transform(value: &str, transform: &str) -> String {
    match transform {
        "lower" => value.to_lowercase(),
        "upper" => value.to_uppercase(),
        "slug" => slugify(value),
        _ => value.to_string(),
    }
}

fn slugify(s: &str) -> String {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> HashMap<&'static str, &'static str> {
        let mut m = HashMap::new();
        m.insert("key", "KAN-3");
        m.insert("title", "Add dark mode support");
        m.insert("description", "We need dark mode for accessibility.");
        m.insert("status", "todo");
        m
    }

    #[test]
    fn raw_variable_substitution() {
        let result = render("{key}: {title}", &vars());
        assert_eq!(result, "KAN-3: Add dark mode support");
    }

    #[test]
    fn slug_transform() {
        let result = render("{title:slug}", &vars());
        assert_eq!(result, "add-dark-mode-support");
    }

    #[test]
    fn lower_transform() {
        let result = render("{key:lower}", &vars());
        assert_eq!(result, "kan-3");
    }

    #[test]
    fn upper_transform() {
        let result = render("{key:upper}", &vars());
        assert_eq!(result, "KAN-3");
    }

    #[test]
    fn combined_template() {
        let result = render("{key:lower}/{title:slug}", &vars());
        assert_eq!(result, "kan-3/add-dark-mode-support");
    }

    #[test]
    fn missing_variable_left_as_empty() {
        let result = render("{unknown}", &vars());
        assert_eq!(result, "");
    }

    #[test]
    fn no_placeholders_passthrough() {
        let result = render("plain text", &vars());
        assert_eq!(result, "plain text");
    }

    #[test]
    fn multiline_template() {
        let result = render("Task {key}: {title}\n\n{description}", &vars());
        assert_eq!(
            result,
            "Task KAN-3: Add dark mode support\n\nWe need dark mode for accessibility."
        );
    }
}
