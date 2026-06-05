// src/pattern_library.rs  v8.9.0 — Pattern Library
//
// يتعلم الوكيل من مهامه السابقة ويتجنب تكرار نفس مسارات الإصلاح
// الفاشلة. كل pattern يُخزّن بـ success/failure counts حتى يكون
// الـ lookup مبنياً على بيانات حقيقية لا افتراضات.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ─────────────────────────────────────────────────────────────────
// RepairRoute  — مشترك مع v9.0 Adaptive Repair
// مُعرَّف هنا مبكراً لتفادي refactor مزدوج لاحقاً
// ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RepairRoute {
    #[default]
    Generic,
    ForceSourceOnly,
    MissingDependency,
    FunctionDeleted,
    NullGuard,
    RustOwnership,
    CircularImport,
    TypeMismatch,
}

impl RepairRoute {
    /// وصف قصير يُضاف إلى repair prompt كـ hint
    pub fn hint(&self) -> &'static str {
        match self {
            Self::Generic => "",
            Self::ForceSourceOnly => {
                "Fix source files only — never touch test files."
            }
            Self::MissingDependency => {
                "Add the missing import or install the missing dependency."
            }
            Self::FunctionDeleted => {
                "The function was deleted — restore it from context or rewrite it."
            }
            Self::NullGuard => {
                "Add a nil/null check before using the value."
            }
            Self::RustOwnership => {
                "Use clone() or a reference to fix the ownership issue."
            }
            Self::CircularImport => {
                "Restructure imports to break the circular dependency."
            }
            Self::TypeMismatch => {
                "Verify the types match — check return type and argument types."
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────
// Pattern
// ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pattern {
    /// معرّف فريد: {language}:{error_signature_hash}
    pub id: String,
    /// اللغة: python, typescript, go, rust, unknown
    pub language: String,
    /// بصمة الخطأ — أول 120 byte من stderr بعد التنظيف
    pub error_signature: String,
    /// مسار الإصلاح المقترح
    pub route: RepairRoute,
    /// عدد مرات النجاح بعد استخدام هذا الـ pattern
    pub success_count: u32,
    /// عدد مرات الفشل بعد استخدام هذا الـ pattern
    pub failure_count: u32,
    /// مجموع مرات الاستخدام (بما فيها حالات لم تُسجَّل نتيجتها)
    pub usage_count: u32,
    /// آخر مرة شُوهد هذا الخطأ (RFC3339)
    pub last_seen_utc: String,
    /// مثال على fix ناجح (أول واحد فقط)
    pub example_fix: Option<String>,
}

impl Pattern {
    /// نسبة النجاح بين 0.0 و 1.0
    pub fn success_rate(&self) -> f32 {
        let total = self.success_count + self.failure_count;
        if total == 0 {
            return 0.0;
        }
        self.success_count as f32 / total as f32
    }

    /// pattern قوي = استُخدم مرتين على الأقل ونجح 70% فأكثر
    pub fn is_strong(&self) -> bool {
        self.usage_count >= 2 && self.success_rate() >= 0.7
    }
}

// ─────────────────────────────────────────────────────────────────
// PatternStore — ما يُخزَّن في patterns.json
// ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternStore {
    pub schema_version: u32,
    pub patterns: Vec<Pattern>,
}

impl PatternStore {
    pub fn new() -> Self {
        Self {
            schema_version: 1,
            patterns: Vec::new(),
        }
    }
}

impl Default for PatternStore {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────
// PatternLibrary — الواجهة الرئيسية
// ─────────────────────────────────────────────────────────────────

pub struct PatternLibrary {
    store: PatternStore,
    path: PathBuf,
}

impl PatternLibrary {
    /// يحمّل من القرص أو يبدأ فارغاً
    pub fn load() -> Self {
        let path = Self::default_path();
        let store = if path.exists() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str::<PatternStore>(&s).ok())
                .unwrap_or_default()
        } else {
            PatternStore::new()
        };
        Self { store, path }
    }

