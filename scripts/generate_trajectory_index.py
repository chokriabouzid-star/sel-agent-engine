#!/usr/bin/env python3
"""
Scans fixtures/trajectories/ and builds fixtures/trajectories/index.json
Will look for JSON files and try to extract common metadata keys if present.
"""
import json, os, glob, datetime
ROOT = "fixtures/trajectories"
out = []
if not os.path.isdir(ROOT):
    os.makedirs(ROOT, exist_ok=True)
for root, dirs, files in os.walk(ROOT):
    for f in files:
        if f.endswith(".json"):
            path = os.path.join(root, f)
            try:
                with open(path, "r", encoding="utf-8") as fh:
                    j = json.load(fh)
                meta = j.get("meta", {}) if isinstance(j, dict) else {}
                task_id = meta.get("task_id") or os.path.relpath(path, ROOT)
                recorded_at = meta.get("recorded_at") or datetime.datetime.fromtimestamp(os.path.getmtime(path)).isoformat()
                constitution_hash = meta.get("constitution_hash")
                language = meta.get("language") or os.path.basename(root)
                interaction_count = meta.get("interaction_count") or (len(j.get("interactions")) if isinstance(j, dict) and "interactions" in j else None)
                has_repairs = meta.get("has_repairs") if meta.get("has_repairs") is not None else bool(j.get("repairs") if isinstance(j, dict) else False)
                out.append({
                    "path": os.path.relpath(path, ROOT),
                    "task_id": task_id,
                    "recorded_at": recorded_at,
                    "constitution_hash": constitution_hash,
                    "language": language,
                    "interaction_count": interaction_count,
                    "has_repairs": has_repairs
                })
            except Exception as e:
                # skip unreadable
                out.append({
                    "path": os.path.relpath(path, ROOT),
                    "error": str(e)
                })
index_path = os.path.join(ROOT, "index.json")
with open(index_path, "w", encoding="utf-8") as fh:
    json.dump(sorted(out, key=lambda x: x.get("path")), fh, indent=2, ensure_ascii=False)
print("Wrote", index_path)
