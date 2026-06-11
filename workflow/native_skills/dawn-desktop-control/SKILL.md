# Dawn Desktop Control

This is a Dawn native builtin skill.

Purpose:
- translate guarded chat intent into desktop observation and mouse commands
- keep phone-originated desktop control on the node-command approval path
- expose mouse position, coordinate move, coordinate click, and screen snapshot workflows

Primary local surfaces:
- Chat: `#assist` to preview, `#autopilot` to create approval-backed actions
- Examples: `看一下屏幕`, `鼠标位置`, `移动鼠标到 400,300`, `点击 400,300`
- `/console` Approval Center and Node Command Console
- CLI: `dawn-node node-command dispatch --type desktop_mouse_click --payload '{"x":400,"y":300,"button":"left"}'`

Safety model:
- desktop commands require an attested online node with the matching capability
- `desktop_*` commands are routed through node-command approval before execution
- free-form button names such as `点击确定` are not clicked blindly; use coordinates or a future UI-recognition step

Notes:
- this is a native Dawn workflow skill, not a Wasm artifact
- it is always available on the local system once Dawn is installed
