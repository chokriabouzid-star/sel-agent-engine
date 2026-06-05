//! # SEL Agent
//!
//! **S**oftware **E**ngineering **L**ab Agent  an autonomous coding agent that
//! uses LLM providers to generate, repair, and validate code across multiple
//! programming languages (Go, Rust, TypeScript, Python, JavaScript).
//!
//! ## Architecture
//!
//! ```text
//!
//!    CLI/main      Executor      LLM Provider
//!
//!
//!
//!
//!
//!                Repair         Protocol
//!                Strategy       Parser
//!
//! ```
//!
//! ## Modules
//!
//! - [`executor`]         Core execution engine: file I/O, compile-check, test runner
//! - [`repair_strategy`]  Escalating repair prompts with loop detection
//! - [`protocol`]         LLM response protocol parsing and validation
//! - [`json_sanitizer`]   Robust JSON extraction from noisy LLM output
//! - [`provider`]         Smart Provider Orchestra (SPO) for multi-LLM management
//! - [`diagnostic`]       Error diagnosis and hint generation
//! - [`constitution`]     Agent behavioral rules and constraints

pub mod constitution;
pub mod dependency_graph;
pub mod diagnostic;
pub mod executor;
pub mod failure;
pub mod json_sanitizer;
pub mod protocol;
pub mod provider;
pub mod repair_strategy;
pub mod types;
pub mod workspace_oracle;
