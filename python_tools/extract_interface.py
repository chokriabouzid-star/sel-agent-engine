#!/usr/bin/env python3
"""
يستخرج الرموز من ملف Python ويعيد JSON
{
  "symbols": ["User", "Product", "get_db", "engine"],
  "imports": {
    "User": "from models import User",
    "get_db": "from database import get_db"
  }
}
"""
import ast, sys, json, pathlib

def extract(path: str) -> dict:
    src = pathlib.Path(path).read_text(encoding="utf-8", errors="ignore")
    stem = pathlib.Path(path).stem

    try:
        tree = ast.parse(src)
    except SyntaxError:
        return {"symbols": [], "imports": {}}

    symbols = []

    for node in tree.body:
        # دوال عادية وasync
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            if not node.name.startswith("_"):
                symbols.append(node.name)

        # أصناف
        elif isinstance(node, ast.ClassDef):
            symbols.append(node.name)

        # متغيرات المستوى الأعلى
        elif isinstance(node, ast.Assign):
            for target in node.targets:
                if isinstance(target, ast.Name) and not target.id.startswith("_"):
                    symbols.append(target.id)

        # متغيرات بنوع (x: int = ...)
        elif isinstance(node, ast.AnnAssign):
            if isinstance(node.target, ast.Name):
                if not node.target.id.startswith("_"):
                    symbols.append(node.target.id)

    # أزل المكررات مع الحفاظ على الترتيب
    seen = set()
    unique = []
    for s in symbols:
        if s not in seen:
            seen.add(s)
            unique.append(s)

    # ابنِ import lines
    imports = {s: f"from {stem} import {s}" for s in unique}

    return {"symbols": unique, "imports": imports}

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print(json.dumps({"symbols": [], "imports": {}}))
        sys.exit(0)

    result = extract(sys.argv[1])
    print(json.dumps(result))
