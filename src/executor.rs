// src/executor.rs — v0.4: تنفيذ آمن
use std::collections::HashMap;

use anyhow::{anyhow, Result};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::process::Command as TCmd;
use crate::types::{ExecResult, SafetyError};
use crate::protocol::Cmd;
use crate::workspace_oracle::WorkspaceOracle;

const ALLOWED: &[&str] = &[
    "python3", "python",
    "venv/bin/python3", "venv/bin/python",
    "venv/bin/pip3",    "venv/bin/pip",
    "venv/bin/uvicorn", "venv/bin/gunicorn",
    "venv/bin/pytest",  "pytest",
    "node", "npm", "npx", "node_modules/.bin/jest",
    "cargo", "rustc", "git",
    "go",
    "mkdir", "touch", "ls", "cat", "cp", "mv",
    "echo", "find", "grep", "curl", "chmod",
    "node", "npm",
];

const BLOCKED: &[&str] = &[
    "sudo", "rm -rf", "mkfs", "dd if=",
    "| sh", "| bash", "curl | bash",
    "> /dev/", "/etc/", "/sys/", "/proc/",
];

pub struct SafeExecutor {
    pub workspace: PathBuf,
    pub oracle: WorkspaceOracle,
    timeout_secs: u64,
    patch_attempts: std::cell::RefCell<HashMap<PathBuf, usize>>, // v5.2: track patch failures
}

fn fix_rust_string_literals(src: &str) -> String {
    let mut result = String::with_capacity(src.len());
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\'' {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end] != b'\'' && bytes[end] != b'\n' {
                end += 1;
            }
            if end < bytes.len() && bytes[end] == b'\'' && end > start + 1 {
                let word = &src[start..end];
                if word.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
                    result.push('"');
                    result.push_str(word);
                    result.push('"');
                    i = end + 1;
                    continue;
                }
            }
        }
        let ch = src[i..].chars().next().unwrap();
        result.push(ch);
        i += ch.len_utf8();
    }
    result
}


impl SafeExecutor {
    pub fn new(workspace: PathBuf, timeout_secs: u64) -> Self {
        let oracle = WorkspaceOracle::new(workspace.clone());
        Self { 
            workspace, 
            oracle,
            timeout_secs,
            patch_attempts: std::cell::RefCell::new(HashMap::new()),
        }
    }

    pub async fn run(&self, cmd: &Cmd) -> Result<ExecResult> {
        match cmd {
            Cmd::Run       { command }         => self.shell(command).await,
            Cmd::WriteFile { path, content }   => self.write_file(path, content),
            Cmd::AppendFile{ path, content }   => self.append_file(path, content),
            Cmd::DeleteFile{ path }               => self.delete_file(path),
            Cmd::PatchFile { path, search, replace } => self.patch_file(path, search, replace),
            Cmd::ReadFile  { path }            => self.read_file(path),
            Cmd::Mkdir     { path }            => self.mkdir(path),
            Cmd::RunTests  { target }          => self.run_tests(target).await,
            Cmd::Done      { .. }              => Ok(ExecResult::ok("done")),
        }
    }

    // ─── Shell ─────────────────────────────────────

