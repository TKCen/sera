#!/usr/bin/env python3
"""ledger_create.py"""
import json, sys, os
from datetime import datetime, timezone

ledger_file = sys.argv[1]
slug = sys.argv[2]
branch = sys.argv[3]
cwd = sys.argv[4]
task_id = sys.argv[5]
artifact_root = sys.argv[6]
issue = sys.argv[7] if len(sys.argv) > 7 else ""

created_at = datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')

with open(ledger_file) as f:
    data = json.load(f)

if any(e.get('slug') == slug for e in data):
    print(f"DUPLICATE:{slug}", file=sys.stderr)
    sys.exit(1)

entry = {
    "slug": slug, "branch": branch, "cwd": cwd,
    "status": "active", "created_at": created_at,
    "last_lane": "", "session_names": [], "pr": None,
    "issue": issue if issue else None,
    "taskId": task_id, "artifact_root": artifact_root
}
data.append(entry)
with open(ledger_file, 'w') as f:
    json.dump(data, f, indent=2)
print(slug)
