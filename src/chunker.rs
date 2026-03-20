// ============================================================
// src/chunker.rs  —  SEL Agent v5.3
// Context Chunking: يحل مشكلة 413 Payload Too Large
// ============================================================
//
// المنطق الأساسي:
//   1. استخراج file:line من مخرجات الاختبارات الفاشلة
//   2. إذا كان الملف > MAX_FILE_LINES → اقرأ فقط ±CHUNK_RADIUS سطر
//   3. أضف أرقام الأسطر في المحتوى المُرسل للـ LLM
//   4. أضف context_hint في الـ prompt يخبر اللغوي أنه يرى جزءاً من الملف
// ============================================================
 
use std::fs;
use std::path::Path;
 
// ─── ثوابت ───────────────────────────────────────────────────
pub const MAX_FILE_LINES: usize = 400;   // فوق هذا → نستخدم chunking
pub const CHUNK_RADIUS: usize = 60;      // ±60 سطر حول الخطأ
pub const CHARS_PER_TOKEN: usize = 4;    // تقدير: 1 token ≈ 4 حرف
pub const MAX_TOKENS_PER_FILE: usize = 3_000; // ~12K حرف كحد أقصى لملف واحد
 
// ─── البنى ───────────────────────────────────────────────────
 
/// موقع خطأ مستخرج من مخرجات الاختبار
#[derive(Debug, Clone)]
pub struct ErrorLocation {
    /// المسار النسبي أو المطلق للملف
    pub file: String,
    /// رقم السطر (1-indexed)
    pub line: usize,
}
 
/// نتيجة قراءة chunk من ملف كبير
#[derive(Debug)]
pub struct FileChunk {
    /// المحتوى مع أرقام الأسطر مضافة
    pub content: String,
    /// رقم السطر الأول في الـ chunk
    pub start_line: usize,
    /// رقم السطر الأخير في الـ chunk
    pub end_line: usize,
    /// إجمالي أسطر الملف الأصلي
    pub total_lines: usize,
}
 
// ─── استخراج مواقع الأخطاء ────────────────────────────────────
 
/// يستخرج مواقع الأخطاء من مخرجات الاختبارات
///
/// يدعم صياغات:
/// - Rust:   `src/main.rs:42:10` أو `--> src/main.rs:42`
/// - Python: `File "src/main.py", line 42`
/// - Go:     `src/main.go:42:`
/// - Node:   `src/main.js:42`
pub fn extract_error_locations(test_output: &str) -> Vec<ErrorLocation> {
    let mut locations: Vec<ErrorLocation> = Vec::new();
 
    for line in test_output.lines() {
        if let Some(loc) = parse_rust_location(line) {
            if !is_duplicate(&locations, &loc) {
                locations.push(loc);
            }
            continue;
        }
 
        if let Some(loc) = parse_python_location(line) {
            if !is_duplicate(&locations, &loc) {
                locations.push(loc);
            }
            continue;
        }
 
        if let Some(loc) = parse_generic_location(line) {
            if !is_duplicate(&locations, &loc) {
                locations.push(loc);
            }
        }
    }
 
    locations.truncate(5);
    locations
}
 
fn parse_rust_location(line: &str) -> Option<ErrorLocation> {
    let line = line.trim();
 
    let search_str = if line.starts_with("-->") {
        line.trim_start_matches("-->").trim()
    } else if line.contains(" --> ") {
        line.split(" --> ").nth(1)?
    } else {
        line
    };
 
    parse_file_line_col(search_str, &[".rs"])
}
 
fn parse_python_location(line: &str) -> Option<ErrorLocation> {
    let line = line.trim();
 
    if !line.contains("File ") {
        return None;
    }
 
    let file_pos = line.find("File ")?;
    let after_file = &line[file_pos + 5..];
    let (path, rest) = if after_file.starts_with('"') {
        let end = after_file[1..].find('"')? + 1;
        (&after_file[1..end], &after_file[end + 1..])
    } else if after_file.starts_with('\'') {
        let end = after_file[1..].find('\'')? + 1;
        (&after_file[1..end], &after_file[end + 1..])
    } else {
        return None;
    };
 
    if !path.ends_with(".py") {
        return None;
    }
 
    let line_part = rest.trim().strip_prefix(',')?;
    let line_part = line_part.trim().strip_prefix("line")?;
    let line_num: usize = line_part
        .trim()
        .split_whitespace()
        .next()?
        .trim_end_matches(',')
        .parse()
        .ok()?;
 
    Some(ErrorLocation {
        file: path.to_string(),
        line: line_num,
    })
}
 
