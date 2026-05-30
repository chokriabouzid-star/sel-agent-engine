/// AutoFix:  Go stdlib import   LLM
pub fn autofix_go_undefined_import(file: &std::path::Path, err: &str) -> Option<String> {
    let filename = file.file_name()?.to_str()?;
    let go_std: &[(&str, &str)] = &[
        ("fmt", "fmt"),
        ("errors", "errors"),
        ("strings", "strings"),
        ("strconv", "strconv"),
        ("sort", "sort"),
        ("math", "math"),
        ("os", "os"),
        ("io", "io"),
        ("log", "log"),
        ("time", "time"),
        ("sync", "sync"),
        ("context", "context"),
        ("bytes", "bytes"),
        ("bufio", "bufio"),
    ];

    //    "undefined: fmt"
    let sym = err
        .lines()
        .find(|l| l.contains(filename) && l.contains("undefined:"))?
        .split("undefined:")
        .nth(1)?
        .split_whitespace()
        .next()?
        .split('.')
        .next()?
        .to_string();

    //   stdlib
    let pkg = go_std
        .iter()
        .find(|(name, _)| *name == sym.as_str())
        .map(|(_, pkg)| *pkg)?;

    //
    let src = std::fs::read_to_string(file).ok()?;

    //
    if src.contains(&format!("\"{}\"", pkg)) {
        return None;
    }

    //  import
    let new_src = if src.contains("import (") {
        src.replacen("import (", &format!("import (\n\t\"{}\"", pkg), 1)
    } else {
        //   package declaration
        let pkg_line = src.lines().find(|l| l.starts_with("package "))?.to_string();
        src.replacen(&pkg_line, &format!("{}\n\nimport \"{}\"", pkg_line, pkg), 1)
    };

    std::fs::write(file, &new_src).ok()?;
    Some(pkg.to_string())
}

/// AutoFix:  Go import
pub fn autofix_go_unused_import(file: &std::path::Path, err: &str) -> Option<String> {
    let filename = file.file_name()?.to_str()?;
    let pkg = err
        .lines()
        .find(|l| l.contains(filename) && l.contains("imported and not used"))?
        .split('"')
        .nth(1)?
        .to_string();

    if pkg.is_empty() {
        return None;
    }

    let src = std::fs::read_to_string(file).ok()?;

    let block_pattern = format!("\t\"{}\"", pkg);
    let block_pattern_no_tab = format!("    \"{}\"", pkg);

    let new_src = if src.contains(&block_pattern) {
        let lines: Vec<&str> = src.lines().collect();
        let filtered: Vec<&str> = lines
            .iter()
            .filter(|&&line| {
                let trimmed = line.trim();
                trimmed != format!("\"{}\"", pkg).as_str()
            })
            .cloned()
            .collect();
        let new = filtered.join("\n");
        new.replace("import (\n)", "").replace("import (\n\n)", "")
    } else if src.contains(&block_pattern_no_tab) {
        src.lines()
            .filter(|line| line.trim() != format!("\"{}\"", pkg).as_str())
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        let single_pattern = format!("import \"{}\"", pkg);
        if src.contains(&single_pattern) {
            src.lines()
                .filter(|line| !line.trim().starts_with(&single_pattern))
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            return None;
        }
    };

    let new_src = {
        let mut cleaned = new_src.clone();
        while let Some(start) = cleaned.find("import (") {
            if let Some(end) = cleaned[start..].find(')') {
                let block_content = &cleaned[start + 8..start + end];
                if block_content.trim().is_empty() {
                    cleaned = format!("{}{}", &cleaned[..start], &cleaned[start + end + 1..]);
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        cleaned
    };

    std::fs::write(file, &new_src).ok()?;
    Some(pkg)
}

pub fn find_cargo_workspace(root: &std::path::Path) -> std::path::PathBuf {
    if root.join("Cargo.toml").exists() {
        return root.to_path_buf();
    }
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let sub = entry.path();
            if sub.is_dir() && sub.join("Cargo.toml").exists() {
                return sub;
            }
        }
    }
    root.to_path_buf()
}
