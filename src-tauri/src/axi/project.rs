//! AXI project subcommands — ls.

use planeai_toon::{field, render, str_val, Value};

use crate::db;

pub fn project_ls(conn: &rusqlite::Connection) -> (String, i32) {
    let projects = db::list_projects(conn).unwrap_or_default();

    if projects.is_empty() {
        return (
            render(&[
                field("projects", str_val("0 projects")),
                field(
                    "help",
                    Value::List(vec!["Create a project in the planeai GUI".into()]),
                ),
            ]),
            0,
        );
    }

    let rows: Vec<Vec<String>> = projects
        .iter()
        .map(|p| {
            vec![
                p.prefix.clone(),
                p.name.clone(),
                p.path.clone(),
                p.status.clone(),
            ]
        })
        .collect();

    let fields = vec![field(
        "projects",
        Value::Table {
            columns: vec![
                "prefix".into(),
                "name".into(),
                "path".into(),
                "status".into(),
            ],
            rows,
        },
    )];
    (render(&fields), 0)
}
