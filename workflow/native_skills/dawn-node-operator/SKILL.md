# Dawn Node Operator

This is a Dawn native builtin skill.

Purpose:
- operate the local Dawn node lifecycle
- inspect node health, rollout status, and workspace readiness
- keep desktop execution attached to the gateway control plane

Primary local surfaces:
- `dawn.ps1 status`
- `dawn-node setup`
- `dawn.cmd doctor --deep`
- `/console`

Notes:
- this is a native Dawn workflow skill, not a Wasm artifact
- it is always available on the local system once Dawn is installed
