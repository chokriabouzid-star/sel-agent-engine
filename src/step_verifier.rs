// src/step_verifier.rs — StepVerifier v1.0
// يتحقق أن كل خطوة أنتجت تغييراً حقيقياً وصحيحاً

use crate::file_snapshot::{FileSnapshot, SnapshotDiff};
use std::path::Path;

// ── النتائج ──────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum VerifyResult {
    Success,
    NoChange,            // لم يُكتب أي ملف
    UnexpectedFile,      // كُتب ملف غير متوقع
    EmptyFile(String),   // الملف موجود لكنه فارغ
    PartialWrite(String),// الملف مقطوع — محتوى ناقص
    MissingSymbol {      // رمز متوقع غائب
        file:   String,
        symbol: String,
    },
}

impl VerifyResult {
    pub fn is_success(&self) -> bool {
        matches!(self, VerifyResult::Success)
    }

    pub fn description(&self) -> String {
        match self {
            VerifyResult::Success =>
                "✓ step verified".to_string(),
            VerifyResult::NoChange =>
                "✗ no files changed after step".to_string(),
            VerifyResult::UnexpectedFile =>
                "✗ unexpected file written".to_string(),
            VerifyResult::EmptyFile(f) =>
                format!("✗ file is empty: {}", f),
            VerifyResult::PartialWrite(f) =>
                format!("✗ file appears truncated: {}", f),
            VerifyResult::MissingSymbol { file, symbol } =>
                format!("✗ symbol '{}' missing in {}", symbol, file),
        }
    }
}

// ── الفاحص الرئيسي ───────────────────────────────────────────────

pub struct StepVerifier;

impl StepVerifier {
    /// التحقق الكامل بعد تنفيذ خطوة
    pub fn verify(
        before:          &FileSnapshot,
        after:           &FileSnapshot,
        expected_files:  &[String],   // الملفات التي يجب أن تُكتب
        expected_symbols:&[(&str, &str)], // [(مسار_ملف, رمز_متوقع)]
        workspace:       &Path,
    ) -> VerifyResult {
        let diff = after.diff(before);

        // 1. هل كُتب أي ملف؟
        if expected_files.is_empty() {
            // لا توقعات — تحقق فقط من أن هناك تغييراً
            if diff.is_empty() {
                return VerifyResult::NoChange;
            }
        } else {
            // تحقق من الملفات المتوقعة
            let all_changed: Vec<String> = diff.added.iter()
                .chain(diff.modified.iter())
                .cloned()
                .collect();

            // هل الملفات المتوقعة موجودة في التغييرات؟
            for expected in expected_files {
                let found = all_changed.iter().any(|f| {
                    f == expected || f.ends_with(expected.as_str())
                });
                if !found && diff.is_empty() {
                    return VerifyResult::NoChange;
                }
            }
        }

        // 2. تحقق من كل ملف كُتب
        let changed_files: Vec<String> = diff.added.iter()
            .chain(diff.modified.iter())
            .cloned()
            .collect();

        for file_path in &changed_files {
            let full_path = workspace.join(file_path);
            let result = Self::verify_file_content(&full_path, file_path);
            if !result.is_success() {
                return result;
            }
        }

        // 3. تحقق من الرموز المتوقعة
        for (file, symbol) in expected_symbols {
            let full_path = workspace.join(file);
            if let Ok(content) = std::fs::read_to_string(&full_path) {
                if !content.contains(symbol) {
                    return VerifyResult::MissingSymbol {
                        file:   file.to_string(),
                        symbol: symbol.to_string(),
                    };
                }
            }
        }

        VerifyResult::Success
    }

    /// تحقق من محتوى ملف واحد
    fn verify_file_content(full_path: &Path, rel_path: &str) -> VerifyResult {
        let content = match std::fs::read_to_string(full_path) {
            Ok(c) => c,
            Err(_) => return VerifyResult::Success, // ملف binary أو غير مقروء — تجاهل
        };

        // فارغ؟
        if content.trim().is_empty() {
            return VerifyResult::EmptyFile(rel_path.to_string());
        }

        // مقطوع؟
        let ext = full_path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        if let Some(result) = Self::check_truncation(&content, ext, rel_path) {
            return result;
        }

        VerifyResult::Success
    }

