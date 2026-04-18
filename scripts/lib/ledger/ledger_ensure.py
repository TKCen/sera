#!/usr/bin/env python3
"""ledger_ensure.py — init the ledger file"""
import json, sys, os

ledger_file = sys.argv[1] if len(sys.argv) > 1 else os.getenv(
    'LEDGER_FILE', os.path.expanduser('~/.sera/wrapper_ledger.json'))
os.makedirs(os.path.dirname(ledger_file), exist_ok=True)
if not os.path.exists(ledger_file):
    with open(ledger_file, 'w') as f:
        json.dump([], f)
print(ledger_file)
