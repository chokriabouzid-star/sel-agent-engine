// src/executor/node_builtins.rs
// Node.js built-in modules and lightweight command parsing helpers.

pub static NODE_BUILTIN_MODULES: &[&str] = &[
    "assert",
    "buffer",
    "child_process",
    "cluster",
    "console",
    "crypto",
    "dns",
    "domain",
    "events",
    "fs",
    "http",
    "http2",
    "https",
    "module",
    "net",
    "os",
    "path",
    "perf_hooks",
    "process",
    "punycode",
    "querystring",
    "readline",
    "repl",
    "stream",
    "string_decoder",
    "timers",
    "tls",
    "trace_events",
    "tty",
    "url",
    "util",
    "v8",
    "vm",
    "worker_threads",
    "zlib",
];

pub fn is_node_builtin(module_name: &str) -> bool {
    let name = module_name.strip_prefix("node:").unwrap_or(module_name);
    let base = name.split('/').next().unwrap_or(name);
    NODE_BUILTIN_MODULES.contains(&base)
}

/// "npm install crypto" -> Some("crypto")
/// "npm install --save express" -> Some("express")
/// "npm i lodash" -> Some("lodash")
/// "npm install" -> None
pub fn extract_npm_package(command: &str) -> Option<&str> {
    let parts: Vec<&str> = command.split_whitespace().collect();
    if parts.first().copied() != Some("npm") {
        return None;
    }

    let install_pos = parts.iter().position(|&p| p == "install" || p == "i")?;

    parts[install_pos + 1..]
        .iter()
        .copied()
        .find(|part| !part.starts_with('-'))
}

/// "npm install crypto" -> Some("crypto")
/// "npm install node:fs" -> Some("fs")
/// "npm install express" -> None
pub fn is_npm_install_builtin(command: &str) -> Option<&str> {
    let pkg = extract_npm_package(command)?;
    let name = pkg.strip_prefix("node:").unwrap_or(pkg);
    let base = name.split('/').next().unwrap_or(name);

    if is_node_builtin(base) {
        Some(base)
    } else {
        None
    }
}

/// Detect pip install commands that forgot the package name.
///
/// Blocked:
/// - "pip install"
/// - "pip3 install"
/// - "venv/bin/pip install"
/// - "python -m pip install"
///
/// Allowed:
/// - "pip install pytest"
/// - "venv/bin/pip install flask"
/// - "python -m pip install requests"
pub fn is_pip_without_package(command: &str) -> bool {
    let parts: Vec<&str> = command.split_whitespace().collect();
    if parts.is_empty() {
        return false;
    }

    let install_index = if let Some(pip_pos) = parts.iter().position(|part| is_pip_token(part)) {
        if parts.get(pip_pos + 1).copied() == Some("install") {
            Some(pip_pos + 1)
        } else {
            None
        }
    } else if parts.len() >= 4
        && is_python_token(parts[0])
        && parts[1] == "-m"
        && is_pip_token(parts[2])
        && parts[3] == "install"
    {
        Some(3)
    } else {
        None
    };

    let Some(install_index) = install_index else {
        return false;
    };

    !parts
        .iter()
        .skip(install_index + 1)
        .any(|arg| !arg.starts_with('-'))
}

fn is_pip_token(part: &str) -> bool {
    part == "pip" || part == "pip3" || part.ends_with("/pip") || part.ends_with("/pip3")
}

fn is_python_token(part: &str) -> bool {
    part == "python" || part == "python3" || part.ends_with("/python") || part.ends_with("/python3")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crypto_is_builtin() {
        assert!(is_node_builtin("crypto"));
    }

    #[test]
    fn node_prefix_crypto_is_builtin() {
        assert!(is_node_builtin("node:crypto"));
    }

    #[test]
    fn fs_promises_is_builtin() {
        assert!(is_node_builtin("fs/promises"));
    }

    #[test]
    fn path_is_builtin() {
        assert!(is_node_builtin("path"));
    }

    #[test]
    fn os_is_builtin() {
        assert!(is_node_builtin("os"));
    }

    #[test]
    fn empty_string_is_not_builtin() {
        assert!(!is_node_builtin(""));
    }

    #[test]
    fn express_is_not_builtin() {
        assert!(!is_node_builtin("express"));
    }

    #[test]
    fn axios_is_not_builtin() {
        assert!(!is_node_builtin("axios"));
    }

    #[test]
    fn lodash_is_not_builtin() {
        assert!(!is_node_builtin("lodash"));
    }

    #[test]
    fn extracts_npm_package_name() {
        assert_eq!(extract_npm_package("npm install crypto"), Some("crypto"));
        assert_eq!(
            extract_npm_package("npm install --save express"),
            Some("express")
        );
        assert_eq!(
            extract_npm_package("npm install --save-dev jest"),
            Some("jest")
        );
        assert_eq!(extract_npm_package("npm i lodash"), Some("lodash"));
        assert_eq!(extract_npm_package("npm install"), None);
        assert_eq!(extract_npm_package("npm install --legacy-peer-deps"), None);
        assert_eq!(extract_npm_package("pip install pytest"), None);
    }

    #[test]
    fn detects_builtin_npm_install() {
        assert_eq!(is_npm_install_builtin("npm install crypto"), Some("crypto"));
        assert_eq!(
            is_npm_install_builtin("npm install node:crypto"),
            Some("crypto")
        );
        assert_eq!(is_npm_install_builtin("npm install path"), Some("path"));
        assert_eq!(is_npm_install_builtin("npm install fs"), Some("fs"));
        assert_eq!(is_npm_install_builtin("npm install express"), None);
        assert_eq!(is_npm_install_builtin("npm install"), None);
    }

    #[test]
    fn detects_pip_without_package() {
        assert!(is_pip_without_package("pip install"));
        assert!(is_pip_without_package("pip3 install"));
        assert!(is_pip_without_package("venv/bin/pip install"));
        assert!(is_pip_without_package("python -m pip install"));
        assert!(!is_pip_without_package("pip install pytest"));
        assert!(!is_pip_without_package("venv/bin/pip install flask"));
        assert!(!is_pip_without_package("python -m pip install requests"));
    }
}
