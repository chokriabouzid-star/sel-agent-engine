// src/bench_compile.rs  SEL Bench Compile Suite v1.1
// : operator precedence, content verification, mutation requirement

use std::path::Path;

pub struct CompileCase {
    pub name: &'static str,
    pub lang: &'static str,
    pub goal: &'static str,
    pub max_repairs: u8,
    pub require_mutation: bool,
}

pub struct CompileCheck {
    pub passed: bool,
    pub repairs: usize,
    pub created_wrong_files: bool,
    pub mutation_ok: bool,
    pub notes: Vec<String>,
}

pub fn setup_case(name: &str, ws: &Path) {
    match name {
        "go_undefined_import" => setup_go_undefined_import(ws),
        "go_unescaped_quotes" => setup_go_unescaped_quotes(ws),
        "go_wrong_logic" => setup_go_wrong_logic(ws),
        "python_module_missing" => setup_python_module_missing(ws),
        "python_wrong_logic" => setup_python_wrong_logic(ws),
        "python_wrong_import" => setup_python_wrong_import(ws),
        "go_syntax_error" => setup_go_syntax_error(ws),
        "python_syntax_then_logic" => setup_python_syntax_then_logic(ws),
        _ => {}
    }
}

pub fn check_result(
    name: &str,
    ws: &Path,
    ok: bool,
    repairs: usize,
    mutation: f64,
) -> CompileCheck {
    let mut result = CompileCheck {
        passed: ok,
        repairs,
        created_wrong_files: false,
        mutation_ok: true,
        notes: Vec::new(),
    };

    //  :      
    match name {
        n if n.starts_with("go_")
            && (path_exists(ws, "*.py") || path_exists(ws, "test_*.py")) => {
                result.created_wrong_files = true;
                result.passed = false;
                result.notes.push("created .py files in Go project".into());
            }
        n if n.starts_with("python_")
            && path_exists(ws, "*.go") => {
                result.created_wrong_files = true;
                result.passed = false;
                result
                    .notes
                    .push("created .go files in Python project".into());
            }
        _ => {}
    }

    //         
    match name {
        "go_undefined_import" => {
            if repairs > 1 {
                result.passed = false;
                result.notes.push(format!("too many repairs: {}", repairs));
            }
            match std::fs::read_to_string(ws.join("main.go")) {
                Ok(content) => {
                    if !content.contains("\"fmt\"") {
                        result.passed = false;
                        result.notes.push("missing import \"fmt\"".into());
                    }
                    if !content.contains("func Hello()") {
                        result.passed = false;
                        result.notes.push("Hello function removed".into());
                    }
                }
                Err(_) => {
                    result.passed = false;
                    result.notes.push("main.go not found".into());
                }
            }
        }

        "go_unescaped_quotes" => {
            if path_exists(ws, "calc.go") || path_exists(ws, "calc_test.go") {
                result.created_wrong_files = true;
                result.passed = false;
                result.notes.push("created unexpected files".into());
            }
            //   main.go   (  test )
            if let Ok(content) = std::fs::read_to_string(ws.join("main.go")) {
                if !content.contains("func Reverse(s string) string") {
                    result.passed = false;
                    result.notes.push("Reverse function was modified".into());
                }
            }
        }

        "go_wrong_logic" => {
            match std::fs::read_to_string(ws.join("main.go")) {
                Ok(content) => {
                    // Add    a + b
                    let has_correct_add = content.contains("a + b")
                        && content.lines().any(|l| {
                            l.contains("Add") && l.contains("func")
                                || (l.contains("return")
                                    && l.contains("a + b")
                                    && !l.contains("Multiply"))
                        });

                    // Multiply    a * b
                    let has_correct_multiply = content.contains("a * b");

                    //     
                    let still_has_subtract = content.lines().any(|l| l.contains("a - b"));

                    if still_has_subtract {
                        result.passed = false;
                        result.notes.push("Add still uses a - b".into());
                    }
                    if !has_correct_multiply {
                        result.passed = false;
                        result.notes.push("Multiply not fixed to a * b".into());
                    }
                    if !has_correct_add && !still_has_subtract {
                        //  
                        result.notes.push("Add implementation unclear".into());
                    }
                }
                Err(_) => {
                    result.passed = false;
                    result.notes.push("main.go not found".into());
                }
            }
            // mutation 
            if mutation < 0.0 {
                result.notes.push("mutation not measured".into());
                //      
            } else if mutation < 1.0 {
                result.mutation_ok = false;
                result.passed = false;
                result
                    .notes
                    .push(format!("mutation {:.0}% < 100%", mutation * 100.0));
            }
        }

        "python_module_missing" => {
            if let Ok(content) = std::fs::read_to_string(ws.join("app.py")) {
                if !content.contains("import requests") && !content.contains("from requests") {
                    result.passed = false;
                    result.notes.push("import requests was removed".into());
                }
            }
        }

        "python_wrong_logic" => {
            if let Ok(content) = std::fs::read_to_string(ws.join("calculator.py")) {
                // divide    /  //
                let has_divide_op = content.lines().any(|l| {
                    l.contains("return")
                        && (l.contains("a / b") || l.contains("a // b"))
                        && !l.contains("a * b")
                });
                // power    **
                let has_power_op = content.contains("**");

                if !has_divide_op {
                    result.passed = false;
                    result.notes.push("divide not fixed".into());
                }
                if !has_power_op {
                    result.passed = false;
                    result.notes.push("power not using **".into());
                }
            }
        }

        "python_wrong_import" => {
            if let Ok(content) = std::fs::read_to_string(ws.join("test_models.py")) {
                if content.contains("wrong_module") {
                    result.passed = false;
                    result.notes.push("still imports from wrong_module".into());
                }
                if !content.contains("from models") {
                    result.passed = false;
                    result.notes.push("not importing from models".into());
                }
            }
            // models.py    
            if let Ok(content) = std::fs::read_to_string(ws.join("models.py")) {
                if !content.contains("class User:") {
                    result.passed = false;
                    result
                        .notes
                        .push("models.py was incorrectly modified".into());
                }
            }
        }

        "go_syntax_error" => {
            if repairs > 2 {
                result.passed = false;
                result.notes.push(format!("too many repairs: {}", repairs));
            }
            //   Fibonacci 
            if let Ok(content) = std::fs::read_to_string(ws.join("main.go")) {
                let open_braces = content.matches('{').count();
                let close_braces = content.matches('}').count();
                if open_braces != close_braces {
                    result.passed = false;
                    result.notes.push("unbalanced braces".into());
                }
            }
        }

        "python_syntax_then_logic" => {
            if repairs > 2 {
                result.passed = false;
                result.notes.push(format!("too many repairs: {}", repairs));
            }
            if let Ok(content) = std::fs::read_to_string(ws.join("processor.py")) {
                //  syntax errors
                if content.contains("== 0\n") && !content.contains("== 0:") {
                    result.passed = false;
                    result.notes.push("missing colon after if".into());
                }
                if content.contains("len(items\n") {
                    result.passed = false;
                    result.notes.push("unclosed paren".into());
                }
            }
        }

        _ => {}
    }

    //     
    if result.notes.is_empty() && result.passed {
        result.notes.push("ok".into());
    }

    result
}