    /// فحص الاقتطاع — ثلاث مراحل
    fn check_truncation(content: &str, ext: &str, rel_path: &str) -> Option<VerifyResult> {
        // المرحلة الأولى: try_parse للغات التي تدعمه
        if let Some(result) = Self::try_parse(content, ext, rel_path) {
            return Some(result);
        }

        // المرحلة الثانية: فحص السطر الأخير
        if let Some(result) = Self::check_last_line(content, ext, rel_path) {
            return Some(result);
        }

        None // لا مشكلة
    }

    /// المرحلة الأولى: تحليل بنية الملف
    fn try_parse(content: &str, ext: &str, rel_path: &str) -> Option<VerifyResult> {
        match ext {
            "json" => {
                // JSON: parse مباشر
                if serde_json::from_str::<serde_json::Value>(content).is_err() {
                    return Some(VerifyResult::PartialWrite(rel_path.to_string()));
                }
            }
            "toml" => {
                // TOML: تحقق من التوازن الأساسي
                if Self::unbalanced_brackets(content) {
                    return Some(VerifyResult::PartialWrite(rel_path.to_string()));
                }
            }
            _ => {}
        }
        None
    }

    /// المرحلة الثانية: فحص السطر الأخير
    fn check_last_line(content: &str, ext: &str, rel_path: &str) -> Option<VerifyResult> {
        let last = content.lines()
            .rev()
            .find(|l| !l.trim().is_empty())?;
        let last = last.trim();

        let truncated = match ext {
            "py" => Self::python_last_line_truncated(last),
            "js" | "ts" => Self::js_last_line_truncated(last),
            "rs" => Self::rust_last_line_truncated(last, content),
            "go" => Self::go_last_line_truncated(last, content),
            _ => false,
        };

        if truncated {
            Some(VerifyResult::PartialWrite(rel_path.to_string()))
        } else {
            None
        }
    }

    // ── Python ───────────────────────────────────────────────────

    fn python_last_line_truncated(last: &str) -> bool {
        // السطر الأخير يشير لاقتطاع
        last.ends_with(':')          // def foo(): بدون جسم
        || last.ends_with(',')       // في منتصف قائمة
        || last.ends_with('(')       // استدعاء لم يُغلق
        || last.ends_with('[')       // قائمة لم تُغلق
        || last.ends_with('{')       // dict لم يُغلق
        || last.ends_with('\\')      // line continuation
        || last.ends_with('+')       // عملية لم تكتمل
        || last.ends_with('=')       // تعيين بدون قيمة
        || last.starts_with("def ")  // دالة بدون جسم (سطر واحد)
            && last.ends_with(':')
        || last.starts_with("class ")
            && last.ends_with(':')
    }

    // ── JavaScript / TypeScript ───────────────────────────────────

    fn js_last_line_truncated(last: &str) -> bool {
        last.ends_with(',')
        || last.ends_with('{')
        || last.ends_with('(')
        || last.ends_with('[')
        || last.ends_with("=>")
        || last.ends_with("&&")
        || last.ends_with("||")
        || last.ends_with('+')
    }

    // ── Rust ─────────────────────────────────────────────────────

    fn rust_last_line_truncated(last: &str, content: &str) -> bool {
        // في Rust: الملف الكامل يجب أن ينتهي بـ }
        // عدّ { و } في مستوى الجذر (خارج strings)
        let opens  = content.chars().filter(|&c| c == '{').count();
        let closes = content.chars().filter(|&c| c == '}').count();

        // إذا الأقواس غير متوازنة بوضوح
        if opens > closes + 2 {
            return true;
        }

        // السطر الأخير لا يجب أن يكون fn أو struct بدون جسم
        last.ends_with("->")
        || last.ends_with("where")
        || (last.starts_with("pub fn") && last.ends_with('{'))
            && !content.ends_with("}\n") && !content.ends_with('}')
    }

    // ── Go ───────────────────────────────────────────────────────

    fn go_last_line_truncated(last: &str, content: &str) -> bool {
        let opens  = content.chars().filter(|&c| c == '{').count();
        let closes = content.chars().filter(|&c| c == '}').count();

        if opens > closes + 2 {
            return true;
        }

        last.ends_with(',')
        || last.ends_with('{')
        || last.ends_with("func")
    }

    // ── أدوات مساعدة ─────────────────────────────────────────────