fn parse_generic_location(line: &str) -> Option<ErrorLocation> {
    parse_file_line_col(line.trim(), &[".go", ".js", ".ts", ".java", ".c"])
}
 
fn parse_file_line_col(text: &str, extensions: &[&str]) -> Option<ErrorLocation> {
    let parts: Vec<&str> = text.splitn(4, ':').collect();
    if parts.len() < 2 {
        return None;
    }
 
    for i in 0..parts.len().saturating_sub(1) {
        let file_part = parts[..=i].join(":");
        let file_part = file_part.trim();
 
        let has_valid_ext = if extensions.contains(&".rs") {
            file_part.ends_with(".rs")
        } else {
            extensions.iter().any(|ext| file_part.ends_with(ext))
        };
 
        if !has_valid_ext {
            continue;
        }
 
        if let Some(line_str) = parts.get(i + 1) {
            let line_str = line_str.split_whitespace().next().unwrap_or("");
            let line_str = line_str.trim_end_matches(':');
            if let Ok(line_num) = line_str.parse::<usize>() {
                if line_num > 0 && line_num < 100_000 {
                    return Some(ErrorLocation {
                        file: file_part.to_string(),
                        line: line_num,
                    });
                }
            }
        }
    }
    None
}
 
fn is_duplicate(locations: &[ErrorLocation], new: &ErrorLocation) -> bool {
    locations
        .iter()
        .any(|l| l.file == new.file && (l.line as i64 - new.line as i64).abs() < 10)
}
 
// ─── قراءة Chunk من ملف كبير ─────────────────────────────────
 
/// يقرأ chunk من ملف كبير حول سطر محدد
/// يُضيف أرقام الأسطر في البداية لمساعدة اللغوي على الإشارة بدقة
pub fn read_file_chunk(path: &Path, center_line: usize, radius: usize) -> Option<FileChunk> {
    let content = fs::read_to_string(path).ok()?;
    let all_lines: Vec<&str> = content.lines().collect();
    let total_lines = all_lines.len();
 
    if total_lines == 0 {
        return None;
    }
 
    let center_idx = center_line.saturating_sub(1).min(total_lines - 1);
    let start_idx = center_idx.saturating_sub(radius);
    let end_idx = (center_idx + radius).min(total_lines - 1);
 
    let mut chunk_lines = Vec::new();
    for (i, line) in all_lines[start_idx..=end_idx].iter().enumerate() {
        let line_num = start_idx + i + 1;
        chunk_lines.push(format!("{:5}: {}", line_num, line));
    }
 
    Some(FileChunk {
        content: chunk_lines.join("\n"),
        start_line: start_idx + 1,
        end_line: end_idx + 1,
        total_lines,
    })
}
 
/// يقرر هل يُرسل الملف كاملاً أم chunk
pub fn get_file_content_smart(
    path: &Path,
    error_locations: &[ErrorLocation],
) -> Result<SmartContent, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("خطأ في قراءة {:?}: {}", path, e))?;
 
    let line_count = content.lines().count();
    let token_estimate = content.len() / CHARS_PER_TOKEN;
 
    if line_count <= MAX_FILE_LINES && token_estimate <= MAX_TOKENS_PER_FILE {
        return Ok(SmartContent::FullFile(content));
    }
 
    let path_str = path.to_string_lossy();
    let center_line = error_locations
        .iter()
        .find(|loc| path_str.contains(&loc.file) || loc.file.contains(path_str.as_ref()))
        .map(|loc| loc.line)
        .unwrap_or(1);
 
    match read_file_chunk(path, center_line, CHUNK_RADIUS) {
        Some(chunk) => Ok(SmartContent::Chunk(chunk)),
        None => Ok(SmartContent::FullFile(content)),
    }
}
 
// ─── SmartContent ─────────────────────────────────────────────
 
pub enum SmartContent {
    FullFile(String),
    Chunk(FileChunk),
}
 
impl SmartContent {
    pub fn content_for_prompt(&self, file_name: &str) -> String {
        match self {
            SmartContent::FullFile(content) => content.clone(),
            SmartContent::Chunk(chunk) => format!(
                "[[ CHUNK: {} — أسطر {}-{} من {} ]]\n{}",
                file_name, chunk.start_line, chunk.end_line, chunk.total_lines, chunk.content
            ),
        }
    }
 