pub fn all_cases() -> Vec<CompileCase> {
    vec![
        CompileCase {
            name: "go_undefined_import",
            lang: "go",
            goal: "Fix the code so tests pass. The main.go uses fmt but doesn't import it. Add the missing import. Run go test.",
            max_repairs: 3,
            require_mutation: false,
        },
        CompileCase {
            name: "go_unescaped_quotes",
            lang: "go",
            goal: "Fix the test file main_test.go  it has unescaped quotes in the Errorf call. Fix ONLY the test file. Do NOT create new files. Run go test.",
            max_repairs: 3,
            require_mutation: false,
        },
        CompileCase {
            name: "go_wrong_logic",
            lang: "go",
            goal: "Fix main.go: Add should return a+b (not a-b), Multiply should return a*b (not a+b). Do NOT modify tests. Run go test.",
            max_repairs: 3,
            require_mutation: true,
        },
        CompileCase {
            name: "python_module_missing",
            lang: "python",
            goal: "Install the missing requests module and make tests pass. Do NOT remove the import. Run pytest.",
            max_repairs: 3,
            require_mutation: false,
        },
        CompileCase {
            name: "python_wrong_logic",
            lang: "python",
            goal: "Fix calculator.py: divide should return a/b (not a*b), power should return base**exp (not base+exp). Do NOT modify tests. Run pytest.",
            max_repairs: 3,
            require_mutation: true,
        },
        CompileCase {
            name: "python_wrong_import",
            lang: "python",
            goal: "Fix test_models.py: it imports from wrong_module but should import from models. Fix ONLY the import. Do NOT modify models.py. Run pytest.",
            max_repairs: 3,
            require_mutation: false,
        },
        CompileCase {
            name: "go_syntax_error",
            lang: "go",
            goal: "Fix main.go: there is a missing closing brace in the Fibonacci function. Fix the syntax error. Run go test.",
            max_repairs: 3,
            require_mutation: false,
        },
        CompileCase {
            name: "python_syntax_then_logic",
            lang: "python",
            goal: "Fix processor.py: it has syntax errors (missing colon and closing paren). Fix syntax first, then ensure logic handles empty list correctly. Run pytest.",
            max_repairs: 3,
            require_mutation: false,
        },
    ]
}

