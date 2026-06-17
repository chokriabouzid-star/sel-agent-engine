use crate::constitution;
use crate::executor::autofix::*;
use crate::executor::compile::*;
use crate::executor::core::SafeExecutor;
use crate::executor::sanitizers::*;
use crate::types::ExecResult;
use anyhow::Result;

impl SafeExecutor {
    //  File Operations

    pub fn is_spec_file(&self, path: &str) -> bool {
        let filename = std::path::Path::new(path)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("");

        filename.starts_with("test_")           // test_main.py
            || filename.ends_with("_test.go")   // main_test.go
            || filename.ends_with("_test.rs")   // main_test.rs
            || filename.contains(".test.")      // app.test.ts
            || filename.contains(".spec.")      // app.spec.ts
            || filename.ends_with("_spec.rb") // main_spec.rb
    }

    fn blocks_existing_spec_modification(&self, path: &str, p: &std::path::Path) -> bool {
        self.is_spec_file(path) && self.protected_test_files.contains(p)
    }

    pub fn write_file(&self, path: &str, content: &str) -> Result<ExecResult> {
        {
            let ext = std::path::Path::new(path)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            let is_config = matches!(
                path,
                "Cargo.toml"
                    | "go.mod"
                    | "go.sum"
                    | "package.json"
                    | "tsconfig.json"
                    | "Makefile"
                    | ".gitignore"
                    | "README.md"
            );

            if !is_config && !ext.is_empty() {
                if let Err(e) = self.oracle.is_ext_allowed(ext) {
                    return Ok(ExecResult::fail(e));
                }
            }
        }
        let p = self.safe_path(path)?;

        // Protection: Cargo.toml is writable (dependency updates), but lockfiles/go.mod stay protected
        let protected = ["Cargo.lock", "go.mod", "go.sum"];
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if protected.contains(&name) && p.exists() {
            return Ok(ExecResult::fail(format!(
                "write_file: '{}' is protected and cannot be overwritten",
                path
            )));
        }
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Protection: warn if overwriting with much smaller content
        if p.exists() {
            let existing_len = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            let new_len = content.len() as u64;
            if existing_len > 500 && new_len < existing_len / 3 {
                println!("   ⚠️  WARNING: Overwriting {} ({} bytes) with much smaller content ({} bytes)",
                    path, existing_len, new_len);
            }
        }

        // Rust brace balance check
        let content = if path.ends_with(".rs") {
            let open = content.chars().filter(|&c| c == '{').count();
            let close = content.chars().filter(|&c| c == '}').count();
            if open > close {
                let mut fixed = content.to_string();
                for _ in 0..(open - close) {
                    fixed.push_str("\n}");
                }
                std::borrow::Cow::Owned(fixed)
            } else {
                std::borrow::Cow::Borrowed(content)
            }
        } else {
            std::borrow::Cow::Borrowed(content)
        };
        // sanitize Unicode quotes before writing
        let content_str = sanitize_code(content.as_ref());
        let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("");
        let content_str = if ext == "rs" {
            let fixed = sanitize_rust_lifetime_quotes(&content_str);
            fix_rust_string_literals(&fixed)
        } else if ext == "py" {
            fix_python_string_quoting(&content_str)
        } else {
            content_str
        };
        eprintln!(
            "[TRACE] write_file sanitize: input={} output={}",
            content.as_ref().len(),
            content_str.len()
        );

        if let Err(e) = constitution::check_write(
            &p,
            &content_str,
            self.blocks_existing_spec_modification(path, &p),
        ) {
            return Ok(ExecResult::fail(e.to_string()));
        }

        std::fs::write(&p, content_str.as_bytes())?;

        // Auto-fix: if jest.config.js is written -> remove "jest" field from package.json
        if path.ends_with("jest.config.js") {
            let pkg = self.workspace.join("package.json");
            if pkg.exists() {
                if let Ok(pkg_src) = std::fs::read_to_string(&pkg) {
                    if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&pkg_src) {
                        if v.get("jest").is_some() {
                            if let Some(obj) = v.as_object_mut() {
                                obj.remove("jest");
                            }
                            if let Ok(fixed) = serde_json::to_string_pretty(&v) {
                                let _ = std::fs::write(&pkg, fixed);
                                println!("   ⚡ AutoFix: removed jest field from package.json (conflicts with jest.config.js)");
                            }
                        }
                    }
                }
            }
        }
        println!("   ✏️  {} ({} bytes)", path, content.len());

