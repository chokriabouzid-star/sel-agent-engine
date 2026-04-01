use crate::types::FailureKind;

#[derive(Debug, Clone, Default)]
pub struct FailureReport {
    pub kind: FailureKind,
    pub file: Option<String>,
    pub function: Option<String>,
    pub line: Option<u32>,
    pub expected: Option<String>,
    pub got: Option<String>,
    pub error_line: Option<String>,
    pub hint: String,
}

impl Default for FailureKind {
    fn default() -> Self { FailureKind::Unknown }
}

impl FailureReport {
    pub fn extract(stderr: &str, kind: &FailureKind) -> Self {
        let mut report = Self { kind: kind.clone(), ..Default::default() };
        match kind {
            FailureKind::AssertionError => report.extract_assertion(stderr),
            FailureKind::SyntaxError => report.extract_syntax(stderr),
            FailureKind::TypeError => report.extract_type_error(stderr),
            FailureKind::BuildError => report.extract_build_error(stderr),
            FailureKind::ImportError => report.extract_import(stderr),
            FailureKind::PatchError => report.extract_patch(stderr),
            FailureKind::CollectionError => report.extract_collection(stderr),
            _ => report.extract_generic(stderr),
        }
        if report.hint.is_empty() { report.hint = kind.clone().repair_hint().to_string(); }
        report
    }

    fn extract_assertion(&mut self, stderr: &str) {
        for line in stderr.lines() {
            let l = line.trim();
            if l.starts_with("FAILED ") {
                let rest = &l[7..];
                let parts: Vec<&str> = rest.splitn(2, " - ").collect();
                if let Some(loc) = parts.first() {
                    let locs: Vec<&str> = loc.splitn(2, "::").collect();
                    if let Some(f) = locs.first() { self.file = Some(f.trim().to_string()); }
                    if let Some(fn_) = locs.get(1) { self.function = Some(fn_.trim().to_string()); }
                }
                break;
            }
            if l.starts_with("FAIL ") && self.file.is_none() {
                self.file = Some(l[5..].trim().to_string());
            }
        }
        for line in stderr.lines() {
            let l = line.trim();
            if (l.starts_with("E   assert ") || l.starts_with("E assert ")) {
                let expr = l.trim_start_matches("E").trim().trim_start_matches("assert").trim();
                if let Some(idx) = expr.find(" == ") {
                    self.got = Some(expr[..idx].trim().to_string());
                    self.expected = Some(expr[idx+4..].trim().to_string());
                }
            }
            if l.starts_with("Expected:") {
                self.expected = Some(l[9..].trim().to_string());
            }
            if l.starts_with("Received:") {
                self.got = Some(l[9..].trim().to_string());
            }
            if self.line.is_none() {
                if let Some(ln) = extract_line_number(l) { self.line = Some(ln); }
            }
        }
        let fn_str = self.function.as_deref().unwrap_or("test");
        let file_str = self.file.as_deref().unwrap_or("file");
        self.hint = match (&self.got, &self.expected) {
            (Some(got), Some(exp)) => format!(
                "ASSERTION FAILED in {}::{} got [{}] expected [{}]. Fix IMPLEMENTATION not test.",
                file_str, fn_str, got, exp
            ),
            (Some(got), None) => format!(
                "ASSERTION FAILED in {}::{} got [{}]. Check logic.", file_str, fn_str, got
            ),
            _ => format!("ASSERTION FAILED in {}::{}. Fix implementation.", file_str, fn_str),
        };
    }

    fn extract_syntax(&mut self, stderr: &str) {
        for line in stderr.lines() {
            let l = line.trim();
            if l.contains("File ") && l.contains(", line ") {
                if let Some(start) = l.find(char::from(34)) {
                    let after = &l[start+1..];
                    if let Some(end) = after.find(char::from(34)) {
                        self.file = Some(after[..end].to_string());
                    }
                }
                if let Some(ln) = extract_line_number(l) { self.line = Some(ln); }
            }
            if l.starts_with("SyntaxError:") {
                self.error_line = Some(l.to_string());
            }
            if l.contains("error TS") {
                if let Some(f) = extract_ts_file(l) { self.file = Some(f); }
                if let Some(ln) = extract_ts_line(l) { self.line = Some(ln); }
                self.error_line = Some(l.to_string());
            }
        }
        let file_str = self.file.as_deref().unwrap_or("file");
        self.hint = match self.line {
            Some(ln) => format!("SYNTAX ERROR in {} at line {}. Fix syntax.", file_str, ln),
            None => format!("SYNTAX ERROR in {}. Fix syntax.", file_str),
        };
    }

