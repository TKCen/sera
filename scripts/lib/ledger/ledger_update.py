#!/usr/bin/env python3
"""ledger_update.py <ledger_file> <slug> <key=value>..."""
import json, sys

ledger_file = sys.argv[1]
slug = sys.argv[2]
updates = sys.argv[3:]

with open(ledger_file) as f:
    data = json.load(f)

for e in data:
    if e.get('slug') == slug:
        for pair in updates:
            if '=' not in pair:
                continue
            k, _, v = pair.partition('=')
            e[k] = True if v == 'true' else False if v == 'false' else None if v == 'null' else int(v) if v.isdigit() else v
        with open(ledger_file, 'w') as f:
            json.dump(data, f, indent=2)
        print('updated')
        sys.exit(0)

print('NOT_FOUND')
sys.exit(1)
