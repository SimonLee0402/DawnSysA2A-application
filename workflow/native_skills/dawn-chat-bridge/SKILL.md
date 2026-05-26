# Dawn Chat Bridge

This is a Dawn native builtin skill.

Purpose:
- normalize chat commands across supported messaging platforms
- bridge inbound user messages into Dawn tasks and workflows
- return execution results back to the originating channel

Primary local surfaces:
- Telegram, Slack, Discord, Signal, Feishu, DingTalk, WeCom, QQ Bot
- `/help`
- `/skills`
- `/status`

Notes:
- this is a native Dawn workflow skill, not a Wasm artifact
- it is always available on the local system once Dawn is installed