    fn extract_type_error(&mut self, stderr: &str) {
        for line in stderr.lines() {
            let l = line.trim();
            if l.starts_with("TypeError:") { self.error_line = Some(l.to_string()); }
            if l.contains("error TS") {
                if let Some(f) = extract_ts_file(l) { self.file = Some(f); }
                if let Some(ln) = extract_ts_line(l) { self.line = Some(ln); }
                self.error_line = Some(l.to_string());
            }
            if self.file.is_none() { if let Some(f) = extract_python_file(l) { self.file = Some(f); } }
            if self.line.is_none() { if let Some(ln) = extract_line_number(l) { self.line = Some(ln); } }
        }
        let file_str = self.file.as_deref().unwrap_or("file");
        self.hint = match &self.error_line {
            Some(err) => format!("TYPE ERROR in {}: {}. Fix signatures.", file_str, err),
            None => format!("TYPE ERROR in {}. Check types.", file_str),
        };
    }

    fn extract_build_error(&mut self, stderr: &str) {
        let mut errors: Vec<String> = Vec::new();
        for line in stderr.lines() {
            let l = line.trim();
            if l.starts_with("error[") || l.starts_with("error: ") {
                errors.push(l.chars().take(120).collect());
            }
            if l.contains("error TS") {
                errors.push(l.chars().take(120).collect());
                if self.file.is_none() {
                    if let Some(f) = extract_ts_file(l) { self.file = Some(f); }
                    if let Some(ln) = extract_ts_line(l) { self.line = Some(ln); }
                }
            }
            if l.starts_with("-->") && self.file.is_none() {
                let loc = l[3..].trim();
                if let Some(colon) = loc.find(char::from(58)) {
                    self.file = Some(loc[..colon].to_string());
                    let rest = &loc[colon+1..];
                    if let Some(ln) = rest.split(char::from(58)).next().and_then(|s| s.parse::<u32>().ok()) {
                        self.line = Some(ln);
                    }
                }
            }
        }
        let file_str = self.file.as_deref().unwrap_or("source");
        let err_sum = errors.first().map(|s| s.as_str()).unwrap_or("compilation failed");
        self.hint = format!("BUILD ERROR in {}: {}. Fix before tests. {} errors.", file_str, err_sum, errors.len());
    }

    fn extract_import(&mut self, stderr: &str) {
        for line in stderr.lines() {
            let l = line.trim();
            if l.contains("No module named") || l.contains("Cannot find module") {
                self.error_line = Some(l.to_string());
                let quote = char::from(39);
                if let Some(start) = l.find(quote) {
                    let after = &l[start+1..];
                    if let Some(end) = after.find(quote) {
                        self.got = Some(after[..end].to_string());
                    }
                }
            }
        }
        self.hint = match &self.got {
            Some(pkg) => format!("IMPORT ERROR: module {} not found. Install it.", pkg),
            None => "IMPORT ERROR: Module not found. Check install.".to_string(),
        };
    }

    fn extract_patch(&mut self, stderr: &str) {
        for line in stderr.lines() {
            let l = line.trim();
            if l.contains("not found in") || l.contains("patch_file") {
                self.error_line = Some(l.to_string());
                let quote = char::from(39);
                if let Some(start) = l.find(quote) {
                    let after = &l[start+1..];
                    if let Some(end) = after.find(quote) {
                        self.file = Some(after[..end].to_string());
                    }
                }
            }
        }
        let file_str = self.file.as_deref().unwrap_or("the file");
        self.hint = format!("PATCH ERROR in {}: search text not found. Copy EXACTLY from current file.", file_str);
    }

    fn extract_collection(&mut self, stderr: &str) {
        for line in stderr.lines() {
            let l = line.trim();
            if l.contains("ERROR collecting") || l.contains("ImportError") {
                self.error_line = Some(l.chars().take(150).collect());
                if self.file.is_none() { if let Some(f) = extract_python_file(l) { self.file = Some(f); } }
            }
        }
        let file_str = self.file.as_deref().unwrap_or("test file");
        self.hint = match &self.error_line {
            Some(err) => format!("COLLECTION ERROR in {}: {}. Fix imports.", file_str, err),
            None => format!("COLLECTION ERROR in {}. Fix imports and syntax.", file_str),
        };
    }

