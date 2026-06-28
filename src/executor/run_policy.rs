use crate::executor::node_builtins::{is_npm_install_builtin, is_pip_without_package};
use crate::types::ExecResult;
use anyhow::{anyhow, Result};
use std::path::Path;

pub const ALLOWED_PROGRAMS: &[&str] = &[
    "python3",
    "python",
    "venv/bin/python3",
    "venv/bin/python",
    "venv/bin/pip3",
    "venv/bin/pip",
    "venv/bin/uvicorn",
    "venv/bin/gunicorn",
    "venv/bin/pytest",
    "pytest",
    "node",
    "npm",
    "npx",
    "node_modules/.bin/jest",
    "cargo",
    "rustc",
    "git",
    "go",
    "mkdir",
    "touch",
    "ls",
    "cat",
    "cp",
    "mv",
    "echo",
    "find",
    "grep",
    "curl",
    "chmod",
];

const SERVICE_PROGRAMS: &[&str] = &["venv/bin/uvicorn", "uvicorn", "venv/bin/gunicorn"];

pub enum ShellPolicyDecision {
    Return(ExecResult),
    Execute { prog: String, args: Vec<String> },
    Service { prog: String, args: Vec<String> },
}

pub fn preflight_shell(
    command: &str,
    workspace: &Path,
    replay_mode: bool,
) -> Result<ShellPolicyDecision> {
    let mut parts = command.split_whitespace();
    let prog = parts
        .next()
        .ok_or_else(|| anyhow!("Empty command"))?
        .to_string();
    let args: Vec<String> = parts.map(|s| s.to_string()).collect();

    if let Some(decision) =
        replay_npm_mutation_policy(&prog, &args, workspace, command, replay_mode)
    {
        return Ok(decision);
    }

    if let Some(decision) = npm_builtin_install_policy(command, replay_mode) {
        return Ok(decision);
    }

    if let Some(decision) = pip_install_missing_package_policy(command) {
        return Ok(decision);
    }

    if SERVICE_PROGRAMS.contains(&prog.as_str()) {
        return Ok(ShellPolicyDecision::Service { prog, args });
    }

    let is_local_binary = prog.starts_with("./") || prog.starts_with("target/");
    if is_local_binary || ALLOWED_PROGRAMS.contains(&prog.as_str()) {
        return Ok(ShellPolicyDecision::Execute { prog, args });
    }

    Ok(ShellPolicyDecision::Return(ExecResult::fail(format!(
        "'{}' is not in the allowed programs list",
        prog
    ))))
}

fn replay_npm_mutation_policy(
    prog: &str,
    args: &[String],
    workspace: &Path,
    command: &str,
    replay_mode: bool,
) -> Option<ShellPolicyDecision> {
    if !replay_mode || prog != "npm" {
        return None;
    }

    let action = args.first().map(String::as_str).unwrap_or("");
    let is_mutating_npm = matches!(action, "install" | "i" | "ci");
    if !is_mutating_npm {
        return None;
    }

    let node_modules = workspace.join("node_modules");
    if node_modules.exists() {
        eprintln!(
            "[TRACE] Replay mode: skipping '{}' to preserve cached node_modules",
            command
        );
        Some(ShellPolicyDecision::Return(ExecResult::ok(
            "Replay mode: skipped npm dependency mutation; cached node_modules present",
        )))
    } else {
        Some(ShellPolicyDecision::Return(ExecResult::fail(
            "REPLAY_ENV_MISMATCH: npm dependency install requested in replay but node_modules is unavailable".to_string(),
        )))
    }
}

fn npm_builtin_install_policy(command: &str, replay_mode: bool) -> Option<ShellPolicyDecision> {
    if replay_mode {
        return None;
    }

    let module = is_npm_install_builtin(command)?;
    Some(ShellPolicyDecision::Return(ExecResult::fail(format!(
        "npm install '{}' rejected — '{}' is a Node.js built-in module and requires no installation.\nCORRECT: import {{ ... }} from '{}'\nNEVER:   npm install {}",
        module, module, module, module
    ))))
}

fn pip_install_missing_package_policy(command: &str) -> Option<ShellPolicyDecision> {
    if is_pip_without_package(command) {
        Some(ShellPolicyDecision::Return(ExecResult::fail(
            "pip install needs package name: e.g. venv/bin/pip3 install pytest".to_string(),
        )))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn rejects_npm_install_builtin_in_live_mode() {
        let d = tempdir().expect("test setup/use should succeed");
        let decision = preflight_shell("npm install crypto", d.path(), false)
            .expect("test setup/use should succeed");

        match decision {
            ShellPolicyDecision::Return(result) => {
                assert!(!result.success);
                assert!(
                    result.stdout.contains("Node.js built-in")
                        || result.stderr.contains("Node.js built-in")
                );
            }
            _ => panic!("expected rejection result"),
        }
    }

    #[test]
    fn allows_npm_install_non_builtin_package() {
        let d = tempdir().expect("test setup/use should succeed");
        let decision = preflight_shell("npm install express", d.path(), false)
            .expect("test setup/use should succeed");

        match decision {
            ShellPolicyDecision::Execute { prog, args } => {
                assert_eq!(prog, "npm");
                assert_eq!(args, vec!["install".to_string(), "express".to_string()]);
            }
            _ => panic!("expected execute decision"),
        }
    }

    #[test]
    fn replay_skips_npm_install_when_node_modules_exists() {
        let d = tempdir().expect("test setup/use should succeed");
        std::fs::create_dir_all(d.path().join("node_modules"))
            .expect("test setup/use should succeed");

        let decision = preflight_shell("npm install crypto", d.path(), true)
            .expect("test setup/use should succeed");

        match decision {
            ShellPolicyDecision::Return(result) => {
                assert!(result.success);
                assert!(
                    result.stdout.contains("skipped npm dependency mutation")
                        || result.stderr.contains("skipped npm dependency mutation")
                );
            }
            _ => panic!("expected replay skip result"),
        }
    }

    #[test]
    fn replay_rejects_npm_install_without_node_modules() {
        let d = tempdir().expect("test setup/use should succeed");

        let decision = preflight_shell("npm install crypto", d.path(), true)
            .expect("test setup/use should succeed");

        match decision {
            ShellPolicyDecision::Return(result) => {
                assert!(!result.success);
                assert!(
                    result.stdout.contains("REPLAY_ENV_MISMATCH")
                        || result.stderr.contains("REPLAY_ENV_MISMATCH")
                );
            }
            _ => panic!("expected replay mismatch result"),
        }
    }

    #[test]
    fn rejects_pip_install_without_package_name() {
        let d = tempdir().expect("test setup/use should succeed");
        let decision = preflight_shell("venv/bin/pip install", d.path(), false)
            .expect("test setup/use should succeed");

        match decision {
            ShellPolicyDecision::Return(result) => {
                assert!(!result.success);
                assert!(
                    result.stdout.contains("pip install needs package name")
                        || result.stderr.contains("pip install needs package name")
                );
            }
            _ => panic!("expected pip rejection result"),
        }
    }
}
