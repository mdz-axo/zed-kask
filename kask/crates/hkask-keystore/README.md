# hkask-keystore

OS keychain integration and AES-256-GCM encryption for hKask.

## Features

- **OS keychain** — stores secrets in the OS-native keystore (Linux: DBus Secret Service)
- **AES-256-GCM** — authenticated encryption for passphrase-protected secrets
- **Interactive passphrase** — prompts user for master passphrase on first run

## Configuration

| Variable | Description |
|----------|-------------|
| `HKASK_DB_PASSPHRASE` | Database encryption passphrase (env var) |
| `HKASK_MASTER_KEY` | 32-byte master key as 64-char hex (env var) |

### Keychain keys

| Key | Description |
|-----|-------------|
| `hkask-db-passphrase` | Database encryption passphrase (OS keychain) |
| `HKASK_MASTER_KEY` | Master key hex (OS keychain) |
| `a2a-secret` | A2A root-authority secret |
| `ocap-secret` | OCAP signing secret (fails closed if master key is unavailable) |
