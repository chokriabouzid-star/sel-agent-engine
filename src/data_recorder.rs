// src/data_recorder.rs — DataRecorder v1.0
// يسجّل كل استدعاء LLM في JSONL للتدريب المستقبلي

use serde::Serialize;
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Serialize)]
pub struct LlmRecord {
    pub call_type:        String,   // "generate" | "repair" | "plan" | "verify"
    pub input:            String,
    pub output:           String,
    pub success:          bool,
    pub tokens_estimated: u32,
    pub latency_ms:       u64,
    pub model:            String,
    pub language:         String,   // "python" | "rust" | "node" | "go" | ""
    pub attempt_number:   u8,       // 1 = أول محاولة، 2+ = إصلاح
    pub prior_error:      String,   // الخطأ الذي سبق هذا الاستدعاء (فارغ إذا لا يوجد)
    pub step_in_plan:     u32,      // رقم الخطوة داخل المشروع
    pub tests_passed:     Option<bool>, // هل اجتاز الاختبارات؟
    pub quality_source:   String,   // "test_pass" | "manual" | "unknown"
    pub timestamp:        u64,      // unix seconds
}

pub struct DataRecorder {
    file_path: PathBuf,
}

impl DataRecorder {
    /// يفتح أو ينشئ ملف JSONL في ~/.sel-agent/data/sessions.jsonl
    pub fn new() -> Self {
        let path = Self::default_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        Self { file_path: path }
    }

    pub fn with_path(path: PathBuf) -> Self {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        Self { file_path: path }
    }

    fn default_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home)
            .join(".sel-agent")
            .join("data")
            .join("sessions.jsonl")
    }

    /// يكتب سجل واحد — يفشل بصمت لا يوقف الوكيل
    pub fn record(&self, rec: &LlmRecord) {
        let line = match serde_json::to_string(rec) {
            Ok(l) => l,
            Err(_) => return,
        };
        // append-only — الأسلم للكتابة المتزامنة
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)
        {
            let _ = writeln!(f, "{}", line);
        }
    }

    /// عدد السجلات المحفوظة حتى الآن
    pub fn count(&self) -> usize {
        std::fs::read_to_string(&self.file_path)
            .map(|s| s.lines().count())
            .unwrap_or(0)
    }

    /// حجم الملف بالبايت
    pub fn size_bytes(&self) -> u64 {
        std::fs::metadata(&self.file_path)
            .map(|m| m.len())
            .unwrap_or(0)
    }
}

/// unix timestamp بالثواني
pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// تقدير التوكن: 1 توكن ≈ 4 حرف
pub fn estimate_tokens(text: &str) -> u32 {
    (text.len() / 4) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_record_and_count() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.jsonl");
        let recorder = DataRecorder::with_path(path);

        assert_eq!(recorder.count(), 0);

        recorder.record(&LlmRecord {
            call_type:        "generate".into(),
            input:            "write a function".into(),
            output:           "def foo(): pass".into(),
            success:          true,
            tokens_estimated: 10,
            latency_ms:       300,
            model:            "gemini-flash".into(),
            language:         "python".into(),
            attempt_number:   1,
            prior_error:      "".into(),
            step_in_plan:     1,
            tests_passed:     Some(true),
            quality_source:   "test_pass".into(),
            timestamp:        now_unix(),
        });

        assert_eq!(recorder.count(), 1);
    }

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens("hello"), 1);
        assert_eq!(estimate_tokens("hello world foo bar"), 4);
    }

    #[test]
    fn test_silent_on_bad_path() {
        // مسار مستحيل — يجب أن يفشل بصمت
        let recorder = DataRecorder::with_path(
            PathBuf::from("/root/impossible/path/file.jsonl")
        );
        // لا panic — يكتب بصمت
        recorder.record(&LlmRecord {
            call_type: "test".into(),
            input: "x".into(),
            output: "y".into(),
            success: false,
            tokens_estimated: 0,
            latency_ms: 0,
            model: "test".into(),
            language: "".into(),
            attempt_number: 1,
            prior_error: "".into(),
            step_in_plan: 0,
            tests_passed: None,
            quality_source: "unknown".into(),
            timestamp: 0,
        });
    }
}
