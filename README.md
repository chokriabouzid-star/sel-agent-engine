# SEL Agent

**Autonomous execution engine for code generation, testing, and repair.**

[![CI](https://github.com/chokriabouzid-star/sel-agent-engine/actions/workflows/ci.yml/badge.svg)](https://github.com/chokriabouzid-star/sel-agent-engine/actions)

## Version
**v9.3.5**

## What is SEL Agent?

SEL Agent is an autonomous coding agent written in Rust. It takes a natural
language goal, generates a plan, writes code, runs tests, and repairs failures
automatically.

## Supported Languages
- Go
- Rust
- TypeScript / JavaScript
- Python

## Quick Start

```bash
# Build
cargo build --release

# Run a task
SEL_API_KEY="your-key" ./target/release/sel-agent run \
  --workspace ./my_project \
  --goal "Create a calculator in Go"

# Run benchmarks (replay mode, no API needed)
./target/release/sel-agent bench --suite all --replay
./target/release/sel-agent bench-swe --lang all --replay
Environment Variables
Table
Variable	Required	Description
SEL_API_KEY	Yes	LLM API key (OpenRouter/Groq/Gemini)
SEL_MODEL	No	Model name (default: openai/gpt-oss-120b)
SEL_API_BASE	No	Custom API base URL
Quality Metrics
Table
Suite	Status
Smoke	12/12 ✅
Bench All	36/36 ✅
Bench SWE	28/30
Bench SEL-v11	18/18 ✅
Bench Real-World	14/14 ✅
Clippy	0 warnings ✅
Documentation
Architecture
Benchmark Shortcuts
Roadmap
License
MIT