    /// للاختبار: يحمّل من مسار محدد
    pub fn load_from(path: PathBuf) -> Self {
        let store = if path.exists() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str::<PatternStore>(&s).ok())
                .unwrap_or_default()
        } else {
            PatternStore::new()
        };
        Self { store, path }
    }

    /// يبحث عن pattern مطابق وقوي — يرجع أفضل match
    pub fn lookup<'a>(&'a self, language: &str, stderr: &str) -> Option<&'a Pattern> {
        let sig = Self::normalize_signature(stderr);
        if sig.is_empty() {
            return None;
        }

        let mut best: Option<(&Pattern, u32)> = None;

        for pattern in &self.store.patterns {
            // فلترة اللغة — unknown يطابق الجميع
            if pattern.language != "unknown"
                && language != "unknown"
                && pattern.language != language
            {
                continue;
            }

            // يجب أن يكون قوياً
            if !pattern.is_strong() {
                continue;
            }

            let score = Self::match_score(&pattern.error_signature, &sig);
            if score > 0 {
                match best {
                    None => best = Some((pattern, score)),
                    Some((_, best_score)) if score > best_score => {
                        best = Some((pattern, score));
                    }
                    _ => {}
                }
            }
        }

        best.map(|(p, _)| p)
    }

    /// يسجّل نتيجة محاولة إصلاح
    pub fn record_outcome(
        &mut self,
        language: &str,
        stderr: &str,
        route: RepairRoute,
        success: bool,
        fix: Option<String>,
    ) {
        let sig = Self::normalize_signature(stderr);
        if sig.is_empty() {
            return;
        }

        let id = Self::make_id(language, &sig);
        let now = chrono::Utc::now().to_rfc3339();

        if let Some(p) = self.store.patterns.iter_mut().find(|p| p.id == id) {
            // pattern موجود — نحدّثه
            if success {
                p.success_count += 1;
            } else {
                p.failure_count += 1;
            }
            p.usage_count += 1;
            p.last_seen_utc = now;
            if success && p.example_fix.is_none() {
                p.example_fix = fix;
            }
        } else {
            // pattern جديد
            self.store.patterns.push(Pattern {
                id,
                language: language.to_string(),
                error_signature: sig,
                route,
                success_count: if success { 1 } else { 0 },
                failure_count: if success { 0 } else { 1 },
                usage_count: 1,
                last_seen_utc: now,
                example_fix: if success { fix } else { None },
            });
        }
    }

    /// يحفظ إلى القرص — يفشل بصمت حتى لا يكسر الوكيل
    pub fn save(&self) {
        let _ = self.try_save();
    }

    fn try_save(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.store)?;
        std::fs::write(&self.path, json.as_bytes())?;
        Ok(())
    }

    pub fn pattern_count(&self) -> usize {
        self.store.patterns.len()
    }

    pub fn strong_pattern_count(&self) -> usize {
        self.store.patterns.iter().filter(|p| p.is_strong()).count()
    }

    // ─── helpers ───────────────────────────────────────────────

    fn default_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".sel-agent")
            .join("patterns.json")
    }

    /// ينظّف stderr ويستخرج بصمة قابلة للمقارنة:
    /// - يحذف أرقام الأسطر  (line 42)
    /// - يحذف absolute paths  (/home/user/...)
    /// - يحذف timestamps
    /// - يأخذ أول 120 byte
    pub fn normalize_signature(stderr: &str) -> String {
        // خذ السطر الأول غير الفارغ فقط
        let first_line = stderr
            .lines()
            .map(|l| l.trim())
            .find(|l| !l.is_empty())
            .unwrap_or("");

        // احذف أرقام الأسطر مثل ":42:" أو "line 42"
        let mut s = first_line.to_string();

        // احذف paths مطلقة
        let re_path = regex::Regex::new(r"/[^\s:]+").unwrap();
        s = re_path.replace_all(&s, "<path>").to_string();

        // احذف أرقام سطور محددة
        let re_line = regex::Regex::new(r":\d+:\d*").unwrap();
        s = re_line.replace_all(&s, ":<N>").to_string();

        let re_line2 = regex::Regex::new(r"\bline\s+\d+\b").unwrap();
        s = re_line2.replace_all(&s, "line <N>").to_string();

        // خذ أول 120 byte فقط (UTF-8 safe)
        let max = 120;
        if s.len() <= max {
            s.trim().to_string()
        } else {
            let mut end = max;
            while end > 0 && !s.is_char_boundary(end) {
                end -= 1;
            }
            s[..end].trim().to_string()
        }
    }

    /// يحسب درجة التطابق بين بصمتين (0 = لا تطابق)
    pub fn match_score(stored_sig: &str, query_sig: &str) -> u32 {
        if stored_sig.is_empty() || query_sig.is_empty() {
            return 0;
        }
        // تطابق تام
        if stored_sig == query_sig {
            return 100;
        }
        // تطابق جزئي: هل أحدهما prefix للآخر؟
        let min_len = stored_sig.len().min(query_sig.len());
        let common = stored_sig
            .chars()
            .zip(query_sig.chars())
            .take_while(|(a, b)| a == b)
            .count();

        if common >= min_len / 2 {
            // على الأقل 50% من الأقصر متطابق
            (common * 50 / min_len.max(1)) as u32
        } else {
            0
        }
    }

    fn make_id(language: &str, sig: &str) -> String {
        // FNV-1a hash مختصر
        const FNV_OFFSET: u64 = 0xcbf29ce484222325;
        const FNV_PRIME: u64 = 0x100000001b3;
        let mut hash = FNV_OFFSET;
        for byte in sig.as_bytes() {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        format!("{}:{:012x}", language, hash & 0xffffffffffff)
    }
}

