// src/context_ledger.rs — ContextLedger v1.0
// ذاكرة الوكيل داخل مهمة واحدة — يُضاف إليه فقط، لا يُحذف منه
// الكتابة ذرية: write tmp → verify → rename (ADR-011)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ── الأنواع ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StepStatus {
    Pending,
    Running,
    Done,
    Failed,
    Partial, // أنجز جزئياً — تابع
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletedStep {
    pub name:          String,
    pub status:        StepStatus,
    pub files_created: Vec<String>,
    pub exports:       HashMap<String, Vec<String>>, // مسار → [رموز]
    pub test_result:   Option<bool>,
    pub completed_at:  u64, // unix seconds
    pub repair_count:  u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileIndex {
    pub size:     u64,
    pub exports:  Vec<String>, // دوال وكلاسات مُصدَّرة
    pub imports:  Vec<String>, // وحدات مستوردة
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextLedger {
    pub task_id:          String,
    pub goal:             String,
    pub language:         String,
    pub entry_point:      String,
    pub current_step:     u32,
    pub completed_steps:  Vec<CompletedStep>,
    pub file_index:       HashMap<String, FileIndex>,
    pub last_error:       Option<String>,
    pub last_error_hash:  Option<u32>,
    pub repair_attempts:  u8,
    pub total_tokens:     u32,
    pub started_at:       u64,
    pub updated_at:       u64,
}

// ── التنفيذ ──────────────────────────────────────────────────────

impl ContextLedger {
    pub fn new(goal: &str, language: &str) -> Self {
        let now = now_unix();
        Self {
            task_id:         new_task_id(),
            goal:            goal.to_string(),
            language:        language.to_string(),
            entry_point:     String::new(),
            current_step:    0,
            completed_steps: vec![],
            file_index:      HashMap::new(),
            last_error:      None,
            last_error_hash: None,
            repair_attempts: 0,
            total_tokens:    0,
            started_at:      now,
            updated_at:      now,
        }
    }

    // ── القراءة والكتابة ─────────────────────────────────────────

    /// مسار الـ Ledger داخل المشروع
    pub fn ledger_path(workspace: &Path) -> PathBuf {
        workspace.join(".agent").join("ledger.json")
    }

    /// تحميل من القرص — None إذا لا يوجد أو تالف
    pub fn load(workspace: &Path) -> Option<Self> {
        let path = Self::ledger_path(workspace);

        // تحقق من .tmp أولاً (recovery من كتابة فاشلة)
        let tmp = path.with_extension("json.tmp");
        if tmp.exists() && !path.exists() {
            // الحالة الحرجة: tmp موجود والأصل غير موجود
            if let Ok(content) = std::fs::read_to_string(&tmp) {
                if let Ok(ledger) = serde_json::from_str::<Self>(&content) {
                    // أعِد التسمية واستخدمه
                    let _ = std::fs::rename(&tmp, &path);
                    return Some(ledger);
                }
            }
            // tmp تالف — احذفه
            let _ = std::fs::remove_file(&tmp);
        }

        // القراءة الطبيعية
        let content = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// حفظ ذري: write tmp → verify JSON → rename
    pub fn save(&mut self, workspace: &Path) -> Result<(), String> {
        self.updated_at = now_unix();

        let path = Self::ledger_path(workspace);
        let tmp  = path.with_extension("json.tmp");

        // أنشئ المجلد إذا لم يوجد
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create_dir failed: {}", e))?;
        }

        // اكتب في tmp
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("serialize failed: {}", e))?;

        std::fs::write(&tmp, &json)
            .map_err(|e| format!("write tmp failed: {}", e))?;

        // تحقق من صحة JSON
        serde_json::from_str::<Self>(&json)
            .map_err(|e| {
                let _ = std::fs::remove_file(&tmp);
                format!("verify failed: {}", e)
            })?;

        // rename ذري
        std::fs::rename(&tmp, &path)
            .map_err(|e| format!("rename failed: {}", e))?;

        Ok(())
    }

    // ── التحديث ──────────────────────────────────────────────────

    /// سجّل خطوة مكتملة
    pub fn record_step(&mut self, step: CompletedStep) {
        self.current_step += 1;
        self.completed_steps.push(step);
    }

    /// حدّث فهرس الملفات
    pub fn update_file_index(&mut self, path: &str, index: FileIndex) {
        self.file_index.insert(path.to_string(), index);
    }

    /// سجّل خطأ — مع hash للكشف عن التكرار
    pub fn record_error(&mut self, error: &str) -> bool {
        let hash = fnv1a(error.as_bytes());
        let is_repeat = self.last_error_hash == Some(hash);
        self.last_error      = Some(error.chars().take(500).collect());
        self.last_error_hash = Some(hash);
        self.repair_attempts += 1;
        is_repeat // true = نفس الخطأ يتكرر → صعّد
    }

    /// أضف توكن مستهلكة
    pub fn add_tokens(&mut self, tokens: u32) {
        self.total_tokens += tokens;
    }

    // ── الاستعلام ────────────────────────────────────────────────

    /// هل الخطوة مكتملة؟
    pub fn is_step_done(&self, name: &str) -> bool {
        self.completed_steps.iter()
            .any(|s| s.name == name && s.status == StepStatus::Done)
    }

    /// الخطوات المكتملة بنجاح فقط
    pub fn done_steps(&self) -> Vec<&CompletedStep> {
        self.completed_steps.iter()
            .filter(|s| s.status == StepStatus::Done)
            .collect()
    }

    /// الملفات المنشأة حتى الآن
    pub fn all_created_files(&self) -> Vec<String> {
        let mut files: Vec<String> = self.completed_steps.iter()
            .flat_map(|s| s.files_created.clone())
            .collect();
        files.sort();
        files.dedup();
        files
    }

    /// الرموز المصدَّرة من ملف معين
    pub fn exports_of(&self, path: &str) -> Vec<String> {
        self.file_index.get(path)
            .map(|fi| fi.exports.clone())
            .unwrap_or_default()
    }

    /// ملخص للنموذج — بدون إرسال المحتوى الكامل
    pub fn summary_for_llm(&self) -> String {
        let done: Vec<String> = self.done_steps()
            .iter()
            .map(|s| format!("✓ {}", s.name))
            .collect();

        let files = self.all_created_files();

        format!(
            "Task: {}\nLanguage: {}\nStep: {}\nDone: {}\nFiles: {}\nTokens: {}",
            self.goal,
            self.language,
            self.current_step,
            if done.is_empty() { "none".to_string() } else { done.join(", ") },
            if files.is_empty() { "none".to_string() } else { files.join(", ") },
            self.total_tokens,
        )
    }
}

// ── الأدوات ──────────────────────────────────────────────────────

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn new_task_id() -> String {
    // UUID بسيط بدون dependency
    let t = now_unix();
    format!("task-{:x}", t)
}

fn fnv1a(data: &[u8]) -> u32 {
    let mut hash: u32 = 2_166_136_261;
    for &byte in data {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(16_777_619);
    }
    hash
}

// ── الاختبارات ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_step(name: &str, status: StepStatus) -> CompletedStep {
        CompletedStep {
            name:          name.to_string(),
            status,
            files_created: vec!["src/main.py".into()],
            exports:       HashMap::new(),
            test_result:   Some(true),
            completed_at:  0,
            repair_count:  0,
        }
    }

    #[test]
    fn test_new_ledger() {
        let l = ContextLedger::new("build a CLI", "python");
        assert_eq!(l.current_step, 0);
        assert_eq!(l.language, "python");
        assert!(l.completed_steps.is_empty());
    }

    #[test]
    fn test_atomic_save_and_load() {
        let dir = tempdir().unwrap();
        let mut l = ContextLedger::new("test goal", "rust");
        l.record_step(make_step("setup", StepStatus::Done));

        l.save(dir.path()).unwrap();

        // تحقق من عدم وجود .tmp بعد الحفظ الناجح
        let tmp = ContextLedger::ledger_path(dir.path())
            .with_extension("json.tmp");
        assert!(!tmp.exists(), ".tmp يجب أن يُحذف بعد rename");

        let loaded = ContextLedger::load(dir.path()).unwrap();
        assert_eq!(loaded.goal, "test goal");
        assert_eq!(loaded.current_step, 1);
        assert_eq!(loaded.completed_steps.len(), 1);
    }

    #[test]
    fn test_record_error_detects_repeat() {
        let mut l = ContextLedger::new("goal", "python");
        let first  = l.record_error("ImportError: no module named x");
        let second = l.record_error("ImportError: no module named x");
        let diff   = l.record_error("SyntaxError: invalid syntax");

        assert!(!first,  "أول مرة — ليس تكراراً");
        assert!(second,  "نفس الخطأ — تكرار");
        assert!(!diff,   "خطأ مختلف — ليس تكراراً");
        assert_eq!(l.repair_attempts, 3);
    }

    #[test]
    fn test_done_steps_filter() {
        let mut l = ContextLedger::new("goal", "python");
        l.record_step(make_step("step1", StepStatus::Done));
        l.record_step(make_step("step2", StepStatus::Failed));
        l.record_step(make_step("step3", StepStatus::Done));

        assert_eq!(l.done_steps().len(), 2);
        assert!(l.is_step_done("step1"));
        assert!(!l.is_step_done("step2"));
    }

    #[test]
    fn test_all_created_files_dedup() {
        let mut l = ContextLedger::new("goal", "python");
        let mut s1 = make_step("s1", StepStatus::Done);
        s1.files_created = vec!["a.py".into(), "b.py".into()];
        let mut s2 = make_step("s2", StepStatus::Done);
        s2.files_created = vec!["b.py".into(), "c.py".into()]; // b.py مكرر

        l.record_step(s1);
        l.record_step(s2);

        let files = l.all_created_files();
        assert_eq!(files.len(), 3); // a, b, c — بدون تكرار
    }

    #[test]
    fn test_recovery_from_tmp() {
        let dir = tempdir().unwrap();
        let ledger_path = ContextLedger::ledger_path(dir.path());
        let tmp_path    = ledger_path.with_extension("json.tmp");

        // اكتب tmp بدون ledger.json (محاكاة crash أثناء rename)
        std::fs::create_dir_all(ledger_path.parent().unwrap()).unwrap();
        let mut l = ContextLedger::new("recovered goal", "go");
        l.record_step(make_step("init", StepStatus::Done));
        let json = serde_json::to_string_pretty(&l).unwrap();
        std::fs::write(&tmp_path, json).unwrap();

        // load يجب أن يستعيد من tmp
        let recovered = ContextLedger::load(dir.path()).unwrap();
        assert_eq!(recovered.goal, "recovered goal");

        // tmp يجب أن يُحوَّل لـ ledger.json
        assert!(ledger_path.exists());
        assert!(!tmp_path.exists());
    }

    #[test]
    fn test_summary_for_llm() {
        let mut l = ContextLedger::new("build API", "python");
        l.record_step(make_step("models", StepStatus::Done));
        l.add_tokens(1500);

        let summary = l.summary_for_llm();
        assert!(summary.contains("build API"));
        assert!(summary.contains("python"));
        assert!(summary.contains("1500"));
        assert!(summary.contains("models"));
    }
}
