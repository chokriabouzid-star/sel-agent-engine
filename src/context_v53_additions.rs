// ============================================================
// context_v53_additions.rs — مرجع للتعديلات على context.rs
// لا تُضف هذا الملف للـ module tree — هو للقراءة فقط
// طبّق التعديلات يدوياً على context.rs الموجود
// ============================================================
 
// ─── الخطوة أ: أضف هذه الـ imports في أعلى context.rs ───────
 
// use crate::chunker::{
//     extract_error_locations, get_file_content_smart,
//     estimate_context_tokens, SmartContent, MAX_FILE_LINES,
// };
 
// ─── الخطوة ب: البنية الجديدة لنتيجة build_context ───────────
 
pub struct ContextResult {
    pub context_text: String,
    pub chunk_hints: Vec<String>,
    pub estimated_tokens: usize,
    pub used_chunking: bool,
}
 
// ─── الخطوة ج: الدالة الجديدة — أضفها في context.rs ─────────
//
// استدعيها من agent.rs/executor.rs بدل build_context القديمة
// المعاملات الجديدة الوحيدة: test_output
//
// pub fn build_context_v53(
//     workspace: &Path,
//     test_output: &str,          // ← الجديد
//     focus_paths: Option<&[String]>,
//     ref_file: Option<&Path>,
//     max_files: usize,
// ) -> Result<ContextResult, String> {
//
//     let error_locs = extract_error_locations(test_output);
//
//     if !error_locs.is_empty() {
//         eprintln!("[SEL v5.3] مواقع الأخطاء:");
//         for loc in &error_locs {
//             eprintln!("  {} سطر {}", loc.file, loc.line);
//         }
//     }
//
//     let files = collect_workspace_files(workspace, focus_paths, max_files)?;
//     let mut context_parts: Vec<String> = Vec::new();
//     let mut chunk_hints: Vec<String> = Vec::new();
//     let mut used_chunking = false;
//
//     // معالجة --ref-file
//     if let Some(ref_path) = ref_file {
//         let ref_name = ref_path.file_name()
//             .map(|n| n.to_string_lossy().to_string())
//             .unwrap_or("ref-file".into());
//         let smart = get_file_content_smart(ref_path, &error_locs)?;
//         if smart.is_chunk() { used_chunking = true; }
//         if let Some(hint) = smart.context_hint(&ref_name) { chunk_hints.push(hint); }
//         context_parts.push(format!("### [REF] {}\n```\n{}\n```",
//             ref_name, smart.content_for_prompt(&ref_name)));
//     }
//
//     // معالجة ملفات workspace
//     for (file_path, _) in &files {
//         let path = workspace.join(file_path);
//         let smart = get_file_content_smart(&path, &error_locs)
//             .unwrap_or_else(|_| SmartContent::FullFile(
//                 fs::read_to_string(&path).unwrap_or_default()
//             ));
//         if smart.is_chunk() { used_chunking = true; }
//         if let Some(hint) = smart.context_hint(file_path) { chunk_hints.push(hint); }
//         let content = smart.content_for_prompt(file_path);
//         if !content.is_empty() {
//             context_parts.push(format!("### {}\n```\n{}\n```", file_path, content));
//         }
//     }
//
//     let context_text = context_parts.join("\n\n");
//     let estimated_tokens = context_text.len() / 4;
//
//     if used_chunking {
//         eprintln!("[SEL v5.3] chunking مُفعّل → tokens مقدّرة: ~{}", estimated_tokens);
//     }
//
//     Ok(ContextResult { context_text, chunk_hints, estimated_tokens, used_chunking })
// }
 
// ─── الخطوة د: تعديل بناء الـ system prompt في agent.rs ──────
//
// ابحث عن مكان بناء الـ prompt وأضف:
//
// if !context.chunk_hints.is_empty() {
//     system_prompt.push_str("\n\n## ⚠️ ملفات مقطوعة — اقرأ هذا أولاً\n");
//     for hint in &context.chunk_hints {
//         system_prompt.push_str(&format!("- {}\n", hint));
//     }
//     system_prompt.push_str(
//         "\nعند كتابة patch_file، استخدم أرقام الأسطر الظاهرة في الكود.\n\
//          لا تبدأ العد من 1 إذا كان الـ chunk يبدأ من سطر آخر.\n"
//     );
// }
