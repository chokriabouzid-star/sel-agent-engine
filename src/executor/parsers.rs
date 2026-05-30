pub fn parse_pytest(output: &str) -> (usize, usize) {
    let mut passed = 0;
    let mut failed = 0;

    for line in output.lines().rev() {
        if line.contains(" passed") || line.contains(" failed") {
            for seg in line.split(',') {
                let words: Vec<&str> = seg.split_whitespace().collect();
                for (i, w) in words.iter().enumerate() {
                    if *w == "passed" && i > 0 {
                        if let Ok(n) = words[i - 1].parse::<usize>() {
                            passed = n;
                        }
                    }
                    if *w == "failed" && i > 0 {
                        if let Ok(n) = words[i - 1].parse::<usize>() {
                            failed = n;
                        }
                    }
                }
            }
            break;
        }
    }

    (passed, failed)
}

pub fn parse_rust_tests(output: &str) -> (usize, usize) {
    let mut total_passed = 0usize;
    let mut total_failed = 0usize;

    for line in output.lines() {
        if line.contains("test result:") {
            for seg in line.split(';') {
                let s = seg.trim();
                let words: Vec<&str> = s.split_whitespace().collect();
                for (i, w) in words.iter().enumerate() {
                    if *w == "passed" && i > 0 {
                        if let Ok(n) = words[i - 1].parse::<usize>() {
                            total_passed += n;
                        }
                    }
                    if *w == "failed" && i > 0 {
                        if let Ok(n) = words[i - 1].parse::<usize>() {
                            total_failed += n;
                        }
                    }
                }
            }
        }
    }

    (total_passed, total_failed)
}

pub fn parse_go_tests(output: &str) -> (usize, usize) {
    let mut passed = 0usize;
    let mut failed = 0usize;

    for line in output.lines() {
        if line.starts_with("--- PASS") {
            passed += 1;
        }
        if line.starts_with("--- FAIL") {
            failed += 1;
        }
    }

    (passed, failed)
}

pub fn parse_jest(output: &str) -> (usize, usize) {
    for line in output.lines().rev() {
        let line = line.trim();

        if let Some(rest) = line.strip_prefix("Tests:") {
            let mut passed = 0usize;
            let mut failed = 0usize;

            for seg in rest.split(',') {
                let words: Vec<&str> = seg.split_whitespace().collect();
                for (i, w) in words.iter().enumerate() {
                    if *w == "passed" && i > 0 {
                        if let Ok(n) = words[i - 1].parse::<usize>() {
                            passed = n;
                        }
                    }
                    if *w == "failed" && i > 0 {
                        if let Ok(n) = words[i - 1].parse::<usize>() {
                            failed = n;
                        }
                    }
                }
            }

            return (passed, failed);
        }
    }

    let passed = output
        .lines()
        .filter(|l| l.trim_start().starts_with("PASS "))
        .count();

    let failed = output
        .lines()
        .filter(|l| l.trim_start().starts_with("FAIL "))
        .count();

    (passed, failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pytest_summary() {
        let out = "================ 2 passed, 1 failed in 0.03s ================";
        assert_eq!(parse_pytest(out), (2, 1));
    }

    #[test]
    fn parse_jest_summary_pass_only() {
        let out = r#"
Test Suites: 2 passed, 2 total
Tests:       12 passed, 12 total
Snapshots:   0 total
Time:        1.23 s
"#;
        assert_eq!(parse_jest(out), (12, 0));
    }

    #[test]
    fn parse_jest_summary_with_failures() {
        let out = r#"
Test Suites: 1 failed, 1 passed, 2 total
Tests:       1 failed, 11 passed, 12 total
Snapshots:   0 total
Time:        1.23 s
"#;
        assert_eq!(parse_jest(out), (11, 1));
    }

    #[test]
    fn parse_jest_fallback_pass_fail_lines() {
        let out = r#"
PASS src/math.test.ts
FAIL src/api.test.ts
"#;
        assert_eq!(parse_jest(out), (1, 1));
    }
}
