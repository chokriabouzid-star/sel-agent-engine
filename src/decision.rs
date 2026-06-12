// src/decision.rs  v9.2.5: Decision & Validation Logic — mod facade

mod goal;
mod plan_risk;
mod validators;
mod context_builders;
mod checklist;

pub use self::goal::{goal_advisory_hints, validate_goal, GoalClarity};
pub use self::plan_risk::evaluate_plan_risk;
pub use self::validators::{
    validate_patch_uniqueness,
    validate_plan_integrity,
    validate_protected_writes,
};
pub use self::context_builders::{
    build_lang_hint, build_ref_context, build_skeleton_context, build_workspace_context,
};
pub use self::checklist::{pre_repair_checklist, ChecklistResult};
