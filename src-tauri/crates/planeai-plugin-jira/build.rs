use std::fs::File;
use std::io::Write;

fn main() {
    println!("cargo::rerun-if-env-changed=JIRA_CLIENT_ID");
    println!("cargo::rerun-if-env-changed=JIRA_CLIENT_SECRET");
    let client_id = std::env::var("JIRA_CLIENT_ID").unwrap_or_else(|_| {
        println!("cargo::warning=JIRA_CLIENT_ID not set — using placeholder. OAuth will not work.");
        "PLACEHOLDER_CLIENT_ID".to_string()
    });
    let client_secret = std::env::var("JIRA_CLIENT_SECRET").unwrap_or_else(|_| {
        println!(
            "cargo::warning=JIRA_CLIENT_SECRET not set — using placeholder. OAuth will not work."
        );
        "PLACEHOLDER_CLIENT_SECRET".to_string()
    });
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR must be set");
    let mut file = File::create(std::path::Path::new(&out_dir).join("oauth_credentials.rs"))
        .expect("failed to create generated OAuth credentials");
    writeln!(file, "const CLIENT_ID: &str = {:?};", client_id).unwrap();
    writeln!(file, "const CLIENT_SECRET: &str = {:?};", client_secret).unwrap();
}