pub fn infer_language_from_workspace(workspace: &std::path::Path) -> &'static str {
    if workspace.join("Cargo.toml").exists() {
        "rust"
    } else if workspace.join("go.mod").exists() {
        "go"
    } else if workspace.join("package.json").exists() || workspace.join("tsconfig.json").exists() {
        "typescript"
    } else if workspace.join("pyproject.toml").exists()
        || workspace.join("requirements.txt").exists()
        || workspace.join("setup.py").exists()
    {
        "python"
    } else {
        "unknown"
    }
}

// ─────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_lib(dir: &TempDir) -> PatternLibrary {
        PatternLibrary::load_from(dir.path().join("patterns.json"))
    }

    #[test]
    fn test_lookup_returns_none_when_empty() {
        let dir = TempDir::new().unwrap();
        let lib = make_lib(&dir);
        assert!(lib.lookup("python", "ModuleNotFoundError: No module named 'utils'").is_none());
    }

    #[test]
    fn test_record_and_lookup_after_two_successes() {
        let dir = TempDir::new().unwrap();
        let mut lib = make_lib(&dir);
        let stderr = "ModuleNotFoundError: No module named 'utils'";

        lib.record_outcome("python", stderr, RepairRoute::MissingDependency, true, None);
        // أول مرة: usage_count=1 — ليست قوية بعد
        assert!(lib.lookup("python", stderr).is_none());

        lib.record_outcome("python", stderr, RepairRoute::MissingDependency, true, None);
        // ثاني مرة: usage_count=2, success_rate=1.0 — قوية الآن
        let found = lib.lookup("python", stderr);
        assert!(found.is_some());
        assert_eq!(found.unwrap().route, RepairRoute::MissingDependency);
    }

    #[test]
    fn test_pattern_not_strong_below_threshold() {
        let p = Pattern {
            id: "test:000000000000".to_string(),
            language: "go".to_string(),
            error_signature: "undefined: foo".to_string(),
            route: RepairRoute::FunctionDeleted,
            success_count: 0,
            failure_count: 5,
            usage_count: 5,
            last_seen_utc: "2026-01-01T00:00:00Z".to_string(),
            example_fix: None,
        };
        assert!(!p.is_strong());
        assert_eq!(p.success_rate(), 0.0);
    }

    #[test]
    fn test_pattern_is_strong_after_two_successes() {
        let p = Pattern {
            id: "test:000000000001".to_string(),
            language: "rust".to_string(),
            error_signature: "cannot borrow".to_string(),
            route: RepairRoute::RustOwnership,
            success_count: 3,
            failure_count: 1,
            usage_count: 4,
            last_seen_utc: "2026-01-01T00:00:00Z".to_string(),
            example_fix: None,
        };
        assert!(p.is_strong());
        assert!((p.success_rate() - 0.75).abs() < 0.01);
    }

    #[test]
    fn test_normalize_strips_line_numbers() {
        let stderr = "error[E0502]: cannot borrow at line 42: conflict";
        let sig = PatternLibrary::normalize_signature(stderr);
        assert!(!sig.contains("42"));
        assert!(!sig.is_empty());
    }

    #[test]
    fn test_normalize_strips_paths() {
        let stderr = "error: /home/user/projects/main.rs:10: undefined";
        let sig = PatternLibrary::normalize_signature(stderr);
        assert!(!sig.contains("/home/user"));
    }

    #[test]
    fn test_record_increments_counts() {
        let dir = TempDir::new().unwrap();
        let mut lib = make_lib(&dir);
        let stderr = "undefined: someFunc";
        lib.record_outcome("go", stderr, RepairRoute::FunctionDeleted, true, None);
        lib.record_outcome("go", stderr, RepairRoute::FunctionDeleted, false, None);
        lib.record_outcome("go", stderr, RepairRoute::FunctionDeleted, true, None);

        let p = lib.store.patterns.iter().find(|p| p.language == "go").unwrap();
        assert_eq!(p.success_count, 2);
        assert_eq!(p.failure_count, 1);
        assert_eq!(p.usage_count, 3);
    }

    #[test]
    fn test_save_and_reload_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("patterns.json");

        let mut lib = PatternLibrary::load_from(path.clone());
        lib.record_outcome(
            "typescript",
            "Cannot find module './utils'",
            RepairRoute::MissingDependency,
            true,
            Some("import { foo } from './utils';".to_string()),
        );
        lib.record_outcome(
            "typescript",
            "Cannot find module './utils'",
            RepairRoute::MissingDependency,
            true,
            None,
        );
        lib.save();

        let lib2 = PatternLibrary::load_from(path);
        assert_eq!(lib2.pattern_count(), 1);
        let p = &lib2.store.patterns[0];
        assert_eq!(p.language, "typescript");
        assert_eq!(p.success_count, 2);
        assert!(p.example_fix.is_some());
    }

    #[test]
    fn test_schema_version_preserved() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("patterns.json");
        let mut lib = PatternLibrary::load_from(path.clone());
        lib.record_outcome("python", "SyntaxError: invalid syntax", RepairRoute::Generic, false, None);
        lib.save();
        let lib2 = PatternLibrary::load_from(path);
        assert_eq!(lib2.store.schema_version, 1);
    }

    #[test]
    fn test_match_score_exact() {
        let score = PatternLibrary::match_score("cannot borrow as mutable", "cannot borrow as mutable");
        assert_eq!(score, 100);
    }

    #[test]
    fn test_match_score_no_match() {
        let score = PatternLibrary::match_score("cannot borrow", "undefined variable");
        assert_eq!(score, 0);
    }

    #[test]
    fn test_language_filter_no_cross_match() {
        let dir = TempDir::new().unwrap();
        let mut lib = make_lib(&dir);
        let stderr = "undefined variable x";
        lib.record_outcome("go", stderr, RepairRoute::FunctionDeleted, true, None);
        lib.record_outcome("go", stderr, RepairRoute::FunctionDeleted, true, None);
        // lookup بلغة مختلفة يجب ألا يرجع شيئاً
        assert!(lib.lookup("python", stderr).is_none());
        // lookup بنفس اللغة يرجع
        assert!(lib.lookup("go", stderr).is_some());
    }

    #[test]
    fn test_route_hint_nonempty_for_non_generic() {
        assert!(!RepairRoute::MissingDependency.hint().is_empty());
        assert!(!RepairRoute::CircularImport.hint().is_empty());
        assert!(!RepairRoute::RustOwnership.hint().is_empty());
        // Generic لا hint
        assert!(RepairRoute::Generic.hint().is_empty());
    }
}

