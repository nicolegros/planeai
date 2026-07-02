use std::io::Write;

fn main() {
    // Load .env from repo root
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let repo_root = std::path::Path::new(&manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let _ = dotenvy::from_path(repo_root.join(".env"));

    let client_id = std::env::var("JIRA_CLIENT_ID").expect("JIRA_CLIENT_ID must be set");
    let client_secret =
        std::env::var("JIRA_CLIENT_SECRET").expect("JIRA_CLIENT_SECRET must be set");

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let dest = std::path::Path::new(&out_dir).join("oauth_credentials.rs");
    let mut f = std::fs::File::create(dest).unwrap();
    writeln!(f, "const CLIENT_ID: &str = {:?};", client_id).unwrap();
    writeln!(f, "const CLIENT_SECRET: &str = {:?};", client_secret).unwrap();

    println!("cargo::rerun-if-env-changed=JIRA_CLIENT_ID");
    println!("cargo::rerun-if-env-changed=JIRA_CLIENT_SECRET");
    println!(
        "cargo::rerun-if-changed={}",
        repo_root.join(".env").display()
    );
}
