
pub fn parse_pytest(output: &str) -> (usize, usize) {
    let mut passed = 0;
    let mut failed = 0;
    for line in output.lines().rev() {
        if line.contains(" passed") || line.contains(" failed") {
            // : "=== 2 passed, 1 failed in 0.03s ==="
            //       
            for seg in line.split(',') {
                let s = seg.trim();
                let words: Vec<&str> = s.split_whitespace().collect();
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

