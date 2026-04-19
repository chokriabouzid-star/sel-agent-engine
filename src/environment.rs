use std::process::Command;

#[derive(Debug, Clone)]
pub struct PythonInfo {
    pub cmd: String,
    pub version: String,
    pub venv: bool,
    pub pip: bool,
}

#[derive(Debug, Clone)]
pub struct ToolInfo {
    pub cmd: String,
    pub version: String,
}

#[derive(Debug, Clone, Default)]
pub struct EnvironmentCapabilities {
    pub python: Option<PythonInfo>,
    pub node: Option<ToolInfo>,
    pub rust: Option<ToolInfo>,
    pub go: Option<ToolInfo>,
    pub git: Option<ToolInfo>,
}

impl EnvironmentCapabilities {
    pub fn probe() -> Self {
        Self {
            python: probe_python(),
            node: probe_tool("node", &["--version"]),
            rust: probe_tool("cargo", &["--version"]),
            go: probe_tool("go", &["version"]),
            git: probe_tool("git", &["--version"]),
        }
    }

    pub fn to_planning_context(&self) -> String {
        let mut lines = vec!["[ENVIRONMENT CAPABILITIES]".to_string()];
        match &self.python {
            Some(p) => lines.push(format!(
                "- Python: {} ({}) | venv:{} pip:{}",
                p.cmd, p.version, p.venv, p.pip
            )),
            None => lines.push("- Python: NOT AVAILABLE".to_string()),
        }
        match &self.node {
            Some(t) => lines.push(format!("- Node.js: available ({})", t.version)),
            None => lines.push("- Node.js: NOT AVAILABLE".to_string()),
        }
        match &self.rust {
            Some(t) => lines.push(format!("- Rust/cargo: available ({})", t.version)),
            None => lines.push("- Rust/cargo: NOT AVAILABLE".to_string()),
        }
        match &self.go {
            Some(t) => lines.push(format!("- Go: available ({})", t.version)),
            None => lines.push("- Go: NOT AVAILABLE".to_string()),
        }
        match &self.git {
            Some(t) => lines.push(format!("- Git: available ({})", t.version)),
            None => lines.push("- Git: NOT AVAILABLE".to_string()),
        }
        lines.join("\n")
    }

    pub fn derive_constraints(&self) -> String {
        let mut lines = vec!["[CONSTRAINTS]".to_string()];
        match &self.python {
            Some(p) => {
                lines.push(format!(
                    "- Use \"{}\" for Python commands (confirmed available)",
                    p.cmd
                ));
                if p.venv {
                    lines.push(format!(
                        "- Use \"{} -m venv venv\" directly — venv module confirmed",
                        p.cmd
                    ));
                } else {
                    lines.push("- venv NOT available — do not plan venv commands".to_string());
                }
            }
            None => {
                lines.push("- Python NOT available — do not plan any python commands".to_string())
            }
        }
        if self.node.is_none() {
            lines.push("- Node.js NOT available — do not plan npm/node commands".to_string());
        }
        if self.go.is_none() {
            lines.push("- Go NOT available — do not plan go commands".to_string());
        }
        lines.join("\n")
    }
}

fn probe_python() -> Option<PythonInfo> {
    for cmd in &["python3", "python"] {
        if let Ok(out) = Command::new(cmd).arg("--version").output() {
            if out.status.success() {
                let raw = String::from_utf8_lossy(&out.stdout);
                let version = raw.trim().to_string();
                let venv = Command::new(cmd)
                    .args(["-m", "venv", "--help"])
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false);
                let pip = Command::new(cmd)
                    .args(["-m", "pip", "--version"])
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false);
                return Some(PythonInfo {
                    cmd: cmd.to_string(),
                    version,
                    venv,
                    pip,
                });
            }
        }
    }
    None
}

fn probe_tool(cmd: &str, args: &[&str]) -> Option<ToolInfo> {
    Command::new(cmd).args(args).output().ok().and_then(|out| {
        if out.status.success() {
            let raw = String::from_utf8_lossy(&out.stdout);
            let version = raw.lines().next().unwrap_or("?").trim().to_string();
            Some(ToolInfo {
                cmd: cmd.to_string(),
                version,
            })
        } else {
            None
        }
    })
}
