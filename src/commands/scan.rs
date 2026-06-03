#![allow(clippy::print_literal)]

pub fn cmd_scan(workspace: &str, json: bool) {
    use crate::context::Scanner;
    use std::path::Path;

    let path = Path::new(workspace);
    if !path.exists() {
        eprintln!(" Workspace not found: {}", workspace);
        std::process::exit(1);
    }

    let profile = Scanner::scan(path);

    if json {
        match serde_json::to_string_pretty(&profile) {
            Ok(s) => println!("{}", s),
            Err(e) => {
                eprintln!("failed to serialize scan profile: {}", e);
                return;
            }
        };
        return;
    }

    // human-readable output
    let conf_bar = confidence_bar(profile.confidence);
    println!();
    println!(" Project  : {}", profile.project_name);
    println!(" Language : {}", profile.language);
    println!(
        " Manifest : {}",
        profile
            .dependency_file
            .as_ref()
            .map(|p: &std::path::PathBuf| p.display().to_string())
            .unwrap_or_default()
    );
    println!(
        " Entry    : {}",
        if profile.entry_points.is_empty() {
            "".to_string()
        } else {
            profile
                .entry_points
                .iter()
                .map(|p: &std::path::PathBuf| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        }
    );
    println!(
        " Tests    : {} {}",
        "",
        profile.test_framework.as_deref().unwrap_or("")
    );
    println!(
        "  Build    : {}",
        profile.build_cmd.as_deref().unwrap_or("")
    );
    println!(" Test cmd : {}", profile.test_cmd.as_deref().unwrap_or(""));
    println!(
        " Confid.  : {:.0}%  {}",
        profile.confidence * 100.0,
        conf_bar
    );
    println!();
}

pub fn confidence_bar(c: f32) -> String {
    let filled = (c * 10.0).round() as usize;
    let empty = 10 - filled.min(10);
    format!("[{}{}]", "".repeat(filled), "".repeat(empty))
}

//
// Compile Bench  v7.1
//