    pub fn is_chunk(&self) -> bool {
        matches!(self, SmartContent::Chunk(_))
    }
 
    pub fn context_hint(&self, file_name: &str) -> Option<String> {
        match self {
            SmartContent::FullFile(_) => None,
            SmartContent::Chunk(chunk) => Some(format!(
                "⚠️ CHUNKED FILE: '{}' يحتوي على {} سطر. تظهر لك فقط الأسطر {}-{}. \
                 عند كتابة الـ patch، استخدم أرقام الأسطر الظاهرة. \
                 لا تفترض بداية الملف = سطر 1.",
                file_name, chunk.total_lines, chunk.start_line, chunk.end_line,
            )),
        }
    }
}
 
// ─── تقدير الـ tokens ─────────────────────────────────────────
 
pub fn estimate_tokens(text: &str) -> usize {
    text.len() / CHARS_PER_TOKEN
}
 
pub fn estimate_context_tokens(files: &[(String, String)]) -> usize {
    files.iter().map(|(_, content)| estimate_tokens(content)).sum()
}
 
// ─── اختبارات الوحدة ──────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
 
    #[test]
    fn test_extract_rust_location_arrow() {
        let output = "error[E0308]: mismatched types\n  --> src/main.rs:42:10\n   |";
        let locs = extract_error_locations(output);
        assert_eq!(locs.len(), 1);
        assert_eq!(locs[0].file, "src/main.rs");
        assert_eq!(locs[0].line, 42);
    }
 
    #[test]
    fn test_extract_python_location() {
        let output = "Traceback (most recent call last):\n  File \"src/core.py\", line 1217, in make_context\nAssertionError";
        let locs = extract_error_locations(output);
        assert_eq!(locs.len(), 1);
        assert_eq!(locs[0].file, "src/core.py");
        assert_eq!(locs[0].line, 1217);
    }
 
    #[test]
    fn test_extract_go_location() {
        let output = "FAIL\nmain_test.go:34: got nil, want error";
        let locs = extract_error_locations(output);
        assert!(locs.iter().any(|l| l.file.contains("main_test.go") && l.line == 34));
    }
 
    #[test]
    fn test_chunk_radius() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        for i in 1..=500 {
            writeln!(tmp, "line {}", i).unwrap();
        }
        let chunk = read_file_chunk(tmp.path(), 250, 60).unwrap();
        assert_eq!(chunk.start_line, 190);
        assert_eq!(chunk.end_line, 310);
        assert!(chunk.content.contains("  190:"));
        assert!(chunk.content.contains("  310:"));
    }
 
    #[test]
    fn test_chunk_at_start_of_file() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        for i in 1..=500 {
            writeln!(tmp, "line {}", i).unwrap();
        }
        let chunk = read_file_chunk(tmp.path(), 10, 60).unwrap();
        assert_eq!(chunk.start_line, 1);
        assert_eq!(chunk.end_line, 70);
    }
 
    #[test]
    fn test_estimate_tokens() {
        let text = "a".repeat(400);
        assert_eq!(estimate_tokens(&text), 100);
    }
 
    #[test]
    fn test_no_duplicates_close_lines() {
        let output = "  --> src/main.rs:42:10\n  --> src/main.rs:43:5";
        let locs = extract_error_locations(output);
        assert_eq!(locs.len(), 1);
    }
 
    #[test]
    fn test_smart_content_full_file_small() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        for i in 1..=100 {
            writeln!(tmp, "fn line_{}() {{}}", i).unwrap();
        }
        let result = get_file_content_smart(tmp.path(), &[]).unwrap();
        assert!(!result.is_chunk());
    }
 
    #[test]
    fn test_smart_content_chunk_large() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        for i in 1..=600 {
            writeln!(tmp, "fn line_{}() {{ /* code */ }}", i).unwrap();
        }
        let loc = ErrorLocation { file: "test".into(), line: 300 };
        let result = get_file_content_smart(tmp.path(), &[loc]).unwrap();
        assert!(result.is_chunk());
    }
 
    #[test]
    fn test_context_hint_format() {
        let chunk = FileChunk {
            content: "1210: fn foo() {}".into(),
            start_line: 1210,
            end_line: 1270,
            total_lines: 1800,
        };
        let smart = SmartContent::Chunk(chunk);
        let hint = smart.context_hint("src/core.py").unwrap();
        assert!(hint.contains("1800"));
        assert!(hint.contains("1210-1270"));
    }
}
