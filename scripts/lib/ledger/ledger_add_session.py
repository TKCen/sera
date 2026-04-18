#!/usr/bin/env python3
"""ledger_add_session.py <ledger_file> <slug> <session_name>"""
import json, sys

ledger_file = sys.argv[1]
slug = sys.argv[2]
session = sys.argv[3]

with open(ledger_file) as f:
    data = json.load(f)

for e in data:
    if e.get('slug') == slug:
        s = e.setdefault('session_names', [])
        if session not in s:
            s.append(session)
        with open(ledger_file, 'w') as f:
            json.dump(data, f, indent=2)
        print('added')
        sys.exit(0)

print('NOT_FOUND')
sys.exit(1)
