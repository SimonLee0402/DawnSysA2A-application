# Agent Card Discoverer

This is a Dawn native builtin skill.

Purpose:
- discover A2A-compatible Agent Cards
- validate and inspect locally imported or federated cards
- help the operator move from card discovery to delegate/invoke workflows

Primary local surfaces:
- `dawn.cmd agents search <query> --federated`
- `/app` Agent Cards panel
- operator workflows that combine search, install, quote, and delegate

Notes:
- this is a native Dawn workflow skill, not a Wasm artifact
- it is always available on the local system once Dawn is installed
