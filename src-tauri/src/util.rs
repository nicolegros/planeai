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

/// Returns whether `repo_path` is a project path or one of its descendants.
///
/// `Path::starts_with` compares path components, so sibling names such as
/// `deployment-pipeline` and `deployment-pipeline-api` cannot collide.
pub fn is_project_path_or_descendant(repo_path: &str, project_path: &str) -> bool {
    std::path::Path::new(repo_path).starts_with(std::path::Path::new(project_path))
}

/// Resolve a command name to its full path, checking user-local bin directories
/// that may not be in PATH when launched from a GUI app.
#[allow(dead_code)]
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
    fn project_path_match_requires_a_component_boundary() {
        let pipeline = "/Users/nicolaslegros/Developer/deployment-pipeline";
        let api = "/Users/nicolaslegros/Developer/deployment-pipeline-api";

        assert!(!is_project_path_or_descendant(api, pipeline));
        assert!(is_project_path_or_descendant(api, api));
        assert!(is_project_path_or_descendant(
            "/Users/nicolaslegros/Developer/deployment-pipeline-api/.git",
            api
        ));
    }

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