// 
// Setup functions
// 

fn setup_go_undefined_import(ws: &Path) {
    let _ = std::fs::write(
        ws.join("main.go"),
        "package main\n\nfunc Hello() string {\n\treturn fmt.Sprintf(\"hello\")\n}\n",
    );
    let _ = std::fs::write(ws.join("main_test.go"), "package main\n\nimport \"testing\"\n\nfunc TestHello(t *testing.T) {\n\tif Hello() != \"hello\" {\n\t\tt.Errorf(\"got %q\", Hello())\n\t}\n}\n");
    go_mod_init(ws);
}

fn setup_go_unescaped_quotes(ws: &Path) {
    let _ = std::fs::write(ws.join("main.go"), "package main\n\nfunc Reverse(s string) string {\n\trunes := []rune(s)\n\tfor i, j := 0, len(runes)-1; i < j; i, j = i+1, j-1 {\n\t\trunes[i], runes[j] = runes[j], runes[i]\n\t}\n\treturn string(runes)\n}\n");
    let test_content = b"package main\n\nimport \"testing\"\n\nfunc TestReverse(t *testing.T) {\n\tgot := Reverse(\"hello\")\n\tif got != \"olleh\" {\n\t\tt.Errorf(\"Reverse(\"hello\") = %q, want olleh\", got)\n\t}\n}\n";
    let _ = std::fs::write(ws.join("main_test.go"), test_content);
    go_mod_init(ws);
}

fn setup_go_wrong_logic(ws: &Path) {
    let _ = std::fs::write(ws.join("main.go"), "package main\n\nfunc Add(a, b int) int      { return a - b }\nfunc Multiply(a, b int) int { return a + b }\n");
    let _ = std::fs::write(ws.join("main_test.go"), "package main\n\nimport \"testing\"\n\nfunc TestAdd(t *testing.T) {\n\tif got := Add(2, 3); got != 5 {\n\t\tt.Errorf(\"Add(2,3) = %d, want 5\", got)\n\t}\n\tif got := Add(0, 0); got != 0 {\n\t\tt.Errorf(\"Add(0,0) = %d, want 0\", got)\n\t}\n}\n\nfunc TestMultiply(t *testing.T) {\n\tif got := Multiply(3, 4); got != 12 {\n\t\tt.Errorf(\"Multiply(3,4) = %d, want 12\", got)\n\t}\n\tif got := Multiply(0, 5); got != 0 {\n\t\tt.Errorf(\"Multiply(0,5) = %d, want 0\", got)\n\t}\n}\n");
    go_mod_init(ws);
}

