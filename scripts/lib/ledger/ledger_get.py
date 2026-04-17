#!/usr/bin/env python3
"""ledger_get.py <ledger_file> <slug> [field]"""
import json, sys

ledger_file = sys.argv[1]
slug = sys.argv[2]
field = sys.argv[3] if len(sys.argv) > 3 else None

with open(ledger_file) as f:
    data = json.load(f)

for e in data:
    if e.get('slug') == slug:
        if field:
            print(e.get(field, ''))
        else:
            print(json.dumps(e, indent=2))
        sys.exit(0)

print('NOT_FOUND')
sys.exit(1)
