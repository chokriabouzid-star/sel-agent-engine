use crate::executor::node_builtins::{is_npm_install_builtin, is_pip_without_package};
use crate::types::ExecResult;
use anyhow::{anyhow, Result};
use std::path::Path;
use std::process::Command;

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
    if let Some(decision) = pip_install_stdlib_policy(command) {
        return Ok(decision);
    }
    if let Some(decision) =
        replay_pip_mutation_policy(&prog, &args, workspace, command, replay_mode)
    {
        return Ok(decision);
    }
    if let Some(decision) =
        replay_python_venv_bootstrap_policy(&prog, &args, workspace, replay_mode)
    {
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

fn replay_pip_mutation_policy(
    prog: &str,
    args: &[String],
    workspace: &Path,
    command: &str,
    replay_mode: bool,
) -> Option<ShellPolicyDecision> {
    if !replay_mode {
        return None;
    }

    let package = extract_pip_install_package(prog, args)?;
    let import_name = python_import_name_for_package(&package);

    if python_importable_in_workspace_venv(workspace, &import_name) {
        eprintln!(
            "[TRACE] Replay mode: skipping '{}' because '{}' is already importable in workspace venv",
            command, import_name
        );
        Some(ShellPolicyDecision::Return(ExecResult::ok(format!(
            "Replay mode: skipped pip dependency mutation; package '{}' already available in workspace venv",
            package
        ))))
    } else {
        Some(ShellPolicyDecision::Return(ExecResult::fail(format!(
            "REPLAY_ENV_MISMATCH: pip package '{}' not available in workspace venv during replay",
            package
        ))))
    }
}

fn extract_pip_install_package(prog: &str, args: &[String]) -> Option<String> {
    let is_direct_pip = matches!(prog, "pip" | "pip3" | "venv/bin/pip" | "venv/bin/pip3")
        && args.first().map(String::as_str) == Some("install");

    let is_python_module_pip = matches!(
        prog,
        "python" | "python3" | "venv/bin/python" | "venv/bin/python3"
    ) && args.len() >= 4
        && args[0] == "-m"
        && (args[1] == "pip" || args[1] == "pip3")
        && args[2] == "install";

    if !is_direct_pip && !is_python_module_pip {
        return None;
    }

    let install_pos = args.iter().position(|arg| arg == "install")?;
    args[install_pos + 1..]
        .iter()
        .find(|arg| !arg.starts_with('-'))
        .cloned()
}

fn python_import_name_for_package(package: &str) -> String {
    package
        .split(['=', '<', '>', '!', '~', '['])
        .next()
        .unwrap_or(package)
        .replace('-', "_")
}

fn python_importable_in_workspace_venv(workspace: &Path, module: &str) -> bool {
    let python = workspace.join("venv/bin/python3");
    if !python.exists() {
        return false;
    }

    Command::new(&python)
        .args(["-c", &format!("import {}", module)])
        .current_dir(workspace)
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

fn replay_python_venv_bootstrap_policy(
    prog: &str,
    args: &[String],
    workspace: &Path,
    replay_mode: bool,
) -> Option<ShellPolicyDecision> {
    if !replay_mode {
        return None;
    }

    // Match: python3 -m venv venv  |  python -m venv venv
    let is_venv_create = matches!(
        prog,
        "python3" | "python" | "venv/bin/python3" | "venv/bin/python"
    ) && args.len() >= 3
        && args[0] == "-m"
        && args[1] == "venv";

    if !is_venv_create {
        return None;
    }

    // إذا كان workspace/venv موجوداً (مجلد حقيقي أو symlink)، تخطَّ الأمر
    if workspace.join("venv").exists() {
        eprintln!(
            "[TRACE] Replay mode: skipping '{}' — workspace venv already available",
            std::iter::once(prog)
                .chain(args.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join(" ")
        );
        return Some(ShellPolicyDecision::Return(ExecResult::ok(
            "Replay mode: skipped python venv bootstrap; workspace venv already available",
        )));
    }

    // كاش miss — اترك الأمر يُنفَّذ (محلي، حتمي، لا شبكة)
    None
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

/// Python standard library modules — never need pip install.
const PYTHON_STDLIB_MODULES: &[&str] = &[
    "abc",
    "ast",
    "asyncio",
    "base64",
    "bisect",
    "builtins",
    "calendar",
    "cmath",
    "cmd",
    "codecs",
    "collections",
    "colorsys",
    "concurrent",
    "configparser",
    "contextlib",
    "copy",
    "csv",
    "dataclasses",
    "datetime",
    "decimal",
    "difflib",
    "dis",
    "doctest",
    "email",
    "enum",
    "errno",
    "filecmp",
    "fileinput",
    "fnmatch",
    "fractions",
    "ftplib",
    "functools",
    "gc",
    "getopt",
    "getpass",
    "gettext",
    "glob",
    "gzip",
    "hashlib",
    "heapq",
    "hmac",
    "html",
    "http",
    "importlib",
    "inspect",
    "io",
    "ipaddress",
    "itertools",
    "json",
    "keyword",
    "linecache",
    "locale",
    "logging",
    "lzma",
    "math",
    "mimetypes",
    "mmap",
    "multiprocessing",
    "numbers",
    "operator",
    "optparse",
    "os",
    "pathlib",
    "pdb",
    "pickle",
    "pkgutil",
    "platform",
    "pprint",
    "profile",
    "queue",
    "random",
    "re",
    "runpy",
    "sched",
    "secrets",
    "select",
    "shelve",
    "shlex",
    "shutil",
    "signal",
    "site",
    "smtplib",
    "socket",
    "socketserver",
    "sqlite3",
    "ssl",
    "stat",
    "statistics",
    "string",
    "struct",
    "subprocess",
    "sys",
    "sysconfig",
    "tarfile",
    "tempfile",
    "test",
    "textwrap",
    "threading",
    "time",
    "timeit",
    "token",
    "tokenize",
    "tomllib",
    "trace",
    "traceback",
    "types",
    "typing",
    "unicodedata",
    "unittest",
    "urllib",
    "uuid",
    "venv",
    "warnings",
    "wave",
    "weakref",
    "webbrowser",
    "xml",
    "xmlrpc",
    "zipfile",
    "zipimport",
    "zlib",
    "zoneinfo",
];

fn is_python_stdlib(module: &str) -> bool {
    let base = module.split('.').next().unwrap_or(module);
    PYTHON_STDLIB_MODULES.contains(&base)
}

fn pip_install_stdlib_policy(command: &str) -> Option<ShellPolicyDecision> {
    let cmd = command.trim();
    if !cmd.contains("pip install") && !cmd.contains("pip3 install") {
        return None;
    }
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    let pos = parts.iter().position(|&p| p == "install")?;
    let pkg = parts[pos + 1..].iter().find(|&&p| !p.starts_with('-'))?;
    if is_python_stdlib(pkg) {
        Some(ShellPolicyDecision::Return(ExecResult::fail(format!(
            "pip install '{}' rejected — '{}' is a Python standard library module, no installation needed.
CORRECT: import {}
NEVER:   pip install {}",
            pkg, pkg, pkg, pkg
        ))))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;

    #[cfg(unix)]
    fn write_fake_workspace_python(
        workspace: &Path,
        importable_modules: &[&str],
    ) -> std::path::PathBuf {
        let bin_dir = workspace.join("venv/bin");
        std::fs::create_dir_all(&bin_dir).expect("test setup/use should succeed");
        let python = bin_dir.join("python3");

        let mut script = String::from("#!/bin/sh\ncase \"$2\" in\n");
        for module in importable_modules {
            script.push_str(&format!("  \"import {}\") exit 0 ;;\n", module));
        }
        script.push_str("  *) exit 1 ;;\nesac\n");

        std::fs::write(&python, script).expect("test setup/use should succeed");
        let mut perms = std::fs::metadata(&python)
            .expect("test setup/use should succeed")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&python, perms).expect("test setup/use should succeed");
        python
    }

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
    #[test]
    fn rejects_pip_install_stdlib_unittest() {
        let d = tempdir().expect("tempdir");
        let decision = preflight_shell("venv/bin/pip install unittest", d.path(), false)
            .expect("should return decision");
        match decision {
            ShellPolicyDecision::Return(result) => {
                assert!(!result.success);
                assert!(
                    result.stdout.contains("standard library")
                        || result.stderr.contains("standard library"),
                    "got: {} / {}",
                    result.stdout,
                    result.stderr
                );
            }
            _ => panic!("expected rejection but got a non-Return decision"),
        }
    }

    #[test]
    fn rejects_pip_install_stdlib_json() {
        let d = tempdir().expect("tempdir");
        let decision = preflight_shell("venv/bin/pip3 install json", d.path(), false)
            .expect("should return decision");
        match decision {
            ShellPolicyDecision::Return(result) => assert!(!result.success),
            _ => panic!("expected rejection"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn replay_skips_pip_install_when_package_already_importable() {
        let d = tempdir().expect("test setup/use should succeed");
        let _python = write_fake_workspace_python(d.path(), &["pytest"]);

        let decision = preflight_shell("venv/bin/pip install pytest", d.path(), true)
            .expect("test setup/use should succeed");

        match decision {
            ShellPolicyDecision::Return(result) => {
                assert!(result.success);
                assert!(
                    result.stdout.contains("skipped pip dependency mutation")
                        || result.stderr.contains("skipped pip dependency mutation")
                );
            }
            _ => panic!("expected replay skip result"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn replay_skips_venv_bootstrap_when_venv_already_exists() {
        let d = tempdir().expect("test setup/use should succeed");
        std::fs::create_dir_all(d.path().join("venv/bin")).expect("test setup");

        let decision = preflight_shell("python3 -m venv venv", d.path(), true)
            .expect("test setup/use should succeed");

        match decision {
            ShellPolicyDecision::Return(result) => {
                assert!(result.success);
                assert!(
                    result.stdout.contains("skipped python venv bootstrap")
                        || result.stderr.contains("skipped python venv bootstrap")
                );
            }
            _ => panic!("expected replay skip result"),
        }
    }

    #[test]
    fn replay_allows_venv_bootstrap_when_venv_missing() {
        let d = tempdir().expect("test setup/use should succeed");

        let decision = preflight_shell("python3 -m venv venv", d.path(), true)
            .expect("test setup/use should succeed");

        match decision {
            ShellPolicyDecision::Execute { prog, .. } => {
                assert_eq!(prog, "python3");
            }
            _ => panic!("expected execute decision when venv is absent"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn replay_rejects_pip_install_when_package_missing_from_venv() {
        let d = tempdir().expect("test setup/use should succeed");
        let _python = write_fake_workspace_python(d.path(), &[]);

        let decision = preflight_shell("venv/bin/pip install requests", d.path(), true)
            .expect("test setup/use should succeed");

        match decision {
            ShellPolicyDecision::Return(result) => {
                assert!(!result.success);
                assert!(
                    result.stdout.contains("REPLAY_ENV_MISMATCH")
                        || result.stderr.contains("REPLAY_ENV_MISMATCH")
                );
                assert!(result.stdout.contains("requests") || result.stderr.contains("requests"));
            }
            _ => panic!("expected replay mismatch result"),
        }
    }

    #[test]
    fn allows_pip_install_third_party() {
        let d = tempdir().expect("tempdir");
        let decision = preflight_shell("venv/bin/pip install requests", d.path(), false)
            .expect("should return decision");
        // requests هي third-party — لا يجب رفضها بسبب stdlib check
        if let ShellPolicyDecision::Return(result) = decision {
            assert!(
                !result.stdout.contains("standard library")
                    && !result.stderr.contains("standard library"),
                "requests should not be rejected as stdlib"
            );
        }
    }
}