fn setup_python_module_missing(ws: &Path) {
    let _ = std::fs::write(
        ws.join("app.py"),
        "import requests\n\ndef fetch(url):\n    return requests.get(url).status_code\n",
    );
    let _ = std::fs::write(ws.join("test_app.py"), "from app import fetch\n\ndef test_fetch_callable():\n    assert callable(fetch)\n\ndef test_fetch_type():\n    assert fetch.__name__ == \"fetch\"\n");
}

fn setup_python_wrong_logic(ws: &Path) {
    let _ = std::fs::write(
        ws.join("calculator.py"),
        "def divide(a, b):\n    return a * b\n\ndef power(base, exp):\n    return base + exp\n",
    );
    let _ = std::fs::write(ws.join("test_calculator.py"), "from calculator import divide, power\n\ndef test_divide():\n    assert divide(10, 2) == 5\n    assert divide(9, 3) == 3\n    assert divide(0, 5) == 0\n\ndef test_power():\n    assert power(2, 3) == 8\n    assert power(3, 2) == 9\n    assert power(5, 0) == 1\n");
}

fn setup_python_wrong_import(ws: &Path) {
    let _ = std::fs::write(ws.join("models.py"), "class User:\n    def __init__(self, name, email):\n        self.name = name\n        self.email = email\n\n    def greet(self):\n        return f\"Hello, {self.name}\"\n");
    let _ = std::fs::write(ws.join("test_models.py"), "from wrong_module import User\n\ndef test_user_creation():\n    u = User(\"Alice\", \"alice@example.com\")\n    assert u.name == \"Alice\"\n    assert u.email == \"alice@example.com\"\n\ndef test_user_greet():\n    u = User(\"Bob\", \"bob@example.com\")\n    assert u.greet() == \"Hello, Bob\"\n");
}

fn setup_go_syntax_error(ws: &Path) {
    let _ = std::fs::write(ws.join("main.go"), "package main\n\nfunc Fibonacci(n int) int {\n\tif n <= 1 {\n\t\treturn n\n\t\n\treturn Fibonacci(n-1) + Fibonacci(n-2)\n}\n");
    let _ = std::fs::write(ws.join("main_test.go"), "package main\n\nimport \"testing\"\n\nfunc TestFibonacci(t *testing.T) {\n\ttests := []struct{ n, want int }{\n\t\t{0, 0}, {1, 1}, {5, 5}, {10, 55},\n\t}\n\tfor _, tt := range tests {\n\t\tif got := Fibonacci(tt.n); got != tt.want {\n\t\t\tt.Errorf(\"Fibonacci(%d) = %d, want %d\", tt.n, got, tt.want)\n\t\t}\n\t}\n}\n");
    go_mod_init(ws);
}

fn setup_python_syntax_then_logic(ws: &Path) {
    let _ = std::fs::write(ws.join("processor.py"), "def process(items):\n    if len(items) == 0\n        return 0\n    return sum(items) / len(items\n");
    let _ = std::fs::write(ws.join("test_processor.py"), "from processor import process\n\ndef test_normal():\n    assert process([1, 2, 3, 4, 5]) == 3.0\n    assert process([10, 20]) == 15.0\n\ndef test_edge():\n    assert process([]) == 0\n    assert process([42]) == 42.0\n");
}

fn go_mod_init(ws: &Path) {
    let _ = std::process::Command::new("go")
        .args(["mod", "init", "gotest"])
        .current_dir(ws)
        .output();
}

fn path_exists(ws: &Path, pattern: &str) -> bool {
    if let Ok(entries) = std::fs::read_dir(ws) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if pattern.starts_with("*.") {
                let ext = &pattern[1..];
                if name.ends_with(ext) {
                    return true;
                }
            } else if pattern.starts_with("test_*.") {
                let ext = &pattern[6..];
                if name.starts_with("test_") && name.ends_with(ext) {
                    return true;
                }
            } else if name == pattern {
                return true;
            }
        }
    }
    false
}
