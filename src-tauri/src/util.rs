use crate::command;
use crate::config;

pub fn expand_tilde(path: &str) -> String {
    if path.starts_with("~/") || path == "~" {
        let home = config::home_dir();
        if !home.is_empty() {
            return path.replacen("~", &home, 1);
        }
    }
    path.to_string()
}

/// Shell-escape a string by wrapping in single quotes, escaping any internal single quotes.
/// Resolve a command name to its full path, checking user-local bin directories
/// that may not be in PATH when launched from a GUI app.
pub fn resolve_command(cmd: &str) -> String {
    command::resolve(cmd)
}

pub fn sanitize_project_name(name: &str) -> String {
    name.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .replace(' ', "-")
        .replace([':', '?', '*', '<', '>', '|', '"'], "")
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_windows_absolute_path() {
        assert_eq!(
            sanitize_project_name(r"C:\Users\nic\Developer\my-project"),
            "my-project"
        );
    }

    #[test]
    fn sanitize_unix_path() {
        assert_eq!(sanitize_project_name("/home/user/my-project"), "my-project");
    }

    #[test]
    fn sanitize_trailing_slash() {
        assert_eq!(
            sanitize_project_name("/home/user/my-project/"),
            "my-project"
        );
    }

    #[test]
    fn sanitize_spaces() {
        assert_eq!(sanitize_project_name("My Cool Project"), "my-cool-project");
    }

    #[test]
    fn sanitize_plain_name() {
        assert_eq!(sanitize_project_name("planeai"), "planeai");
    }
}
