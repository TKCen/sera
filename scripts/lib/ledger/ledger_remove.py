#!/usr/bin/env python3
"""ledger_remove.py <ledger_file> <slug>"""
import json, sys

ledger_file = sys.argv[1]
slug = sys.argv[2]

with open(ledger_file) as f:
    data = json.load(f)

data = [e for e in data if e.get('slug') != slug]
with open(ledger_file, 'w') as f:
    json.dump(data, f, indent=2)
print('removed')