    fn extract_generic(&mut self, stderr: &str) {
        for line in stderr.lines() {
            let l = line.trim();
            if !l.is_empty() && (l.contains("error") || l.contains("Error") || l.contains("ERROR")) {
                self.error_line = Some(l.chars().take(200).collect());
                break;
            }
        }
        self.hint = match &self.error_line {
            Some(err) => format!("ERROR: {}. Fix root cause.", err),
            None => "Unknown error. Read stderr and fix.".to_string(),
        };
    }

    pub fn to_repair_hint(&self) -> String {
        let mut parts = vec![self.hint.clone()];
        if let Some(f) = &self.file { parts.push(format!("Affected file: {}", f)); }
        if let Some(func) = &self.function { parts.push(format!("Failed test: {}", func)); }
        if let (Some(got), Some(exp)) = (&self.got, &self.expected) {
            parts.push(format!("Got: {} | Expected: {}", got, exp));
        }
        if let Some(ln) = self.line { parts.push(format!("At line: {}", ln)); }
        parts.join("\n")
    }
}

fn extract_line_number(text: &str) -> Option<u32> {
    if let Some(idx) = text.find("line ") {
        let rest = &text[idx + 5..];
        let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !num.is_empty() { return num.parse().ok(); }
    }
    None
}

fn extract_python_file(text: &str) -> Option<String> {
    let dq = char::from(34);
    if text.contains("File ") {
        if let Some(start) = text.find("File ") {
            let after = &text[start + 5..];
            if let Some(s) = after.find(dq) {
                let inner = &after[s+1..];
                if let Some(e) = inner.find(dq) {
                    return Some(inner[..e].to_string());
                }
            }
        }
    }
    if text.contains("collecting ") {
        let after = &text[text.find("collecting ").unwrap() + 11..];
        let name: String = after.chars().take_while(|c| !c.is_whitespace()).collect();
        if name.ends_with(".py") { return Some(name); }
    }
    None
}

fn extract_ts_file(text: &str) -> Option<String> {
    if let Some(paren) = text.find(char::from(40)) {
        let candidate = &text[..paren];
        if candidate.ends_with(".ts") || candidate.ends_with(".js") {
            return Some(candidate.to_string());
        }
    }
    None
}

fn extract_ts_line(text: &str) -> Option<u32> {
    if let Some(paren) = text.find(char::from(40)) {
        let after = &text[paren+1..];
        let num: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        return num.parse().ok();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_assertion_pytest() {
        let stderr = "FAILED test_add.py::test_neg - AssertionError\nE   assert -1 == 1";
        let r = FailureReport::extract(stderr, &FailureKind::AssertionError);
        assert_eq!(r.file.as_deref(), Some("test_add.py"));
        assert_eq!(r.function.as_deref(), Some("test_neg"));
        assert!(r.hint.contains("ASSERTION"));
    }

    #[test]
    fn test_import_error() {
        let stderr = "ModuleNotFoundError: No module named 'flask'";
        let r = FailureReport::extract(stderr, &FailureKind::ImportError);
        assert_eq!(r.got.as_deref(), Some("flask"));
        assert!(r.hint.contains("flask"));
    }

    #[test]
    fn test_patch_error() {
        let stderr = "search block not found in 'src/lib.rs'";
        let r = FailureReport::extract(stderr, &FailureKind::PatchError);
        assert_eq!(r.file.as_deref(), Some("src/lib.rs"));
        assert!(r.hint.contains("PATCH ERROR"));
    }

    #[test]
    fn test_build_error_ts() {
        let stderr = "src/add.ts(5,3): error TS2345: Argument of type";
        let r = FailureReport::extract(stderr, &FailureKind::BuildError);
        assert_eq!(r.file.as_deref(), Some("src/add.ts"));
        assert_eq!(r.line, Some(5));
    }

    #[test]
    fn test_repair_hint() {
        let r = FailureReport::extract("FAILED t.py::t1 - Err", &FailureKind::AssertionError);
        let hint = r.to_repair_hint();
        assert!(!hint.is_empty());
    }
}