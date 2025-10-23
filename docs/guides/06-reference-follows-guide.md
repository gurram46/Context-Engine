# Reference Linked to Guides

Every guide should link directly to the reference information that advanced users expect. Keep parameter tables, flag listings, and environment variables adjacent to the walkthrough.

Example reference pulled from context-engine --help:

| Command | Description | Source |
|---------|-------------|--------|
| context-engine start-session --auto | Start background tracker | ackend/context_engine/cli.py |
| context-engine session save | Generate session summary | ackend/context_engine/cli.py |
| context-engine bundle | Package bundle for handoff | ackend/context_engine/commands/bundle_command.py |

When writing a guide (e.g., Quickstart), place a “Reference” subsection at the end pointing back to these tables. That way, readers who already know the basics can jump straight into exhaustive options.