    async fn shell(&self, command: &str) -> Result<ExecResult> {
        self.safety_check(command)?;

        let parts: Vec<&str> = command.split_whitespace().collect();
        let prog = parts.first().ok_or_else(|| anyhow!("Empty command"))?;

        // رفض pip install بدون package name
        if prog.contains("pip3") || prog.contains("pip") {
            let is_install = parts.iter().any(|p| *p == "install");
            let has_package = parts.len() > 2 && parts.iter().skip(2).any(|p| !p.starts_with('-'));
            if is_install && !has_package {
                return Ok(ExecResult::fail(
                    "pip install needs package name: e.g. venv/bin/pip3 install pytest".to_string()
                ));
            }
        }

        if !ALLOWED.iter().any(|a| *a == *prog) {
            return Ok(ExecResult::fail(format!(
                "'{}' is not in the allowed programs list", prog
            )));
        }

        let services = ["venv/bin/uvicorn", "uvicorn", "venv/bin/gunicorn"];
        if services.contains(prog) {
            return self.service(prog, &parts[1..]).await;
        }

        let start = Instant::now();
        let out = tokio::time::timeout(
            Duration::from_secs(self.timeout_secs),
            TCmd::new(prog).args(&parts[1..]).current_dir(&self.workspace).output(),
        ).await
        .map_err(|_| anyhow!("Timeout after {}s: {}", self.timeout_secs, command))??;

        Ok(ExecResult {
            success:     out.status.success(),
            exit_code:   out.status.code().unwrap_or(-1),
            stdout:      String::from_utf8_lossy(&out.stdout).into(),
            stderr:      String::from_utf8_lossy(&out.stderr).into(),
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }

    async fn service(&self, prog: &str, args: &[&str]) -> Result<ExecResult> {
        println!("   🌐 Service: {}", prog);
        TCmd::new(prog).args(args).current_dir(&self.workspace).spawn()?;
        tokio::time::sleep(Duration::from_millis(800)).await;
        Ok(ExecResult::ok("Service started"))
    }

    // ─── File Operations ───────────────────────────

    fn write_file(&self, path: &str, content: &str) -> Result<ExecResult> {
        {
            let ext = std::path::Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("");
            let is_config = matches!(path, "Cargo.toml" | "go.mod" | "go.sum" | "package.json"
                | "tsconfig.json" | "Makefile" | ".gitignore" | "README.md");

            if !is_config && !ext.is_empty() {
                if let Err(e) = self.oracle.is_ext_allowed(ext) {
                    return Ok(ExecResult::fail(e));
                }
            }
        }
        let p = self.safe_path(path)?;
        // حماية: ملفات محمية لا يُكتب عليها إذا كانت موجودة
        let protected = ["Cargo.toml", "Cargo.lock", "go.mod", "go.sum"];
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if protected.contains(&name) && p.exists() {
            return Ok(ExecResult::fail(format!(
                "write_file: '{}' is protected — use patch_file to modify existing files", path
            )));
        }
        if let Some(parent) = p.parent() { std::fs::create_dir_all(parent)?; }
        // حماية: إذا كان الملف موجوداً وأكبر بكثير من المحتوى الجديد → تحذير
        if p.exists() {
            let existing_len = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            let new_len = content.len() as u64;
            if existing_len > 500 && new_len < existing_len / 3 {
                println!("   ⚠ WARNING: Overwriting {} ({} bytes) with much smaller content ({} bytes)",
                    path, existing_len, new_len);
            }
        }
        // Rust brace balance check
        let content = if path.ends_with(".rs") {
            let open  = content.chars().filter(|&c| c == '{').count();
            let close = content.chars().filter(|&c| c == '}').count();
            if open > close {
                let mut fixed = content.to_string();
                for _ in 0..(open - close) { fixed.push_str("
}"); }
                std::borrow::Cow::Owned(fixed)
            } else {
                std::borrow::Cow::Borrowed(content)
            }
        } else {
            std::borrow::Cow::Borrowed(content)
        };
        // v5.5: auto-fix single-quote string literals in Rust files
        let content = if p.extension().map(|x| x == "rs").unwrap_or(false) {
            std::borrow::Cow::Owned(fix_rust_string_literals(content.as_ref()))
        } else {
            content
        };
        // v7.3: sanitize Unicode quotes قبل الكتابة
        let content_str = sanitize_code(content.as_ref());
        eprintln!("[TRACE] write_file sanitize: input={} output={}", content.as_ref().len(), content_str.len());
        std::fs::write(&p, content_str.as_bytes())?;
        // Auto-fix: إذا كُتب jest.config.js → احذف "jest" field من package.json
        if path.ends_with("jest.config.js") {
            let pkg = self.workspace.join("package.json");
            if pkg.exists() {
                if let Ok(pkg_src) = std::fs::read_to_string(&pkg) {
                    if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&pkg_src) {
                        if v.get("jest").is_some() {
                            v.as_object_mut().unwrap().remove("jest");
                            if let Ok(fixed) = serde_json::to_string_pretty(&v) {
                                let _ = std::fs::write(&pkg, fixed);
                                println!("   🔧 AutoFix: removed jest field from package.json (conflicts with jest.config.js)");
                            }
                        }
                    }
                }
            }
        }
        println!("   📝 {} ({} bytes)", path, content.len());
        // v7.3: compile check فوري بعد write_file
        if path.ends_with(".go") {
            // AutoFix: undefined Go stdlib import بدون LLM
            if let Some(err) = go_compile_check(&self.workspace) {
                // حاول AutoFix أولاً
                eprintln!("[TRACE] Checking autofix for: {}", &err[..std::cmp::min(80, err.len())]);
                if let Some(fixed) = autofix_go_undefined_import(&p, &err) {
                    println!("   🔧 AutoFix Go import: {}", fixed);
                    // أعد الفحص بعد الإصلاح
                    if go_compile_check(&self.workspace).is_none() {
                        println!("   ✅ AutoFix succeeded");
                    } else if let Some(err2) = go_compile_check(&self.workspace) {
                        return Ok(ExecResult::fail(format!(
                            "COMPILE ERROR in '{}' — NOTE: The actual error might be in a DIFFERENT file. Check the error details below and fix the file mentioned there:\n{}",
                            path, err2
                        )));
                    }
                } else {
                    return Ok(ExecResult::fail(format!(
                        "COMPILE ERROR in '{}' — NOTE: The actual error might be in a DIFFERENT file. Check the error details below and fix the file mentioned there:\n{}",
                        path, err
                    )));
                }
            }
        }
        Ok(ExecResult::ok(format!("Written: {}", path)))
    }

    fn append_file(&self, path: &str, content: &str) -> Result<ExecResult> {
        let p = self.safe_path(path)?;
        if !p.exists() {
            return Ok(ExecResult::fail(format!("'{}' not found — use write_file first", path)));
        }
        let mut orig = std::fs::read_to_string(&p)?;
        if !orig.ends_with('\n') { orig.push('\n'); }
        orig.push('\n');
        orig.push_str(content);
        std::fs::write(&p, &orig)?;
        println!("   ➕ {} (+{} bytes)", path, content.len());
        Ok(ExecResult::ok(format!("Appended: {}", path)))
    }

    fn delete_file(&self, path: &str) -> Result<ExecResult> {
        let p = self.safe_path(path)?;
        // حماية: لا نحذف ملفات الإعداد الجذرية
        let protected = ["Cargo.toml", "go.mod", "package.json", "Cargo.lock"];
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if protected.contains(&name) {
            return Ok(ExecResult::fail(format!(
                "delete_file: '{}' is protected and cannot be deleted", path
            )));
        }
        // حماية: لا نحذف مجلدات
        if p.is_dir() {
            return Ok(ExecResult::fail(format!(
                "delete_file: '{}' is a directory — only files allowed", path
            )));
        }
        if !p.exists() {
            return Ok(ExecResult::ok(format!("delete_file: '{}' already absent", path)));
        }
        std::fs::remove_file(&p)?;
        println!("   🗑  Deleted: {}", path);
        Ok(ExecResult::ok(format!("Deleted: {}", path)))
    }


    // v5.2: Validate patch result to prevent code corruption
    fn validate_patch(&self, path: &str, original: &str, patched: &str) -> Result<(), String> {
        // 1. Sanity checks for common corruption patterns
        if patched.contains("}ype") || patched.contains("{ype") {
            return Err("Suspicious patch: corrupted type keyword detected".to_string());
        }
        
        if patched.contains("#\\[") || patched.contains("#\\]") {
            return Err("Invalid Rust escape: backslash in attribute syntax detected".to_string());
        }
        
        // 2. Line count sanity check
        let orig_lines = original.lines().count();
        let new_lines = patched.lines().count();
        let diff = (new_lines as i32 - orig_lines as i32).abs();
        
        let max_allowed = (orig_lines * 2).max(50) as i32;
        if diff > max_allowed {
            return Err(format!("Patch changed too many lines: {} → {} lines", orig_lines, new_lines));
        }
        
        // 3. Rust-specific checks
        if path.ends_with(".rs") {
            // Check for unmatched braces (basic)
            let open_braces = patched.matches('{').count();
            let close_braces = patched.matches('}').count();
            if open_braces != close_braces {
                return Err(format!("Unmatched braces: {} open, {} close", open_braces, close_braces));
            }
        }
        
        Ok(())
    }



    fn patch_file(&self, path: &str, search: &str, replace: &str) -> Result<ExecResult> {
        {
            let ext = std::path::Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("");
            let is_config = matches!(path, "Cargo.toml" | "go.mod" | "go.sum" | "package.json"
                | "tsconfig.json" | "Makefile" | ".gitignore" | "README.md");

            if !is_config && !ext.is_empty() {
                if let Err(e) = self.oracle.is_ext_allowed(ext) {
                    return Ok(ExecResult::fail(e));
                }
            }
        }
        let p = self.safe_path(path)?;
        
        // v7.3.3: Pre-flight check for BUG-PATCH-01
        if !p.exists() {
            return Ok(ExecResult::fail(format!(
                "PREFLIGHT_FAIL: '{}' does not exist. Use write_file to create it first, then patch.",
                path
            )));
        }

        // v5.2: Fallback to write_file after 2 failed patch attempts
        {
            let mut attempts = self.patch_attempts.borrow_mut();
            let count = *attempts.entry(p.clone()).or_insert(0);
            
            if count >= 2 {
                println!("   ⚠️  patch_file failed {} times on '{}' — switching to write_file fallback", count, path);
                drop(attempts); // release borrow
                
                // Read current content and apply replacement manually
                let content = std::fs::read_to_string(&p)?;
                let content = sanitize_code(&content);
                let search_buf = sanitize_code(search);
                let search = search_buf.as_str();
                let replace_buf = sanitize_code(replace);
                let replace = replace_buf.as_str();
                // FIX(Opus): تحقق من وجود search قبل الكتابة — منع Silent Corruption
                if !content.contains(search) {
                    self.patch_attempts.borrow_mut().insert(p.clone(), 0);
                    return Ok(ExecResult::fail(format!(
                        "PATCH FAILED: search block not found in '{}' after {} attempts — use write_file with complete file content",
                        path, count
                    )));
                }
                let new_content = content.replacen(search, replace, 1);
                let new_content = sanitize_code(&new_content);
                std::fs::write(&p, new_content.as_bytes())?;
                self.patch_attempts.borrow_mut().insert(p.clone(), 0);
                return Ok(ExecResult::ok(format!(
                    "patch_file: fallback write_file applied to '{}' successfully",
                    path
                )));
            }
        }
        if search.trim().is_empty() {
            return Ok(ExecResult::fail("patch_file: search block is empty".to_string()));
        }
        let content = std::fs::read_to_string(&p)?;
        let content = sanitize_code(&content);
        let count = content.matches(search).count();
        // إذا لم يُوجد مباشرة — جرب normalize whitespace
        let (effective_search, effective_replace, normalized) = if count == 0 {
            let norm_content = content.split_whitespace().collect::<Vec<_>>().join(" ");
            let norm_search  = search.split_whitespace().collect::<Vec<_>>().join(" ");
            let norm_replace = replace.split_whitespace().collect::<Vec<_>>().join(" ");
            (norm_content, norm_replace, Some(norm_search))
        } else {
            (content.clone(), replace.to_string(), None)
        };
        let (search_key, content_key) = if let Some(ref ns) = normalized {
            (ns.as_str(), effective_search.as_str())
        } else {
            (search, content.as_str())
        };
        let count = content_key.matches(search_key).count();
        if count == 0 {
            // v5.2: increment failure counter
            *self.patch_attempts.borrow_mut().entry(p.clone()).or_insert(0) += 1;
            return Ok(ExecResult::fail(format!(
                "patch_file: search block not found in '{}' (tried exact + whitespace-normalized) — copy the exact text from the file", path
            )));
        }
        if count > 1 {
            return Ok(ExecResult::fail(format!(
                "patch_file: search block found {} times in '{}' — must be unique, use more context", count, path
            )));
        }
        let new_content = if normalized.is_some() {
            content_key.replacen(search_key, &effective_replace, 1)
        } else {
            content.replacen(search, replace, 1)
        };
        // v5.2: Validate before writing
        if let Err(e) = self.validate_patch(path, &content, &new_content) {
            *self.patch_attempts.borrow_mut().entry(p.clone()).or_insert(0) += 1;
            return Ok(ExecResult::fail(format!("patch_file validation failed: {}", e)));
        }
        
        // v5.5: auto-fix single-quote string literals in Rust files
        let new_content = if p.extension().map(|x| x == "rs").unwrap_or(false) {
            fix_rust_string_literals(&new_content)
        } else {
            new_content
        };
        std::fs::write(&p, &new_content)?;

        // v5.2: reset counter on success
        self.patch_attempts.borrow_mut().insert(p.clone(), 0);
        
        println!("   🔧 patch_file: {} ({} bytes → {} bytes)", path, content.len(), new_content.len());
        Ok(ExecResult::ok(format!("Patched: {}", path)))
    }

    fn read_file(&self, path: &str) -> Result<ExecResult> {
        let p = self.safe_path(path)?;
        if !p.exists() {
            return Ok(ExecResult::fail(format!("'{}' not found", path)));
        }
        let content = std::fs::read_to_string(&p)?;
        println!("   📖 {} ({} bytes)", path, content.len());
        Ok(ExecResult { success: true, exit_code: 0, stdout: content, stderr: String::new(), duration_ms: 0 })
    }

    fn mkdir(&self, path: &str) -> Result<ExecResult> {
        std::fs::create_dir_all(self.workspace.join(path))?;
        Ok(ExecResult::ok(format!("mkdir: {}", path)))
    }

    // ─── RunTests ──────────────────────────────────

    async fn run_tests(&self, target: &str) -> Result<ExecResult> {
        let (prog, args) = self.oracle.resolve_test_command(target);
        let start = std::time::Instant::now();
        
        println!("   🚀 Running tests via Oracle: {} {}", prog, args.join(" "));

        // --- RUST ---
        if prog == "cargo" {
            let out = tokio::time::timeout(
                std::time::Duration::from_secs(self.timeout_secs),
                TCmd::new("cargo")
                    .args(["test", "--", "--nocapture"])
                    .current_dir(&self.workspace)
                    .output(),
            ).await
            .map_err(|_| anyhow!("cargo test timeout"))??;
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let combined = format!("{}\n{}", stdout, stderr);
            
            // v1.4 Digest AutoFix logic (kept for robustness)
            if combined.contains("trait `Digest` which provides") {
                println!("   🔧 AutoFix: adding sha2::Digest import");
                for dir in [self.workspace.as_path(), self.workspace.join("src").as_path()] {
                    if let Ok(entries) = std::fs::read_dir(dir) {
                        for entry in entries.flatten() {
                            let p = entry.path();
                            if p.extension().and_then(|e| e.to_str()) == Some("rs") {
                                if let Ok(content) = std::fs::read_to_string(&p) {
                                    if content.contains("sha2::Sha256") && !content.contains("use sha2::Digest") {
                                        let _ = std::fs::write(&p, format!("use sha2::Digest;\n{}", content));
                                        println!("   ✅ Fixed {:?}", p.file_name().unwrap_or_default());
                                    }
                                }
                            }
                        }
                    }
                }
            }
            let exit_ok = out.status.success();
            let (passed, failed) = parse_rust_tests(&combined);
            let success = exit_ok && passed > 0;
            return Ok(ExecResult {
                success,
                exit_code: out.status.code().unwrap_or(-1),
                stdout: format!("{} passed, {} failed", passed, failed),
                stderr: if success { String::new() } else {
                    let s = combined.len().saturating_sub(2000);
                    combined[s..].to_string()
                },
                duration_ms: start.elapsed().as_millis() as u64,
            });
        }

        // --- GO ---
        if prog == "go" {
            if self.workspace.join("go.mod").exists() {
                let _ = TCmd::new("go").args(["mod", "tidy"]).current_dir(&self.workspace).output().await;
            }
            let out = tokio::time::timeout(
                std::time::Duration::from_secs(self.timeout_secs),
                TCmd::new("go").args(&args).current_dir(&self.workspace).output(),
            ).await.map_err(|_| anyhow!("go test timeout"))??;
            let combined = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            let exit_ok = out.status.success();
            let (passed, failed) = parse_go_tests(&combined);
            let success = exit_ok && passed > 0;
            return Ok(ExecResult {
                success,
                exit_code: out.status.code().unwrap_or(-1),
                stdout: format!("{} passed, {} failed", passed, failed),
                stderr: if success { String::new() } else {
                    let s = combined.len().saturating_sub(2000);
                    combined[s..].to_string()
                },
                duration_ms: start.elapsed().as_millis() as u64,
            });
        }

        // --- NODE / TS ---
        if prog == "npm" || prog == "npx" || prog == "node" {
            let out = tokio::time::timeout(
                std::time::Duration::from_secs(self.timeout_secs),
                TCmd::new(prog).args(&args).current_dir(&self.workspace).output(),
            ).await.map_err(|_| anyhow!("Node.js test timeout"))??;
            let combined = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            let exit_ok = out.status.success();
            let passed = combined.lines().filter(|l| l.contains("✓") || l.contains("✔") || l.contains("passed")).count();
            let failed = combined.lines().filter(|l| l.contains("✗") || l.contains("✘") || l.contains("failed") || l.contains("FAIL")).count();
            let success = exit_ok && passed > 0;
            return Ok(ExecResult {
                success,
                exit_code: out.status.code().unwrap_or(-1),
                stdout: format!("{} passed, {} failed", passed, failed),
                stderr: if success { String::new() } else {
                    let s = combined.len().saturating_sub(2000);
                    combined[s..].to_string()
                },
                duration_ms: start.elapsed().as_millis() as u64,
            });
        }

        // --- PYTHON ---
        if prog.contains("pytest") {
            // v5.6 Auto-create venv if missing
            if !self.workspace.join("venv").exists() {
                let _ = TCmd::new("python3").args(["-m", "venv", "venv"]).current_dir(&self.workspace).output().await;
            }
            let out = tokio::time::timeout(
                std::time::Duration::from_secs(self.timeout_secs),
                TCmd::new(&prog).args(&args).current_dir(&self.workspace).env("PYTHONPATH", &self.workspace).output(),
            ).await.map_err(|_| anyhow!("pytest timeout"))??;
            let combined = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            let (passed, failed) = parse_pytest(&combined);
            let has_error = combined.contains("ERROR collecting") || combined.contains("no tests ran");
            let success = passed > 0 && failed == 0 && !has_error;
            return Ok(ExecResult {
                success,
                exit_code: out.status.code().unwrap_or(-1),
                stdout: format!("{} passed, {} failed", passed, failed),
                stderr: if success { String::new() } else {
                    let s = combined.len().saturating_sub(2000);
                    combined[s..].to_string()
                },
                duration_ms: start.elapsed().as_millis() as u64,
            });
        }

        Ok(ExecResult::fail(format!("No test handler for program: {}", prog)))
    }

    // ─── Helpers ───────────────────────────────────

    fn safe_path(&self, path: &str) -> Result<PathBuf> {
        if path.contains("..") {
            return Err(anyhow!(SafetyError::PathTraversal(path.to_string()).to_string()));
        }
        let full = self.workspace.join(path);
        if !full.starts_with(&self.workspace) {
            return Err(anyhow!(SafetyError::WorkspaceEscape(path.to_string()).to_string()));
        }
        Ok(full)
    }

    fn safety_check(&self, cmd: &str) -> Result<()> {
        let lower = cmd.to_lowercase();
        for b in BLOCKED {
            if lower.contains(b) {
                return Err(anyhow!(SafetyError::BlockedCommand(b.to_string()).to_string()));
            }
        }
        Ok(())
    }
}

fn parse_pytest(output: &str) -> (usize, usize) {
    let mut passed = 0;
    let mut failed = 0;
    for line in output.lines().rev() {
        if line.contains(" passed") || line.contains(" failed") {
            // السطر: "=== 2 passed, 1 failed in 0.03s ==="
            // نبحث عن الرقم قبل كل كلمة مفتاحية
            for seg in line.split(',') {
                let s = seg.trim();
                let words: Vec<&str> = s.split_whitespace().collect();
                for (i, w) in words.iter().enumerate() {
                    if *w == "passed" && i > 0 {
                        if let Ok(n) = words[i-1].parse::<usize>() { passed = n; }
                    }
                    if *w == "failed" && i > 0 {
                        if let Ok(n) = words[i-1].parse::<usize>() { failed = n; }
                    }
                }
            }
            break;
        }
    }
    (passed, failed)
}


#[derive(Debug, PartialEq)]
pub enum MutationResult {
    Strong,
    Weak(String, String),  // (original_line, mutated_line)
    Skipped,
}

fn apply_all_mutations(code: &str) -> Vec<(String, String, String)> {
    let strategies: &[(&str, &str)] = &[
        ("==", "!="), ("!=", "=="),
        (" > ", " < "), (" < ", " > "),
        (" >= ", " <= "), (" <= ", " >= "),
        ("return True", "return False"), ("return False", "return True"),
        (" + ", " - "), (" - ", " + "),
    ];
    let skip_patterns = ["i += ", "i -= ", "j += ", "j -= ",
                          "idx", "index", "len(", "range(", "count +=", "count -="];
    let mut result = Vec::new();
    for (from, to) in strategies {
        let mut found_line = None;
        let mutated: String = code.lines()
            .map(|line| {
                let trimmed = line.trim_start();
                let skip = trimmed.starts_with('#') || trimmed.starts_with("//")
                        || skip_patterns.iter().any(|p| line.contains(p));
                if found_line.is_none() && !skip && line.contains(*from) {
                    let new_line = line.replacen(from, to, 1);
                    found_line = Some((line.trim().to_string(), new_line.trim().to_string()));
                    new_line
                } else { line.to_string() }
            })
            .collect::<Vec<_>>()
            .join("\n");
        if let Some((orig, mutd)) = found_line {
            result.push((mutated, orig, mutd));
        }
    }
    result
}

impl SafeExecutor {
    pub async fn mutation_check(&self, source_file: &str) -> MutationResult {
        let source_path = self.workspace.join(source_file);
        if !source_path.exists() { return MutationResult::Skipped; }
        let ext = source_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let original = match std::fs::read_to_string(&source_path) {
            Ok(s) => s, Err(_) => return MutationResult::Skipped,
        };
        let mutations = apply_all_mutations(&original);
        if mutations.is_empty() { return MutationResult::Skipped; }
        // test runner per language
        let test_cmd: Vec<String> = match ext {
            "py" => {
                let pytest = if self.workspace.join("venv/bin/pytest").exists() {
                    "venv/bin/pytest"
                } else { "pytest" };
                vec![pytest.into(), "-x".into(), "-q".into(), "--tb=no".into()]
            }
            "go" => vec!["go".into(), "test".into(), "./...".into(), "-count=1".into()],
            "js" | "ts" => {
                let npx = if self.workspace.join("node_modules/.bin/jest").exists() {
                    "node_modules/.bin/jest"
                } else { "npx" };
                vec![npx.into(), "--forceExit".into(), "--silent".into()]
            }
            "rs" => vec!["cargo".into(), "test".into(), "--quiet".into()],
            _ => return MutationResult::Skipped,
        };
        let mut survived_orig = String::new();
        let mut survived_mutd = String::new();
        let mut any_caught  = false;
        let mut any_missed  = false;
        for (mutation, orig_line, mutd_line) in &mutations {
            if std::fs::write(&source_path, mutation).is_err() {
                let _ = std::fs::write(&source_path, &original);
                continue;
            }
            let out = tokio::time::timeout(
                std::time::Duration::from_secs(30),
                tokio::process::Command::new(&test_cmd[0])
                    .args(&test_cmd[1..])
                    .current_dir(&self.workspace)
                    .env("PYTHONPATH", &self.workspace)
                    .output(),
            ).await;
            let _ = std::fs::write(&source_path, &original);
            match out {
                Ok(Ok(result)) => {
                    if result.status.success() {
                        if !any_missed {
                            survived_orig = orig_line.clone();
                            survived_mutd = mutd_line.clone();
                        }
                        any_missed = true;
                    } else { any_caught = true; }
                }
                _ => {}
            }
            if any_missed { break; }
        }
        let _ = std::fs::write(&source_path, &original);
        if any_missed { MutationResult::Weak(survived_orig, survived_mutd) }
        else if any_caught { MutationResult::Strong }
        else { MutationResult::Skipped }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn ex(dir: &std::path::Path) -> SafeExecutor {
        SafeExecutor::new(dir.to_path_buf(), 10)
    }

    #[test]
    fn write_and_read() {
        let d = tempdir().unwrap();
        let e = ex(d.path());
        e.write_file("a.py", "x=1").unwrap();
        let r = e.read_file("a.py").unwrap();
        assert_eq!(r.stdout, "x=1");
    }

    #[test]
    fn append_file() {
        let d = tempdir().unwrap();
        std::fs::write(d.path().join("a.py"), "line1\n").unwrap();
        let e = ex(d.path());
        let r = e.append_file("a.py", "line2").unwrap();
        assert!(r.success);
        let c = std::fs::read_to_string(d.path().join("a.py")).unwrap();
        assert!(c.contains("line1") && c.contains("line2"));
    }

    #[test]
    fn blocks_path_traversal() {
        let d = tempdir().unwrap();
        let e = ex(d.path());
        assert!(e.write_file("../../etc/passwd", "x").is_err());
    }

    #[tokio::test]
    async fn runs_echo() {
        let d = tempdir().unwrap();
        let e = ex(d.path());
        let r = e.run(&Cmd::Run { command: "echo hello".into() }).await.unwrap();
        assert!(r.success);
        assert!(r.stdout.contains("hello"));
    }
}

fn parse_rust_tests(output: &str) -> (usize, usize) {
    let mut total_passed = 0usize;
    let mut total_failed = 0usize;
    for line in output.lines() {
        if line.contains("test result:") {
            for seg in line.split(';') {
                let s = seg.trim();
                let words: Vec<&str> = s.split_whitespace().collect();
                for (i, w) in words.iter().enumerate() {
                    if *w == "passed" && i > 0 {
                        if let Ok(n) = words[i-1].parse::<usize>() { total_passed += n; }
                    }
                    if *w == "failed" && i > 0 {
                        if let Ok(n) = words[i-1].parse::<usize>() { total_failed += n; }
                    }
                }
            }
        }
    }
    (total_passed, total_failed)
}

fn parse_go_tests(output: &str) -> (usize, usize) {
    let mut passed = 0usize;
    let mut failed = 0usize;
    for line in output.lines() {
        if line.starts_with("--- PASS") { passed += 1; }
        if line.starts_with("--- FAIL") { failed += 1; }
    }
    (passed, failed)
}

fn go_compile_check(workspace: &std::path::Path) -> Option<String> {
    if !workspace.join("go.mod").exists() { 
        eprintln!("[TRACE] go_compile_check: skipped (no go.mod)");
        return None; 
    }
    eprintln!("[TRACE] go_compile_check: running...");
    let out = std::process::Command::new("go")
        .args(&["test", "-run=^$", "-count=1"])
        .current_dir(workspace)
        .output()
        .ok()?;
    if !out.status.success() {
        Some(String::from_utf8_lossy(&out.stderr).to_string())
    } else {
        None
    }
}

/// AutoFix: يضيف Go stdlib import تلقائياً بدون LLM
fn autofix_go_undefined_import(file: &std::path::Path, err: &str) -> Option<String> {
    let go_std: &[(&str, &str)] = &[
        ("fmt",     "fmt"),
        ("errors",  "errors"),
        ("strings", "strings"),
        ("strconv", "strconv"),
        ("sort",    "sort"),
        ("math",    "math"),
        ("os",      "os"),
        ("io",      "io"),
        ("log",     "log"),
        ("time",    "time"),
        ("sync",    "sync"),
        ("context", "context"),
        ("bytes",   "bytes"),
        ("bufio",   "bufio"),
    ];

    // استخرج الرمز من "undefined: fmt"
    let sym = err.lines()
        .find(|l| l.contains("undefined:"))?
        .split("undefined:")
        .nth(1)?
        .trim()
        .split_whitespace()
        .next()?
        .split('.')
        .next()?
        .to_string();

    // تحقق أنه stdlib
    let pkg = go_std.iter()
        .find(|(name, _)| *name == sym.as_str())
        .map(|(_, pkg)| *pkg)?;

    // اقرأ الملف
    let src = std::fs::read_to_string(file).ok()?;

    // إذا كان موجوداً بالفعل
    if src.contains(&format!("\"{}\"", pkg)) {
        return None;
    }

    // أضف import
    let new_src = if src.contains("import (") {
        src.replacen("import (", &format!("import (\n\t\"{}\"", pkg), 1)
    } else {
        // أضف بعد package declaration
        let pkg_line = src.lines()
            .find(|l| l.starts_with("package "))?
            .to_string();
        src.replacen(
            &pkg_line,
            &format!("{}\n\nimport \"{}\"", pkg_line, pkg),
            1,
        )
    };

    std::fs::write(file, &new_src).ok()?;
    Some(pkg.to_string())
}

/// تنظيف الكود من Unicode quotes
fn sanitize_code(s: &str) -> String {
    let original_len = s.len();
    let result = s
        .replace('\u{201C}', "\"")
        .replace('\u{201D}', "\"")
        .replace('\u{2018}', "'")
        .replace('\u{2019}', "'")
        .replace('\u{2014}', "--")
        .replace('\u{2013}', "-")
        .replace('\u{00A0}', " ")
        .replace('\u{200B}', "")
        .replace('\u{FEFF}', "")
        .to_string();
    
    if result.len() != original_len {
        eprintln!("[TRACE] sanitize_code: cleaned {} Unicode chars", 
                  original_len - result.len());
    }
    result
}



#[cfg(test)]
mod bench_bugs {
    use super::*;
    use std::fs;

    fn make_ws(name: &str) -> PathBuf {
        let ws = std::env::temp_dir().join(format!("sel_bench_{}", name));
        let _ = fs::remove_dir_all(&ws);
        fs::create_dir_all(&ws).unwrap();
        ws
    }

    /// B1: sanitize_code يُصلح Unicode Quotes → ASCII
    #[test]
    fn b1_sanitize_unicode_quotes() {
        let input = "\u{201C}hello\u{201D}";  // "hello" (Unicode)
        let result = sanitize_code(input);
        assert_eq!(result, "\"hello\"", "Unicode quotes must become ASCII");
    }

    /// B2: patch_file يفشل صراحةً عند غياب search block
    #[test]
    fn b2_patch_explicit_fail() {
        let ws = make_ws("b2");
        fs::write(ws.join("main.rs"), "fn main() {}").unwrap();
        
        let exec = SafeExecutor::new(ws.clone(), 60);
        let result = exec.patch_file("main.rs", "GHOST_TEXT", "NEW").unwrap();
        
        assert!(!result.success, "patch_file must fail when search block is missing");
        assert!(result.stderr.contains("not found") || result.stderr.contains("FAILED"),
            "Error should mention 'not found', got: {}", result.stderr);
    }

    /// B4-a: Language Guard يمنع write_file لملفات .py في Rust workspace
    #[test]
    fn b4_language_wall_write() {
        let ws = make_ws("b4w");
        fs::write(ws.join("Cargo.toml"), "[package]\nname=\"t\"\n").unwrap();
        
        let exec = SafeExecutor::new(ws, 60);
        let result = exec.write_file("calc.py", "x = 1").unwrap();
        
        assert!(!result.success, "write_file must reject .py in Rust workspace");
        assert!(result.stderr.contains("LANGUAGE LOCK") || result.stderr.contains("BLOCKED"),
            "Must contain LANGUAGE LOCK, got: {}", result.stderr);
    }

    /// B4-b: Language Guard يمنع patch_file لملفات .py في Go workspace
    #[test]
    fn b4_language_wall_patch() {
        let ws = make_ws("b4p");
        fs::write(ws.join("go.mod"), "module test\n").unwrap();
        fs::write(ws.join("calc.py"), "x = 1\n").unwrap();
        
        let exec = SafeExecutor::new(ws, 60);
        let result = exec.patch_file("calc.py", "x = 1", "x = 2").unwrap();
        
        assert!(!result.success, "patch_file must reject .py in Go workspace");
        assert!(result.stderr.contains("LANGUAGE LOCK") || result.stderr.contains("BLOCKED"),
            "Must contain LANGUAGE LOCK, got: {}", result.stderr);
    }
}
