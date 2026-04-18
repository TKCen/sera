#!/usr/bin/env python3
"""ledger_list.py <ledger_file> [status_filter]"""
import json, sys

ledger_file = sys.argv[1]
status_filter = sys.argv[2] if len(sys.argv) > 2 else None

with open(ledger_file) as f:
    data = json.load(f)

for e in data:
    if status_filter is None or e.get('status') == status_filter:
        print(f"{e['slug']}\t{e['status']}\t{e.get('last_lane','')}\t{e.get('cwd','')}")
