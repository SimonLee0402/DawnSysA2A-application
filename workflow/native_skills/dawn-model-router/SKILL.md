# Dawn Model Router

This is a Dawn native builtin skill.

Purpose:
- select the right connector and model path for a task
- unify cloud and local model invocation
- keep local Ollama-hosted models in the same routing plane as hosted providers

Primary local surfaces:
- `/model`
- `dawn.ps1 connectors status`
- `dawn.ps1 models test <provider>`
- workflow `model_connector` steps

Notes:
- this is a native Dawn workflow skill, not a Wasm artifact
- it is always available on the local system once Dawn is installed
