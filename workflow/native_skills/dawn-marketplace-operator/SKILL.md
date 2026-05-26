# Dawn Marketplace Operator

This is a Dawn native builtin skill.

Purpose:
- publish, search, and install Dawn skills and Agent Cards
- work with federated marketplace catalogs
- move capabilities between local and remote Dawn gateways

Primary local surfaces:
- `/app` Marketplace and Agent Cards panels
- `dawn.cmd agents search <query> --federated`
- `dawn-node skills install`

Notes:
- this is a native Dawn workflow skill, not a Wasm artifact
- it is always available on the local system once Dawn is installed
