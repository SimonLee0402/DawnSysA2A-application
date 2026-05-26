# Dawn Orchestrator

This is a Dawn native builtin skill.

Purpose:
- turn user intent into executable Dawn tasks
- coordinate delegation, workflow fan-out, and result stitching
- keep local task execution aligned with gateway state and operator context

Primary local surfaces:
- `/task`
- `/delegate`
- `/status`
- `/app` Command Studio

Notes:
- this is a native Dawn workflow skill, not a Wasm artifact
- it is always available on the local system once Dawn is installed
