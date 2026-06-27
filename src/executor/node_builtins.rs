// src/executor/node_builtins.rs
// Node.js built-in modules — لا تحتاج npm install
// المصدر: Node.js 18 LTS API

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
/// "npm i node:fs" -> Some("node:fs")
/// "npm install" -> None
pub fn extract_npm_package(command: &str) -> Option<String> {
    let parts: Vec<&str> = command.split_whitespace().collect();
    if parts.first().copied() != Some("npm") {
        return None;
    }

    let install_pos = parts.iter().position(|&p| p == "install" || p == "i")?;

    parts[install_pos + 1..]
        .iter()
        .find(|&&p| !p.starts_with('-'))
        .map(|s| (*s).to_string())
}

/// "npm install crypto" -> Some("crypto")
/// "npm install node:fs" -> Some("fs")
/// "npm install express" -> None
pub fn is_npm_install_builtin(command: &str) -> Option<String> {
    let pkg = extract_npm_package(command)?;
    let name = pkg.strip_prefix("node:").unwrap_or(&pkg);
    let base = name.split('/').next().unwrap_or(name);

    if is_node_builtin(base) {
        Some(base.to_string())
    } else {
        None
    }
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
        assert_eq!(
            extract_npm_package("npm install crypto"),
            Some("crypto".to_string())
        );
        assert_eq!(
            extract_npm_package("npm install --save express"),
            Some("express".to_string())
        );
        assert_eq!(extract_npm_package("npm install"), None);
        assert_eq!(extract_npm_package("pip install pytest"), None);
    }

    #[test]
    fn detects_builtin_npm_install() {
        assert_eq!(
            is_npm_install_builtin("npm install crypto"),
            Some("crypto".to_string())
        );
        assert_eq!(
            is_npm_install_builtin("npm install node:fs"),
            Some("fs".to_string())
        );
        assert_eq!(is_npm_install_builtin("npm install express"), None);
        assert_eq!(is_npm_install_builtin("npm install"), None);
    }
}