    fn unbalanced_brackets(content: &str) -> bool {
        let opens  = content.chars().filter(|&c| c == '[').count();
        let closes = content.chars().filter(|&c| c == ']').count();
        let obraces = content.chars().filter(|&c| c == '{').count();
        let cbraces = content.chars().filter(|&c| c == '}').count();
        opens != closes || obraces != cbraces
    }
}

// ── الاختبارات ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_snapshot::FileSnapshot;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_success_when_file_written() {
        let dir = tempdir().unwrap();
        let before = FileSnapshot::take(dir.path());

        fs::write(dir.path().join("main.py"), "def main():\n    pass\n").unwrap();
        let after = FileSnapshot::take(dir.path());

        let result = StepVerifier::verify(
            &before, &after,
            &["main.py".to_string()],
            &[],
            dir.path(),
        );
        assert_eq!(result, VerifyResult::Success);
    }

    #[test]
    fn test_no_change_detected() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("main.py"), "x = 1").unwrap();
        let snap = FileSnapshot::take(dir.path());

        // لا تغيير — نفس الـ snapshot
        let result = StepVerifier::verify(
            &snap, &snap,
            &["main.py".to_string()],
            &[],
            dir.path(),
        );
        assert_eq!(result, VerifyResult::NoChange);
    }

    #[test]
    fn test_empty_file_detected() {
        let dir = tempdir().unwrap();
        let before = FileSnapshot::take(dir.path());

        fs::write(dir.path().join("empty.py"), "").unwrap();
        let after = FileSnapshot::take(dir.path());

        let result = StepVerifier::verify(
            &before, &after, &[], &[], dir.path(),
        );
        assert_eq!(result, VerifyResult::EmptyFile("empty.py".to_string()));
    }

    #[test]
    fn test_truncated_python_detected() {
        let dir = tempdir().unwrap();
        let before = FileSnapshot::take(dir.path());

        // Python مقطوع — السطر الأخير ينتهي بـ :
        fs::write(dir.path().join("svc.py"),
            "class Service:\n    def process(self):\n        result = compute(\n"
        ).unwrap();
        let after = FileSnapshot::take(dir.path());

        let result = StepVerifier::verify(
            &before, &after, &[], &[], dir.path(),
        );
        assert_eq!(result, VerifyResult::PartialWrite("svc.py".to_string()));
    }

    #[test]
    fn test_invalid_json_detected() {
        let dir = tempdir().unwrap();
        let before = FileSnapshot::take(dir.path());

        fs::write(dir.path().join("config.json"),
            r#"{"key": "value", "arr": [1, 2,"#
        ).unwrap();
        let after = FileSnapshot::take(dir.path());

        let result = StepVerifier::verify(
            &before, &after, &[], &[], dir.path(),
        );
        assert_eq!(result, VerifyResult::PartialWrite("config.json".to_string()));
    }

    #[test]
    fn test_missing_symbol_detected() {
        let dir = tempdir().unwrap();
        let before = FileSnapshot::take(dir.path());

        fs::write(dir.path().join("models.py"),
            "class Product:\n    pass\n"
        ).unwrap();
        let after = FileSnapshot::take(dir.path());

        let result = StepVerifier::verify(
            &before, &after,
            &[],
            &[("models.py", "class User")], // User غائب
            dir.path(),
        );
        assert_eq!(result, VerifyResult::MissingSymbol {
            file:   "models.py".to_string(),
            symbol: "class User".to_string(),
        });
    }

    #[test]
    fn test_valid_json_passes() {
        let dir = tempdir().unwrap();
        let before = FileSnapshot::take(dir.path());

        fs::write(dir.path().join("data.json"),
            r#"{"name": "test", "values": [1, 2, 3]}"#
        ).unwrap();
        let after = FileSnapshot::take(dir.path());

        let result = StepVerifier::verify(
            &before, &after, &[], &[], dir.path(),
        );
        assert_eq!(result, VerifyResult::Success);
    }

    #[test]
    fn test_verify_result_descriptions() {
        assert!(VerifyResult::Success.description().contains('✓'));
        assert!(VerifyResult::NoChange.description().contains('✗'));
        assert!(VerifyResult::EmptyFile("x.py".into()).description().contains("x.py"));
        assert!(VerifyResult::PartialWrite("y.rs".into()).description().contains("y.rs"));
        assert!(VerifyResult::MissingSymbol {
            file: "z.py".into(), symbol: "MyClass".into()
        }.description().contains("MyClass"));
    }
}
