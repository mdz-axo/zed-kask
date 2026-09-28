# hkask-keystore

OS keychain access for hKask credentials and the shared SQLCipher database passphrase.

## Features

- **OS keychain** — stores secrets in the OS-native keystore (Linux: D-Bus Secret Service), using Zed's `url` and `username` item attributes.
- **Async URL access** — editor callers await async-std-backed `oo7` operations without blocking GPUI workers; synchronous key operations remain available for pre-app rotation and standalone callers.
- **First-run DB key** — generates a new random shared key into the OS keychain only for a fresh data tree; an existing managed database without its key stays unavailable until explicitly discarded. Loss of the keychain makes its encrypted stores unrecoverable. A scheduled change is applied at startup before the main keychain slot is updated.

## Configuration

| Variable | Description |
|----------|-------------|
| `HKASK_DB_PASSPHRASE` | Database encryption passphrase override |

### Keychain keys

| Key | Description |
|-----|-------------|
| `kask://credentials/hkask_db_passphrase` | Shared database passphrase; updated only after a successful rotation |
| `kask://credentials/hkask_db_passphrase_pending` | Scheduled replacement passphrase; deleted after rotation |

Data-service keys live under `kask://credentials/<key>`; inference-provider keys live at the provider's API URL. The keychain stores credentials, not an additional master key or encryption layer.
