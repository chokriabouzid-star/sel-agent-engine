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

pub struct WorkspaceOracle {
    pub workspace: PathBuf,
    pub project_type: ProjectType,
}

impl WorkspaceOracle {
    pub fn new(workspace: PathBuf) -> Self {
        let project_type = Self::detect_project_type(&workspace);
        Self {
            workspace,
            project_type,
        }
    }

    fn detect_project_type(path: &Path) -> ProjectType {
        if path.join("Cargo.toml").exists() {
            ProjectType::Rust
        } else if path.join("go.mod").exists() {
            ProjectType::Go
        } else if path.join("package.json").exists() {
            ProjectType::Node
        } else if path.join("setup.py").exists() 
            || path.join("pyproject.toml").exists() 
            || path.join("requirements.txt").exists() 
            || path.join("venv").exists() 
        {
            ProjectType::Python
        } else {
            ProjectType::Unknown
        }
    }

    /// التحقق مما إذا كان امتداد الملف مسموحاً به في مشروع من هذا النوع
    pub fn is_ext_allowed(&self, ext: &str) -> Result<(), String> {
        let allowed = match self.project_type {
            ProjectType::Rust => ["rs", "toml", "md"].contains(&ext),
            ProjectType::Go => ["go", "mod", "sum", "sh", "md"].contains(&ext),
            ProjectType::Node => ["js", "ts", "tsx", "json", "md"].contains(&ext),
            ProjectType::Python => ["py", "txt", "cfg", "toml", "ini", "md", "sql"].contains(&ext),
            ProjectType::Unknown => true, // في المشاريع غير المحددة، لا نفرض قيوداً صارمة حالياً
        };

        if allowed {
            Ok(())
        } else {
            Err(format!(
                "LANGUAGE LOCK BLOCKED: Cannot operate on '.{}' file in a {:?} workspace.",
                ext, self.project_type
            ))
        }
    }

    /// حل الأمر الخاص بالاختبارات بناءً على حقيقة المشروع لا اقتراح الـ LLM فقط
    pub fn resolve_test_command(&self, target: &str) -> (String, Vec<String>) {
        let t = target.to_lowercase();
        
        match self.project_type {
            ProjectType::Go => {
                // إذا حاول الـ LLM طلب cargo في بيئة Go (كما حدث في Tier 3)
                if t.contains("cargo") {
                    return ("go".to_string(), vec!["test".to_string(), "./...".to_string(), "-v".to_string()]);
                }
                ("go".to_string(), vec!["test".to_string(), "./...".to_string(), "-v".to_string()])
            }
            ProjectType::Rust => {
                ("cargo".to_string(), vec!["test".to_string(), "--".to_string(), "--nocapture".to_string()])
            }
            ProjectType::Node => {
                // توجيه ذكي لـ Jest/NPM
                if t.contains("jest") || t.ends_with(".ts") || t.ends_with(".js") {
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
            _ => (target.to_string(), vec![])
        }
    }

    /// التحقق مما إذا كان الأمر المقترح من الـ LLM يتناسب مع نوع المشروع
    pub fn validate_plan_cmd(&self, cmd: &crate::protocol::Cmd) -> Result<(), String> {
        use crate::protocol::Cmd;
        match cmd {
            Cmd::Run { command } => {
                let lc = command.to_lowercase();
                match self.project_type {
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
