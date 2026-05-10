// src/bench_cases.rs — v7.6: مصدر واحد لكل حالات البانش
use crate::types::BenchCase;

pub fn all_cases() -> Vec<BenchCase> {
    vec![
        // Python
        BenchCase::new("broken import",    "python", "Create Python file importing from math_utils import add. Create math_utils.py with add(a,b) function. Write pytest test. Run tests."),
        BenchCase::new("wrong assertion",  "python", "Create Python function double(x) returning x*2. Write pytest test asserting double(3)==6. Run tests."),
        BenchCase::new("wrong signature",  "python", "Create Python function greet(name) returning f'Hi {name}'. Write pytest test expecting greet('Alice')=='Hi Alice'. Run tests."),
        BenchCase::new("wrong logic",      "python", "Create Python function is_even(n) returning n%2==0. Write pytest test for is_even(4)==True and is_even(3)==False. Run tests."),
        BenchCase::new("type mismatch",    "python", "Create Python function add(a,b) returning a+b for integers. Write pytest test expecting add(2,3)==5. Run tests."),
        BenchCase::new("missing closing",  "python", "Create Python function factorial(n) with base case n==0 returns 1. Write pytest test for factorial(5)==120. Run tests."),
        BenchCase::new("undefined func",   "python", "Create Python module with helper() returning 42. Write pytest test asserting helper()==42. Run tests."),
        BenchCase::new("wrong logic 2",    "python", "Create Python function max_of_three(a,b,c) returning max(a,b,c). Write pytest test. Run tests."),
        BenchCase::new("syntax error",     "python", "Create Python function add(a,b) returning a+b with correct syntax. Write pytest test. Run tests."),
        BenchCase::new("wrong return",     "python", "Create Python function reverse_string(s) returning s[::-1]. Write pytest test expecting reverse_string('hello')=='olleh'. Run tests."),
        BenchCase::new("missing function", "python", "Create Python class Stack with push(item) and pop() methods. Write pytest test. Run tests."),
        BenchCase::new("runtime error",    "python", "Create Python function divide(a,b) returning None if b==0 else a/b. Write pytest tests: test divide(10,2)==5.0 AND divide(10,0)==None (both branches required). Run tests."),
        // Go
        BenchCase::new("go add",      "go", "Create Go package main with Add(a,b int) int. Create go.mod with module gotest and go 1.21. Write _test.go testing Add(2,3)==5 and Add(-1,1)==0. Run go test."),
        BenchCase::new("go fizzbuzz", "go", "Create Go package main with FizzBuzz(n int) string returning Fizz/Buzz/FizzBuzz/number. Create go.mod module gotest go 1.21. Write _test.go with 4 test cases using only t.Errorf (no fmt import). Run go test."),
        BenchCase::new("go reverse",  "go", "Create Go package main with Reverse(s string) string. Create go.mod module gotest go 1.21. Write _test.go using only t.Errorf (no fmt): test Reverse(\"hello\")=\"olleh\" and Reverse(\"\")=\"\". Run go test."),
        BenchCase::new("go divide",   "go", "Create Go package main with Divide(a,b float64) (float64,error) returning error if b==0. Create go.mod module gotest go 1.21. Write _test.go testing normal and zero cases. Use t.Fatal(err) immediately after calling Divide with zero, do NOT use if/else error checking patterns. Run go test."),
        // Node
        BenchCase::new("node add",        "node", "Create Node.js CommonJS module math.js exporting add(a,b). Create package.json with jest. Write math.test.js testing add(2,3)===5 and add(-1,1)===0. Run npm test."),
        BenchCase::new("node palindrome", "node", "Create Node.js CommonJS module palindrome.js exporting isPalindrome(s). Create package.json with jest. Write test file testing racecar==true and hello==false. Run npm test."),
        BenchCase::new("node factorial",  "node", "Create Node.js CommonJS module factorial.js exporting factorial(n) with base case 0==1. Create package.json with jest. Write test for factorial(5)==120 and factorial(0)==1. Run npm test."),
        BenchCase::new("node filter",     "node", "Create Node.js CommonJS module filter.js exporting filterEven(arr) returning even numbers. Create package.json with jest. Write test with arrays including empty array case. Run npm test."),
        // TypeScript
        BenchCase::new("ts add",        "ts", "Create TypeScript file math.ts exporting function add(a:number,b:number):number. Create package.json with jest and ts-jest. Create tsconfig.json. Write math.test.ts testing add(2,3)===5 and add(-1,1)===0. Run npm test."),
        BenchCase::new("ts palindrome", "ts", "Create TypeScript file palindrome.ts exporting function isPalindrome(s:string):boolean. Create package.json with jest and ts-jest. Create tsconfig.json. Write palindrome.test.ts testing racecar===true and hello===false. Run npm test."),
        BenchCase::new("ts factorial",  "ts", "Create TypeScript file factorial.ts exporting function factorial(n:number):number with base case 0 returns 1. Create package.json with jest and ts-jest. Create tsconfig.json. Write factorial.test.ts testing factorial(5)===120 and factorial(0)===1. Run npm test."),
        BenchCase::new("ts stack",      "ts", "Create TypeScript file stack.ts exporting class Stack<T> with push(item:T) pop():T|undefined and isEmpty():boolean. Create package.json with jest and ts-jest. Create tsconfig.json. Write stack.test.ts with push/pop/isEmpty tests. Run npm test."),
        // Rust
        BenchCase::new("rust add",     "rust", "Create Rust library crate using 'cargo new mylib --lib'. Write pub fn add(a:i32,b:i32)->i32 in mylib/src/lib.rs. Write tests module inside the SAME lib.rs file testing add(2,3)==5 and add(-1,1)==0. Run cargo test."),
        BenchCase::new("rust fizzbuzz","rust", "Create Rust library crate using 'cargo new mylib --lib'. Write pub fn fizzbuzz(n:u32)->String returning Fizz Buzz FizzBuzz or number in mylib/src/lib.rs. Write tests module inside the SAME lib.rs file with 4 cases. Run cargo test."),
        BenchCase::new("rust reverse", "rust", "Create Rust library crate using 'cargo new mylib --lib'. Write pub fn reverse(s:&str)->String in mylib/src/lib.rs. Write tests module inside the SAME lib.rs file testing hello->olleh and empty string. Run cargo test."),
        BenchCase::new("rust stack",   "rust", "Create a generic Stack<T> data structure in Rust. Use 'cargo new ruststack --lib'. Write implementation AND unit tests in ruststack/src/lib.rs using #[cfg(test)] mod tests { use super::*; }. Do NOT create a separate tests/ directory. Run cargo test."),
        // Web
        BenchCase::new("flask hello",   "python", "Create Python Flask app in app.py with GET /hello route returning JSON {\"message\":\"hello world\"}. Create requirements.txt containing only: flask. Write test_app.py using Flask test client: assert response.status_code==200 and response.get_json()[\"message\"]==\"hello world\". Run pytest."),
        BenchCase::new("fastapi route", "python", "Create Python FastAPI app in main.py with GET /hello route returning {\"message\":\"hello\"}. Create requirements.txt containing: fastapi httpx. Write test_main.py using TestClient from fastapi.testclient: assert response.status_code==200 and response.json()[\"message\"]==\"hello\". Run pytest."),
        BenchCase::new("express api",   "node",   "Create Node.js Express app in app.js exporting the express app with GET /ping route returning JSON {ok:true}. Create package.json with jest supertest express. Write app.test.js using supertest: assert status 200 and body.ok===true. Run npm test."),
        BenchCase::new("ts express",    "ts",     "Create TypeScript Express app. Write app.ts exporting express app with GET /health route returning JSON {status:\"ok\"}. Create package.json with ts-jest jest typescript express @types/express supertest @types/supertest. Create tsconfig.json. Write app.test.ts using supertest: assert status 200 and body.status==\"ok\". Run npm test."),
        // v7 Features
        BenchCase::new("v7_quickfix",    "v7", "Fix this broken Python file that imports 'requests' but it's not installed. In fetcher.py: 'import requests\ndef fetch_url(url):\n    return requests.get(url).status_code'. In test_fetcher.py: 'from fetcher import fetch_url\ndef test_fetch_url_returns_int():\n    result = fetch_url.__code__.co_varnames\n    assert \"url\" in result'. Goal: make the tests pass (pip install requests will be triggered automatically)."),
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
