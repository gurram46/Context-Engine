# Use Plausible Data

Examples should look real so that compression, summarisation, and bundling behave the way they would in production. When building baseline context or session notes, use tools like aker to generate believable content.

## Baseline example

`python
from faker import Faker
from pathlib import Path

fake = Faker()
baseline_dir = Path('.context/baseline')
baseline_dir.mkdir(parents=True, exist_ok=True)

customer = baseline_dir / 'customer_profile.md'
customer.write_text(f"""# Customer Profile

- Name: {fake.name()}
- Account ID: {fake.uuid4()}
- Plan: {fake.random_element(['Starter', 'Growth', 'Enterprise'])}
- Renewal Date: {fake.future_date()}
""", encoding='utf-8')
`

When you run context compress, the generated summaries reference realistic entities instead of placeholders like oo or 123.

## Session notes

Capture notes that sound like the work you did:

`ash
context-engine session save "Integrated billing API for ACME Corp account upgrade"
`

These notes flow into .context/session_summary.md, making it easier for teammates (or AI agents) to infer context.
