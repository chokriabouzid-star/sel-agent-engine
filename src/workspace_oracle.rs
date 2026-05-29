use std::path::{Path, PathBuf};
use anyhow::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectType {
    Rust,
    Go,
    Node,
    Python,
    Unknown,
}

impl ProjectType {
    pub fn as_str(&self) -> &str {
        match self {
            ProjectType::Rust => "Rust",
            ProjectType::Go => "Go",
            ProjectType::Node => "Node.js",
            ProjectType::Python => "Python",
            ProjectType::Unknown => "Unknown",
        }
    }
}

pub struct WorkspaceOracle {
    pub workspace: PathBuf,
}

impl WorkspaceOracle {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }

    pub fn current_type(&self) -> ProjectType {
        Self::detect_project_type(&self.workspace)
    }

    fn detect_project_type(path: &Path) -> ProjectType {
        if path.join("Cargo.toml").exists() {
            ProjectType::Rust
        } else if path.join("go.mod").exists()
            || std::fs::read_dir(path).map(|dir| {
                dir.filter_map(Result::ok)
                   .any(|e| e.path().extension().is_some_and(|ext| ext == "go"))
            }).unwrap_or(false)
        {
            ProjectType::Go
        } else if path.join("package.json").exists() {
            ProjectType::Node
        } else if path.join("setup.py").exists() 
            || path.join("pyproject.toml").exists() 
            || path.join("requirements.txt").exists() 
            || path.join("venv").exists()
            || std::fs::read_dir(path).map(|dir| {
                dir.filter_map(Result::ok)
                   .any(|e| e.path().extension().is_some_and(|ext| ext == "py"))
            }).unwrap_or(false)
        {
            ProjectType::Python
        } else {
            ProjectType::Unknown
        }
    }

    /// Checks whether the given file extension is allowed in the detected project type.
    pub fn is_ext_allowed(&self, ext: &str) -> Result<(), String> {
        let p_type = self.current_type();
        let allowed = match p_type {
            ProjectType::Rust => ["rs", "toml", "md"].contains(&ext),
            ProjectType::Go => ["go", "mod", "sum", "sh", "md"].contains(&ext),
            ProjectType::Node => ["js", "ts", "tsx", "json", "md"].contains(&ext),
            ProjectType::Python => ["py", "txt", "cfg", "toml", "ini", "md", "sql"].contains(&ext),
            ProjectType::Unknown => true,
        };

        if allowed {
            Ok(())
        } else {
            Err(format!(
                "LANGUAGE LOCK BLOCKED: Cannot operate on '.{}' file in a {:?} workspace.",
                ext, p_type
            ))
        }
    }

    ///            LLM 
    pub fn resolve_test_command(&self, target: &str) -> (String, Vec<String>) {
        let t = target.to_lowercase();
        let p_type = self.current_type();
        
        match p_type {
            ProjectType::Go => {
                if t.contains("cargo") {
                    return ("go".to_string(), vec!["test".to_string(), "./...".to_string(), "-v".to_string()]);
                }
                ("go".to_string(), vec!["test".to_string(), "./...".to_string(), "-v".to_string()])
            }
            ProjectType::Rust => {
                ("cargo".to_string(), vec!["test".to_string(), "--".to_string(), "--nocapture".to_string()])
            }
            ProjectType::Node => {
                if t.contains("jest") || t.ends_with(".ts") || t.ends_with(".js") || t.contains("npm ") || t.contains("test") {
                    ("npx".to_string(), vec!["jest".to_string(), "--runInBand".to_string(), "--forceExit".to_string()])
                } else {
                    ("npm".to_string(), vec!["test".to_string(), "--".to_string(), "--runInBand".to_string(), "--forceExit".to_string()])
                }
            }
            ProjectType::Python => {
                let pytest = if self.workspace.join("venv/bin/pytest").exists() {
                    "venv/bin/pytest"
                } else {
                    "pytest"
                };
                (pytest.to_string(), vec!["-v".to_string(), "--tb=short".to_string()])
            }
            _ => {
                // v7.3.6: Smart split for compound commands in unknown projects
                let parts: Vec<String> = target.split_whitespace().map(|s| s.to_string()).collect();
                if parts.is_empty() {
                    match Self::detect_project_type(&self.workspace) {
                        ProjectType::Go => (
                            "go".to_string(),
                            vec!["test".to_string(), "./...".to_string(), "-v".to_string()],
                        ),
                        ProjectType::Rust => (
                            "cargo".to_string(),
                            vec!["test".to_string(), "--".to_string(), "--nocapture".to_string()],
                        ),
                        ProjectType::Node => (
                            "npm".to_string(),
                            vec!["test".to_string(), "--".to_string(), "--runInBand".to_string(), "--forceExit".to_string()],
                        ),
                        ProjectType::Python => {
                            let pytest = if self.workspace.join("venv/bin/pytest").exists() {
                                "venv/bin/pytest"
                            } else {
                                "pytest"
                            };
                            (pytest.to_string(), vec!["-v".to_string(), "--tb=short".to_string()])
                        }
                        ProjectType::Unknown => (target.to_string(), vec![]),
                    }
                } else {
                    let prog = parts[0].clone();
                    let args = parts[1..].to_vec();
                    if prog == "go" && (args.is_empty() || args == vec!["test"]) {
                        ("go".to_string(), vec!["test".to_string(), "./...".to_string(), "-v".to_string()])
                    } else {
                        (prog, args)
                    }
                }
            }
        }
    }

    ///         LLM    
    pub fn validate_plan_cmd(&self, cmd: &crate::protocol::Cmd) -> Result<(), String> {
        use crate::protocol::Cmd;
        let p_type = self.current_type();
        match cmd {
            Cmd::Run { command } => {
                let lc = command.to_lowercase();
                match p_type {
                    ProjectType::Rust if lc.contains("go test") || lc.contains("pytest") || lc.contains("npm ") => {
                        Err("Plan contains non-Rust commands in a Rust project.".to_string())
                    }
                    ProjectType::Go if lc.contains("cargo ") || lc.contains("pytest") || lc.contains("npm ") => {
                        Err("Plan contains non-Go commands in a Go project.".to_string())
                    }
                    ProjectType::Node if lc.contains("cargo ") || lc.contains("go test") || lc.contains("pytest") => {
                        Err("Plan contains non-Node commands in a Node.js project.".to_string())
                    }
                    ProjectType::Python if lc.contains("cargo ") || lc.contains("go test") || lc.contains("npm ") => {
                        Err("Plan contains non-Python commands in a Python project.".to_string())
                    }
                    _ => Ok(()),
                }
            }
            _ => Ok(()),
        }
    }
}
