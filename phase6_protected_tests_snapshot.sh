#!/usr/bin/env bash
set -euo pipefail

step() {
  echo
  echo "============================================================"
  echo "== $1"
  echo "============================================================"
}

check_build() {
  echo '$ cargo check'
  cargo check
  echo "✅ check passed"
}

step "0) backups"
for f in src/executor/core.rs src/executor/file_ops.rs src/agent.rs; do
  cp "$f" "$f.bak6"
  echo "backup: $f.bak6"
done

step "1) patch src/executor/core.rs"
python3 - <<'PYEOF'
from pathlib import Path

p = Path("src/executor/core.rs")
c = p.read_text()

# imports
old = 'use std::collections::HashMap;'
new = 'use std::collections::{HashMap, HashSet};'
if old in c:
    c = c.replace(old, new)

# struct field
old = '''    pub replay_mode: bool, // v7.6.1: prevents internet access during replay
    pub bench_mode: bool,  // v8.5: protect existing test files in repair/bench mode
    pub patch_attempts: std::cell::RefCell<HashMap<PathBuf, usize>>, // v5.2: track patch failures
}'''
new = '''    pub replay_mode: bool, // v7.6.1: prevents internet access during replay
    pub bench_mode: bool,  // v8.5: run-mode hint
    pub protected_test_files: HashSet<PathBuf>, // tests present before agent writes anything
    pub patch_attempts: std::cell::RefCell<HashMap<PathBuf, usize>>, // v5.2: track patch failures
}'''
if old not in c:
    raise SystemExit("struct field block not found in src/executor/core.rs")
c = c.replace(old, new)

# constructor
old = '''            replay_mode: false,
            bench_mode: false,
            patch_attempts: std::cell::RefCell::new(HashMap::new()),
        }'''
new = '''            replay_mode: false,
            bench_mode: false,
            protected_test_files: HashSet::new(),
            patch_attempts: std::cell::RefCell::new(HashMap::new()),
        }'''
if old not in c:
    raise SystemExit("constructor block not found in src/executor/core.rs")
c = c.replace(old, new)

p.write_text(c)
print("✅ patched src/executor/core.rs")
PYEOF

check_build

step "2) patch src/executor/file_ops.rs"
python3 - <<'PYEOF'
from pathlib import Path

p = Path("src/executor/file_ops.rs")
c = p.read_text()

old = '''    fn blocks_existing_spec_modification(&self, path: &str, p: &std::path::Path) -> bool {
        // Only protect test files in bench/repair mode (bench_mode = true)
        // In create/smoke mode (bench_mode = false), the agent writes both source and tests
        self.bench_mode && p.exists() && self.is_spec_file(path)
    }'''
new = '''    fn blocks_existing_spec_modification(&self, path: &str, p: &std::path::Path) -> bool {
        self.is_spec_file(path) && self.protected_test_files.contains(p)
    }'''
if old not in c:
    raise SystemExit("blocks_existing_spec_modification not found in src/executor/file_ops.rs")
c = c.replace(old, new)

c = c.replace(
    'self.blocks_existing_spec_modification(path, &p) && p.exists(),',
    'self.blocks_existing_spec_modification(path, &p),'
)
c = c.replace(
    'self.blocks_existing_spec_modification(path, &p) && p.exists())',
    'self.blocks_existing_spec_modification(path, &p))'
)

p.write_text(c)
print("✅ patched src/executor/file_ops.rs")
PYEOF

check_build

step "3) patch src/agent.rs to snapshot protected tests before planning/execution"
python3 - <<'PYEOF'
from pathlib import Path

p = Path("src/agent.rs")
c = p.read_text()

old = '''        let mut has_tests = false;
        for entry in walkdir::WalkDir::new(&ws)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file() {
                let name = entry.file_name().to_string_lossy();
                if name.starts_with("test_")
                    || name.ends_with("_test.go")
                    || name.ends_with(".test.ts")
                    || name.ends_with(".spec.ts")
                    || name.ends_with("test.py")
                    || name.ends_with("test.rs")
                {
                    has_tests = true;
                    break;
                }
                if name.ends_with(".rs") {
                    if let Ok(content) = std::fs::read_to_string(entry.path()) {
                        if content.contains("#[test]") || content.contains("#[cfg(test)]") {
                            has_tests = true;
                            break;
                        }
                    }
                }
            }
        }'''

new = '''        let mut has_tests = false;
        self.executor.protected_test_files.clear();

        for entry in walkdir::WalkDir::new(&ws)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file() {
                let name = entry.file_name().to_string_lossy();

                if self.executor.is_spec_file(&name) {
                    has_tests = true;
                    self.executor
                        .protected_test_files
                        .insert(entry.path().to_path_buf());
                    continue;
                }

                if name.ends_with(".rs") {
                    if let Ok(content) = std::fs::read_to_string(entry.path()) {
                        if content.contains("#[test]") || content.contains("#[cfg(test)]") {
                            has_tests = true;
                        }
                    }
                }
            }
        }

        if !self.executor.protected_test_files.is_empty() {
            eprintln!(
                "[TRACE] protected test files snapshot: {}",
                self.executor.protected_test_files.len()
            );
        }'''

if old not in c:
    raise SystemExit("preflight test scan block not found in src/agent.rs")
c = c.replace(old, new)

p.write_text(c)
print("✅ patched src/agent.rs")
PYEOF

check_build

step "4) format + validate"
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all --all-features
cargo build --release

step "5) show snapshot-related snippets"
echo
echo "--- core.rs ---"
sed -n '45,85p' src/executor/core.rs
echo
echo "--- file_ops.rs ---"
sed -n '1,40p' src/executor/file_ops.rs
echo
echo "--- agent.rs ---"
sed -n '190,245p' src/agent.rs

echo
echo "✅ phase6_protected_tests_snapshot.sh completed"
