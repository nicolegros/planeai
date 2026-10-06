use regex::Regex;
use std::collections::HashMap;

/// Render a template string with `{var}` and `{var:transform}` syntax, exactly as the
/// desktop UI's `renderTemplate` does: names and slugs use ASCII word characters.
pub fn render(template: &str, vars: &HashMap<&str, &str>) -> String {
    let re = Regex::new(r"\{((?-u:\w)+)(?::((?-u:\w)+))?\}").unwrap();
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
    let mut slug = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.strip_prefix('-').unwrap_or(&slug);
    slug.strip_suffix('-').unwrap_or(slug).to_string()
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

#[cfg(test)]
mod tests {
    use super::render;
    use std::collections::HashMap;

    fn rendered(template: &str, vars: &[(&str, &str)]) -> String {
        render(template, &vars.iter().copied().collect::<HashMap<_, _>>())
    }

    // The same literal cases as src/lib/__tests__/render-template.test.ts: both renderers
    // must agree, since the desktop UI previews what the backend renders.
    #[test]
    fn transforms_match_the_frontend_renderer() {
        let vars = [("key", "PLA-12"), ("title", "Fix Login Redirect!")];
        assert_eq!(
            rendered("{key:lower}/{title:slug}", &vars),
            "pla-12/fix-login-redirect"
        );
        assert_eq!(
            rendered("{key:upper}: {title}", &vars),
            "PLA-12: Fix Login Redirect!"
        );
        assert_eq!(rendered("{title:reverse}", &vars), "Fix Login Redirect!");
    }

    #[test]
    fn slug_keeps_ascii_word_characters_only() {
        let slug = |title| rendered("{title:slug}", &[("title", title)]);
        assert_eq!(slug("  Hello, World  "), "hello-world");
        assert_eq!(slug("snake_case Name"), "snake_case-name");
        assert_eq!(slug("Café déjà vu"), "caf-d-j-vu");
        assert_eq!(slug("--a--"), "a");
    }

    #[test]
    fn unknown_and_non_ascii_placeholders() {
        assert_eq!(rendered("{nope}-x", &[]), "-x");
        assert_eq!(rendered("{tïtle}", &[("tïtle", "x")]), "{tïtle}");
    }
}
