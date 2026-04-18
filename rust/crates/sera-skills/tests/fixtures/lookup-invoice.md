---
name: lookup-invoice
description: Find an invoice by its external id via the finance API
inputs:
  invoice_id: string
tier: 1
---

# Behaviour

When asked about an invoice by id, call the finance API's `/invoices/{id}`
endpoint and summarise the result. Do not expose raw payloads — emit the
invoice number, date, total, and status only.
