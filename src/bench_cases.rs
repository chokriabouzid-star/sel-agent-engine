// src/bench_cases.rs — v7.9.9: True bugfix benchmark tasks
use crate::types::BenchCase;

pub fn all_cases() -> Vec<BenchCase> {
    vec![
        // ═══════════════════════════════════════════════════════
        // Python BUGFIX Tasks (01-12) — broken code provided
        // ═══════════════════════════════════════════════════════

        // 01: broken import — typo in module name
        BenchCase::bugfix("broken import", "python",
            "Fix the import error in app.py. The module is called math_utils (not maths_utils). Do NOT rewrite files from scratch — use patch_file. Run pytest.",
            vec![
                ("math_utils.py", "def add(a, b):\n    return a + b\n"),
                ("app.py", "from maths_utils import add\n\ndef compute(x, y):\n    return add(x, y)\n"),
                ("test_app.py", "from app import compute\n\ndef test_compute():\n    assert compute(2, 3) == 5\n\ndef test_compute_negative():\n    assert compute(-1, 1) == 0\n"),
            ],
        ),

        // 02: wrong logic — returns x*3 instead of x*2
        BenchCase::bugfix("wrong logic", "python",
            "Fix the bug in double.py. The function should return x*2 but it returns x*3. Do NOT modify test_double.py. Run pytest.",
            vec![
                ("double.py", "def double(x):\n    return x * 3\n"),
                ("test_double.py", "from double import double\n\ndef test_double_positive():\n    assert double(3) == 6\n\ndef test_double_zero():\n    assert double(0) == 0\n"),
            ],
        ),

        // 03: wrong signature — missing parameter
        BenchCase::bugfix("wrong signature", "python",
            "Fix greet.py. The function should accept a name parameter and return f'Hi {name}', but it currently takes no arguments. Do NOT modify test_greet.py. Run pytest.",
            vec![
                ("greet.py", "def greet():\n    return 'Hi'\n"),
                ("test_greet.py", "from greet import greet\n\ndef test_greet_alice():\n    assert greet('Alice') == 'Hi Alice'\n\ndef test_greet_bob():\n    assert greet('Bob') == 'Hi Bob'\n"),
            ],
        ),

        // 04: inverted logic — n%2==1 instead of n%2==0
        BenchCase::bugfix("inverted logic", "python",
            "Fix is_even.py. The logic is inverted — it returns True for odd numbers. Do NOT modify test_is_even.py. Run pytest.",
            vec![
                ("is_even.py", "def is_even(n):\n    return n % 2 == 1\n"),
                ("test_is_even.py", "from is_even import is_even\n\ndef test_even():\n    assert is_even(4) == True\n\ndef test_odd():\n    assert is_even(3) == False\n\ndef test_zero():\n    assert is_even(0) == True\n"),
            ],
        ),

        // 05: type mismatch — string concat instead of int addition
        BenchCase::bugfix("type mismatch", "python",
            "Fix add.py. It concatenates strings instead of adding integers. Do NOT modify test_add.py. Run pytest.",
            vec![
                ("add.py", "def add(a, b):\n    return str(a) + str(b)\n"),
                ("test_add.py", "from add import add\n\ndef test_add_positive():\n    assert add(2, 3) == 5\n\ndef test_add_negative():\n    assert add(-1, 1) == 0\n"),
            ],
        ),

        // 06: recursion bug — factorial(n) instead of factorial(n-1)
        BenchCase::bugfix("recursion bug", "python",
            "Fix the infinite recursion bug in factorial.py. It calls factorial(n) instead of factorial(n-1). Do NOT modify test_factorial.py. Run pytest.",
            vec![
                ("factorial.py", "def factorial(n):\n    if n == 0:\n        return 1\n    return n * factorial(n)\n"),
                ("test_factorial.py", "from factorial import factorial\n\ndef test_factorial_5():\n    assert factorial(5) == 120\n\ndef test_factorial_0():\n    assert factorial(0) == 1\n\ndef test_factorial_1():\n    assert factorial(1) == 1\n"),
            ],
        ),

        // 07: function name typo — helpr() vs helper()
        BenchCase::bugfix("name typo", "python",
            "Fix helper.py. The function is named 'helpr' (typo) but should be 'helper'. Do NOT modify test_helper.py. Run pytest.",
            vec![
                ("helper.py", "def helpr():\n    return 42\n"),
                ("test_helper.py", "from helper import helper\n\ndef test_helper_value():\n    assert helper() == 42\n"),
            ],
        ),

        // 08: min instead of max
        BenchCase::bugfix("wrong function", "python",
            "Fix max_of_three.py. It uses min() instead of max(). Do NOT modify test_max.py. Run pytest.",
            vec![
                ("max_of_three.py", "def max_of_three(a, b, c):\n    return min(a, b, c)\n"),
                ("test_max.py", "from max_of_three import max_of_three\n\ndef test_max_basic():\n    assert max_of_three(1, 5, 3) == 5\n\ndef test_max_negative():\n    assert max_of_three(-1, -5, -3) == -1\n\ndef test_max_equal():\n    assert max_of_three(7, 7, 7) == 7\n"),
            ],
        ),

        // 09: syntax error — missing colon
        BenchCase::bugfix("syntax error", "python",
            "Fix the syntax error in calculator.py — the function definition is missing a colon. Do NOT modify test_calculator.py. Run pytest.",
            vec![
                ("calculator.py", "def add(a, b)\n    return a + b\n"),
                ("test_calculator.py", "from calculator import add\n\ndef test_add():\n    assert add(2, 3) == 5\n\ndef test_add_floats():\n    assert add(1.5, 2.5) == 4.0\n"),
            ],
        ),

        // 10: wrong return — returns input instead of reversed
        BenchCase::bugfix("wrong return", "python",
            "Fix reverse_string.py. It returns the input unchanged instead of reversing it. Do NOT modify test_reverse.py. Run pytest.",
            vec![
                ("reverse_string.py", "def reverse_string(s):\n    return s\n"),
                ("test_reverse.py", "from reverse_string import reverse_string\n\ndef test_reverse_hello():\n    assert reverse_string('hello') == 'olleh'\n\ndef test_reverse_empty():\n    assert reverse_string('') == ''\n"),
            ],
        ),

        // 11: missing method — Stack has push but no pop
        BenchCase::bugfix("missing method", "python",
            "Fix stack.py by adding the missing pop() method. It should remove and return the last item, or return None if empty. Do NOT modify test_stack.py. Run pytest.",
            vec![
                ("stack.py", "class Stack:\n    def __init__(self):\n        self.items = []\n\n    def push(self, item):\n        self.items.append(item)\n"),
                ("test_stack.py", "from stack import Stack\n\ndef test_push_pop():\n    s = Stack()\n    s.push(1)\n    s.push(2)\n    assert s.pop() == 2\n    assert s.pop() == 1\n\ndef test_pop_empty():\n    s = Stack()\n    assert s.pop() is None\n"),
            ],
        ),

        // 12: runtime error — no zero division guard
        BenchCase::bugfix("runtime error", "python",
            "Fix divide.py — when b is zero it returns 999 instead of None. Fix the return value. Do NOT modify test_divide.py. Run pytest.",
            vec![
                ("divide.py", "def divide(a, b):\n    if b == 0:\n        return 999\n    return a / b\n"),
                ("test_divide.py", "from divide import divide\n\ndef test_divide_normal():\n    assert divide(10, 2) == 5.0\n\ndef test_divide_zero():\n    assert divide(10, 0) is None\n"),
            ],
        ),

        // ═══════════════════════════════════════════════════════
        // Go — Bugfix Tasks (Moved from Creation to Bugfix)
        // ═══════════════════════════════════════════════════════
        BenchCase::bugfix("go add", "go", 
            "Fix Add. Write test in main_test.go testing Add(2,3)==5 and Add(-1,1)==0. Run go test.",
            vec![
                ("go.mod", "module gotest\n\ngo 1.21\n"),
                ("main.go", "package main\n\nfunc Add(a, b int) int {\n    return a - b\n}\n"),
            ]
        ),
        BenchCase::bugfix("go fizzbuzz", "go", 
            "Fix FizzBuzz(n int) string. Write test in main_test.go with 4 test cases using only t.Errorf. Run go test.",
            vec![
                ("go.mod", "module gotest\n\ngo 1.21\n"),
                ("main.go", "package main\n\nimport \"strconv\"\n\nfunc FizzBuzz(n int) string {\n    if n%3 == 0 { return \"Fizz\" }\n    if n%5 == 0 { return \"Buzz\" }\n    return strconv.Itoa(n)\n}\n"),
            ]
        ),
        BenchCase::bugfix("go reverse", "go", 
            "Fix Reverse — it appends 'WRONG' to the result. Remove the appended string. Write test in main_test.go testing Reverse(\"hello\")==\"olleh\" and Reverse(\"\")==\"\". Run go test.",
            vec![
                ("go.mod", "module gotest\n\ngo 1.21\n"),
                ("main.go", "package main\n\nfunc Reverse(s string) string {\n\trunes := []rune(s)\n\tfor i, j := 0, len(runes)-1; i < j; i, j = i+1, j-1 {\n\t\trunes[i], runes[j] = runes[j], runes[i]\n\t}\n\treturn string(runes) + \"WRONG\"\n}\n"),
            ]
        ),
        BenchCase::bugfix("go divide", "go",
            "Fix Divide — panics on zero. Write test in main_test.go (not divide_test.go).\n           Use EXACTLY: _, err := Divide(10.0, 0.0)\n                        if err == nil { t.Fatal(\"expected error\") }",
            vec![
                ("go.mod", "module gotest\n\ngo 1.21\n"),
                ("main.go", "package main\n\nfunc Divide(a, b float64) (float64, error) {\n    return a / b, nil\n}\n"),
            ],
        ),

        // ═══════════════════════════════════════════════════════
        // Node.js — Creation Tasks
        // ═══════════════════════════════════════════════════════
        BenchCase::bugfix("node add", "node",
            "Fix math.ts — the add function returns 0. Fix it to return a + b. Do NOT modify math.test.ts. Run npm test.",
            vec![
                ("math.ts", "export function add(a: number, b: number): number {\n  return 0;\n}\n"),
                ("math.test.ts", "import { add } from './math';\n\ntest('add positive', () => {\n  expect(add(2, 3)).toBe(5);\n});\n\ntest('add negative', () => {\n  expect(add(-1, 1)).toBe(0);\n});\n\ntest('add zero', () => {\n  expect(add(0, 0)).toBe(0);\n});\n"),
            ],
        ),
        BenchCase::bugfix("node palindrome", "node",
            "Fix palindrome.ts — isPalindrome always returns false. Fix it to correctly detect palindromes. Do NOT modify palindrome.test.ts. Run npm test.",
            vec![
                ("palindrome.ts", "export function isPalindrome(s: string): boolean {\n  return false;\n}\n"),
                ("palindrome.test.ts", "import { isPalindrome } from './palindrome';\n\ntest('racecar is palindrome', () => {\n  expect(isPalindrome('racecar')).toBe(true);\n});\n\ntest('hello is not palindrome', () => {\n  expect(isPalindrome('hello')).toBe(false);\n});\n\ntest('empty string is palindrome', () => {\n  expect(isPalindrome('')).toBe(true);\n});\n"),
            ],
        ),
        BenchCase::bugfix("node factorial", "node",
            "Fix factorial.ts — it returns 1 for all inputs. Fix it to compute factorial correctly (base case: factorial(0)=1). Do NOT modify factorial.test.ts. Run npm test.",
            vec![
                ("factorial.ts", "export function factorial(n: number): number {\n  return 1;\n}\n"),
                ("factorial.test.ts", "import { factorial } from './factorial';\n\ntest('factorial(5) = 120', () => {\n  expect(factorial(5)).toBe(120);\n});\n\ntest('factorial(0) = 1', () => {\n  expect(factorial(0)).toBe(1);\n});\n\ntest('factorial(1) = 1', () => {\n  expect(factorial(1)).toBe(1);\n});\n"),
            ],
        ),
        BenchCase::bugfix("node filter", "node",
            "Fix filter.ts — filterEven returns all numbers instead of only even ones. Fix the filter logic. Do NOT modify filter.test.ts. Run npm test.",
            vec![
                ("filter.ts", "export function filterEven(arr: number[]): number[] {\n  return arr;\n}\n"),
                ("filter.test.ts", "import { filterEven } from './filter';\n\ntest('filters even numbers', () => {\n  expect(filterEven([1, 2, 3, 4, 5, 6])).toEqual([2, 4, 6]);\n});\n\ntest('empty array', () => {\n  expect(filterEven([])).toEqual([]);\n});\n\ntest('no even numbers', () => {\n  expect(filterEven([1, 3, 5])).toEqual([]);\n});\n"),
            ],
        ),

        // ═══════════════════════════════════════════════════════
        // TypeScript — Creation Tasks
        // ═══════════════════════════════════════════════════════
        BenchCase::bugfix("ts add", "ts",
            "Fix math.ts — the add function returns 0. Fix it to return a + b. Do NOT modify math.test.ts. Run npm test.",
            vec![
                ("math.ts", "export function add(a: number, b: number): number {\n  return 0;\n}\n"),
                ("math.test.ts", "import { add } from './math';\n\ntest('add positive', () => {\n  expect(add(2, 3)).toBe(5);\n});\n\ntest('add negative', () => {\n  expect(add(-1, 1)).toBe(0);\n});\n"),
            ],
        ),
        BenchCase::bugfix("ts palindrome", "ts",
            "Fix palindrome.ts — isPalindrome always returns false. Fix it. Do NOT modify palindrome.test.ts. Run npm test.",
            vec![
                ("palindrome.ts", "export function isPalindrome(s: string): boolean {\n  return false;\n}\n"),
                ("palindrome.test.ts", "import { isPalindrome } from './palindrome';\n\ntest('racecar is palindrome', () => {\n  expect(isPalindrome('racecar')).toBe(true);\n});\n\ntest('hello is not', () => {\n  expect(isPalindrome('hello')).toBe(false);\n});\n"),
            ],
        ),
        BenchCase::bugfix("ts factorial", "ts",
            "Fix factorial.ts — it returns 1 for all inputs. Fix it so factorial(5)=120. Do NOT modify factorial.test.ts. Run npm test.",
            vec![
                ("factorial.ts", "export function factorial(n: number): number {\n  return 1;\n}\n"),
                ("factorial.test.ts", "import { factorial } from './factorial';\n\ntest('factorial(5)=120', () => {\n  expect(factorial(5)).toBe(120);\n});\n\ntest('factorial(0)=1', () => {\n  expect(factorial(0)).toBe(1);\n});\n"),
            ],
        ),
        BenchCase::bugfix("ts stack", "ts",
            "Fix stack.ts — push() works but pop() always returns undefined. Fix pop() to return the last item. Do NOT modify stack.test.ts. Run npm test.",
            vec![
                ("stack.ts", "export class Stack<T> {\n  private items: T[] = [];\n  push(item: T): void { this.items.push(item); }\n  pop(): T | undefined { return undefined; }\n  isEmpty(): boolean { return this.items.length === 0; }\n}\n"),
                ("stack.test.ts", "import { Stack } from './stack';\n\ntest('push and pop', () => {\n  const s = new Stack<number>();\n  s.push(1);\n  s.push(2);\n  expect(s.pop()).toBe(2);\n  expect(s.pop()).toBe(1);\n});\n\ntest('isEmpty', () => {\n  const s = new Stack<number>();\n  expect(s.isEmpty()).toBe(true);\n  s.push(1);\n  expect(s.isEmpty()).toBe(false);\n});\n"),
            ],
        ),

        // ═══════════════════════════════════════════════════════
        // Rust — Creation Tasks
        // ═══════════════════════════════════════════════════════
        BenchCase::new("rust add",     "rust", "Create Rust library crate using 'cargo new mylib --lib'. Write pub fn add(a:i32,b:i32)->i32 in mylib/src/lib.rs. Write tests module inside the SAME lib.rs file testing add(2,3)==5 and add(-1,1)==0. Run cargo test."),
        BenchCase::new("rust fizzbuzz","rust", "Create Rust library crate using 'cargo new mylib --lib'. Write pub fn fizzbuzz(n:u32)->String returning Fizz Buzz FizzBuzz or number in mylib/src/lib.rs. Write tests module inside the SAME lib.rs file with 4 cases. Run cargo test."),
        BenchCase::new("rust reverse", "rust", "Create Rust library crate using 'cargo new mylib --lib'. Write pub fn reverse(s:&str)->String in mylib/src/lib.rs. Write tests module inside the SAME lib.rs file testing hello->olleh and empty string. Run cargo test."),
        BenchCase::new("rust stack",   "rust", "Create a generic Stack<T> data structure in Rust. Use 'cargo new ruststack --lib'. Write implementation AND unit tests in ruststack/src/lib.rs using #[cfg(test)] mod tests { use super::*; }. Do NOT create a separate tests/ directory. Run cargo test."),

        // ═══════════════════════════════════════════════════════
        // Web — Creation Tasks
        // ═══════════════════════════════════════════════════════
        BenchCase::new("flask hello",   "python", "Create Python Flask app in app.py with GET /hello route returning JSON {\"message\":\"hello world\"}. Create requirements.txt containing only: flask. Write test_app.py using Flask test client: assert response.status_code==200 and response.get_json()[\"message\"]==\"hello world\". Run pytest."),
        BenchCase::new("fastapi route", "python", "Create Python FastAPI app in main.py with GET /hello route returning {\"message\":\"hello\"}. Create requirements.txt containing: fastapi httpx. Write test_main.py using TestClient from fastapi.testclient: assert response.status_code==200 and response.json()[\"message\"]==\"hello\". Run pytest."),
        BenchCase::new("express api",   "node",   "Create Node.js Express app in app.js exporting the express app with GET /ping route returning JSON {ok:true}. Create package.json with jest supertest express. Write app.test.js using supertest: assert status 200 and body.ok===true. Run npm test."),
        BenchCase::new("ts express",    "ts",     "Create TypeScript Express app. Write app.ts exporting express app with GET /health route returning JSON {status:\"ok\"}. Create package.json with ts-jest jest typescript express @types/express supertest @types/supertest. Create tsconfig.json. Write app.test.ts using supertest: assert status 200 and body.status==\"ok\". Run npm test."),

        // ═══════════════════════════════════════════════════════
        // v7 Feature Tests
        // ═══════════════════════════════════════════════════════
        BenchCase::bugfix("v7_quickfix", "v7",
            "Fix fetcher.py — is_success checks for status 404 instead of 200. Fix it so is_success returns True for 200. Do NOT modify test_fetcher.py. Run pytest.",
            vec![
                ("fetcher.py", "import requests\n\ndef fetch_url(url):\n    return requests.get(url).status_code\n\ndef is_success(status_code):\n    return status_code == 404\n"),
                ("test_fetcher.py", "from fetcher import fetch_url, is_success\n\ndef test_fetch_url_has_url_param():\n    result = fetch_url.__code__.co_varnames\n    assert \"url\" in result\n\ndef test_is_success_200():\n    assert is_success(200) == True\n\ndef test_is_success_404():\n    assert is_success(404) == False\n"),
            ],
        ),
        BenchCase::new("v7_go_autofix",  "v7", "Create Go package main. Write func PrintMessage() that calls fmt.Println(\"Hello\"). STRICT RULE: You must NOT write `import \"fmt\"` anywhere in the file. Leave it missing! Write a test calling the function. Run go test."),
        BenchCase::new("v7_rust_quotes", "v7", "Fix Rust string literals that contain Unicode smart quotes. Use 'cargo new mylib --lib'. Write pub fn greet() -> String that returns String::from(\"Hello\") in mylib/src/lib.rs. Write tests module inside the SAME lib.rs file asserting greet() == \"Hello\". Run cargo test."),
        BenchCase::new("v7_unicode",     "v7", "Create a Python function that uses a variable named \u{2018}msg\u{2019} and returns \u{201C}smart quotes\u{201C}. Write a pytest test checking its value. Run tests (the agent's sanitize_code should fix these Unicode bounds)."),
    ]
}

pub fn suite_cases(suite: &str) -> Vec<BenchCase> {
    match suite {
        "python" => all_cases()
            .into_iter()
            .filter(|c| c.lang == "python")
            .collect(),
        "go" => all_cases().into_iter().filter(|c| c.lang == "go").collect(),
        "node" => all_cases()
            .into_iter()
            .filter(|c| c.lang == "node" || c.lang == "ts")
            .collect(),
        "rust" => all_cases()
            .into_iter()
            .filter(|c| c.lang == "rust")
            .collect(),
        "v7" => all_cases().into_iter().filter(|c| c.lang == "v7").collect(),
        _ => all_cases(),
    }
}
