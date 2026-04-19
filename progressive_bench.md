# SEL Agent v7.3.2 — Progressive Capabilities Benchmark

هذا البانشمارك مصمم للتدرج في الصعوبة لاختبار قدرات الوكيل في سياق عمل مستمر (نفس الـ workspace).

- [ ] المستوى 1 (بناء الأساسيات): Create a Python module `calc.py` with a function `add(a, b)`. Write a pytest suite `test_calc.py` asserting `add(2, 3) == 5`. Run tests.
- [ ] المستوى 2 (استخدام Patch): The file `calc.py` already exists. Use `patch_file` to add a new function `multiply(a, b)` without removing `add(a, b)`. Add a test for `multiply` in `test_calc.py`. Run tests.
- [ ] المستوى 3 (تثبيت الاعتمادات والشبكات): Create a Python script `fetcher.py` that uses the `requests` library to fetch `https://httpbin.org/get`. Write a test that mocks `requests.get` to return `{"status": "ok"}`. Note: do not use pip install in the goal, let the agent figure out it needs `pytest-mock` or `requests` if they are missing.
- [ ] المستوى 4 (التعامل مع بيئة مختلفة - Node.js): In the same workspace, initialize a Node.js project. Create `utils.ts` exporting an exact string reversal function. Set up Jest for TypeScript. Write `utils.test.ts`. Run `npm test`.
- [ ] المستوى 5 (التصليح الذاتي المعقد): The file `utils.ts` has a working reverse function. Use `patch_file` to change its logic so it always returns "BUG". Let the test fail. The agent MUST NOT use `write_file` to rewrite the whole file, it must repair the bug using exact code fixes so the test passes again.