        // compile check for go files
        // v8.4.2: Only fail write_file if the error is IN THE FILE BEING WRITTEN.
        // Errors in OTHER files (e.g. main_test.go when writing main.go) are
        // reported as warnings so the agent can continue and fix them separately.
        if path.ends_with(".go") {
            // AutoFix: goroutine deadlock in ProcessJobs (must run before compile check)
            if path == "main.go" {
                if let Some(msg) = autofix_go_worker_pool_deadlock(&p) {
                    println!("   ⚡ AutoFix Go deadlock: {}", msg);
                }
            }

            if let Some(mut err) = go_compile_check(&self.workspace) {
                eprintln!(
                    "[TRACE] Checking autofix for: {}",
                    err.chars().take(80).collect::<String>()
                );
                // Check if the error mentions THIS file specifically
                let file_name = std::path::Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(path);
                let mut err_mentions_this_file = err.contains(file_name);

                if err_mentions_this_file {
                    // AutoFix chain: syntax → imports → common Go test patterns
                    if let Some(fixed) = autofix_go_missing_comma(&p, &err) {
                        println!("   ⚡ AutoFix Go missing comma: {}", fixed);
                        if let Some(err2) = go_compile_check(&self.workspace) {
                            err = err2;
                        } else {
                            println!("   ✅ AutoFix missing-comma succeeded");
                            return Ok(ExecResult::ok(format!("Written: {}", path)));
                        }
                    }

                    if let Some(fixed) = autofix_go_unused_import(&p, &err) {
                        println!("   ⚡ AutoFix Go unused import: {}", fixed);
                        if let Some(err2) = go_compile_check(&self.workspace) {
                            err = err2;
                        } else {
                            println!("   ✅ AutoFix unused-import succeeded");
                            return Ok(ExecResult::ok(format!("Written: {}", path)));
                        }
                    }

                    if let Some(fixed) = autofix_go_test_run_shadow_alias(&p, &err) {
                        println!("   ⚡ AutoFix Go test shadow alias: {}", fixed);
                        if let Some(err2) = go_compile_check(&self.workspace) {
                            err = err2;
                        } else {
                            println!("   ✅ AutoFix test-shadow-alias succeeded");
                            return Ok(ExecResult::ok(format!("Written: {}", path)));
                        }
                    }

                    if let Some(fixed) = autofix_go_test_table_shadow_run(&p, &err) {
                        println!("   ⚡ AutoFix Go test shadow: {}", fixed);
                        if let Some(err2) = go_compile_check(&self.workspace) {
                            err = err2;
                        } else {
                            println!("   ✅ AutoFix test-shadow succeeded");
                            return Ok(ExecResult::ok(format!("Written: {}", path)));
                        }
                    }

                    if let Some(fixed) = autofix_go_undefined_import(&p, &err) {
                        println!("   ⚡ AutoFix Go import: {}", fixed);
                        if let Some(err2) = go_compile_check(&self.workspace) {
                            err = err2;
                        } else {
                            println!("   ✅ AutoFix import succeeded");
                            return Ok(ExecResult::ok(format!("Written: {}", path)));
                        }
                    }

                    err_mentions_this_file = err.contains(file_name);
                }

                if err_mentions_this_file {
                    // Error is specifically in this file — block and report
                    return Ok(ExecResult::fail(format!(
                        "COMPILE ERROR in '{}':\n{}",
                        path, err
                    )));
                } else {
                    // Error is in a DIFFERENT file — warn but don't block
                    eprintln!(
                        "[TRACE] go_compile_check: error in other file (not '{}'), continuing",
                        path
                    );
                }
            }
        }
        Ok(ExecResult::ok(format!("Written: {}", path)))
    }

    pub fn append_file(&self, path: &str, content: &str) -> Result<ExecResult> {
        let p = self.safe_path(path)?;
        if !p.exists() {
            return Ok(ExecResult::fail(format!(
                "'{}' not found  use write_file first",
                path
            )));
        }
        let mut orig = std::fs::read_to_string(&p)?;
        if !orig.ends_with('\n') {
            orig.push('\n');
        }
        orig.push('\n');
        orig.push_str(content);

        if let Err(e) =
            constitution::check_write(&p, &orig, self.blocks_existing_spec_modification(path, &p))
        {
            return Ok(ExecResult::fail(e.to_string()));
        }

        std::fs::write(&p, &orig)?;
        println!("   📝 {} (+{} bytes)", path, content.len());
        Ok(ExecResult::ok(format!("Appended: {}", path)))
    }

    pub fn delete_file(&self, path: &str) -> Result<ExecResult> {
        let p = self.safe_path(path)?;
        // Protection: do not delete core config files
        let protected = ["Cargo.toml", "go.mod", "package.json", "Cargo.lock"];
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if protected.contains(&name) {
            return Ok(ExecResult::fail(format!(
                "delete_file: '{}' is protected and cannot be deleted",
                path
            )));
        }
        // Protection: do not delete directories
        if p.is_dir() {
            return Ok(ExecResult::fail(format!(
                "delete_file: '{}' is a directory  only files allowed",
                path
            )));
        }
        if !p.exists() {
            return Ok(ExecResult::ok(format!(
                "delete_file: '{}' already absent",
                path
            )));
        }
        std::fs::remove_file(&p)?;
        println!("   🗑  Deleted: {}", path);
        Ok(ExecResult::ok(format!("Deleted: {}", path)))
    }

    // Validate patch result to prevent code corruption
    pub fn validate_patch(&self, path: &str, original: &str, patched: &str) -> Result<(), String> {
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
            return Err(format!(
                "Patch changed too many lines: {}  {} lines",
                orig_lines, new_lines
            ));
        }

        // 3. Rust-specific checks
        if path.ends_with(".rs") {
            let open_braces = patched.matches('{').count();
            let close_braces = patched.matches('}').count();
            if open_braces != close_braces {
                return Err(format!(
                    "Unmatched braces: {} open, {} close",
                    open_braces, close_braces
                ));
            }
        }

        Ok(())
    }

    pub fn patch_file(&self, path: &str, search: &str, replace: &str) -> Result<ExecResult> {
        {
            let ext = std::path::Path::new(path)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            let is_config = matches!(
                path,
                "Cargo.toml"
                    | "go.mod"
                    | "go.sum"
                    | "package.json"
                    | "tsconfig.json"
                    | "Makefile"
                    | ".gitignore"
                    | "README.md"
            );

            if !is_config && !ext.is_empty() {
                if let Err(e) = self.oracle.is_ext_allowed(ext) {
                    return Ok(ExecResult::fail(e));
                }
            }
        }
        let p = self.safe_path(path)?;

        // Pre-flight check
        if !p.exists() {
            return Ok(ExecResult::fail(format!(
                "PREFLIGHT_FAIL: '{}' does not exist. Use write_file to create it first, then patch.",
                path
            )));
        }

        // Fallback to write_file after 2 failed patch attempts
        {
            let mut attempts = self.patch_attempts.borrow_mut();
            let count = *attempts.entry(p.clone()).or_insert(0);

            if count >= 2 {
                println!(
                    "   ⚠️  patch_file failed {} times on '{}'  switching to write_file fallback",
                    count, path
                );
                drop(attempts); // release borrow

                let content = std::fs::read_to_string(&p)?;
                let content = sanitize_code(&content);
                let search_buf = sanitize_code(search);
                let search = search_buf.as_str();
                let replace_buf = sanitize_code(replace);
                let replace = replace_buf.as_str();

                if !content.contains(search) {
                    self.patch_attempts.borrow_mut().insert(p.clone(), 0);
                    return Ok(ExecResult::fail(format!(
                        "PATCH FAILED: search block not found in '{}' after {} attempts  use write_file with complete file content",
                        path, count
                    )));
                }
                let new_content = content.replacen(search, replace, 1);
                let new_content = sanitize_code(&new_content);
                let new_content = if p.extension().map(|x| x == "rs").unwrap_or(false) {
                    let fixed = sanitize_rust_lifetime_quotes(&new_content);
                    fix_rust_string_literals(&fixed)
                } else {
                    new_content
                };

                if let Err(e) = constitution::check_write(
                    &p,
                    &new_content,
                    self.blocks_existing_spec_modification(path, &p),
                ) {
                    return Ok(ExecResult::fail(e.to_string()));
                }

                std::fs::write(&p, new_content.as_bytes())?;
                self.patch_attempts.borrow_mut().insert(p.clone(), 0);
                return Ok(ExecResult::ok(format!(
                    "patch_file: fallback write_file applied to '{}' successfully",
                    path
                )));
            }
        }

        if search.trim().is_empty() {
            return Ok(ExecResult::fail(
                "patch_file: search block is empty".to_string(),
            ));
        }
        let raw_content = std::fs::read_to_string(&p)?;
        let content = sanitize_code(&raw_content); // v8.4: sanitize file content
                                                   // v8.4: sanitize search/replace too — LLM may send unicode quotes
        let search_sanitized = sanitize_code(search);
        let search = search_sanitized.as_str();
        let replace_sanitized = sanitize_code(replace);
        let replace = replace_sanitized.as_str();
        let count = content.matches(search).count();

        // If not found directly, try normalizing whitespace
        let (effective_search, effective_replace, normalized) = if count == 0 {
            let norm_content = content.split_whitespace().collect::<Vec<_>>().join(" ");
            let norm_search = search.split_whitespace().collect::<Vec<_>>().join(" ");
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
            *self
                .patch_attempts
                .borrow_mut()
                .entry(p.clone())
                .or_insert(0) += 1;
            return Ok(ExecResult::fail(format!(
                "patch_file: search block not found in '{}' (tried exact + whitespace-normalized)  copy the exact text from the file", path
            )));
        }
        if count > 1 {
            return Ok(ExecResult::fail(format!(
                "patch_file: search block found {} times in '{}'  must be unique, use more context",
                count, path
            )));
        }

        let new_content = if normalized.is_some() {
            content_key.replacen(search_key, &effective_replace, 1)
        } else {
            content.replacen(search, replace, 1)
        };

        // Validate before writing
        if let Err(e) = self.validate_patch(path, &content, &new_content) {
            *self
                .patch_attempts
                .borrow_mut()
                .entry(p.clone())
                .or_insert(0) += 1;
            return Ok(ExecResult::fail(format!(
                "patch_file validation failed: {}",
                e
            )));
        }

        // auto-fix single-quote string literals in Rust files
        let new_content = if p.extension().map(|x| x == "rs").unwrap_or(false) {
            {
                let fixed = sanitize_rust_lifetime_quotes(&new_content);
                fix_rust_string_literals(&fixed)
            }
        } else {
            new_content
        };

        if let Err(e) = constitution::check_write(
            &p,
            &new_content,
            self.blocks_existing_spec_modification(path, &p),
        ) {
            return Ok(ExecResult::fail(e.to_string()));
        }

        std::fs::write(&p, &new_content)?;

        // reset counter on success
        self.patch_attempts.borrow_mut().insert(p.clone(), 0);

        println!(
            "   🔧 patch_file: {} ({} bytes  {} bytes)",
            path,
            content.len(),
            new_content.len()
        );
        Ok(ExecResult::ok(format!("Patched: {}", path)))
    }

    pub fn read_file(&self, path: &str) -> Result<ExecResult> {
        let p = self.safe_path(path)?;
        if !p.exists() {
            return Ok(ExecResult::fail(format!("'{}' not found", path)));
        }
        let content = std::fs::read_to_string(&p)?;
        println!("   ✏️  {} ({} bytes)", path, content.len());
        Ok(ExecResult {
            success: true,
            exit_code: 0,
            stdout: content,
            stderr: String::new(),
            duration_ms: 0,
            autofix_triggered: false,
        })
    }

    pub fn mkdir(&self, path: &str) -> Result<ExecResult> {
        std::fs::create_dir_all(self.workspace.join(path))?;
        Ok(ExecResult::ok(format!("mkdir: {}", path)))
    }
}
