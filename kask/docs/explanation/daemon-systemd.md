---
title: "Daemon Management with systemd"
audience: [developers, administrators]
last_updated: 2026-07-18
version: "0.31.0"
status: "Active"
domain: "Core"
mds_categories: [lifecycle]
---

# Daemon Management with systemd

**Purpose:** Run the hKask daemon as a managed service that survives user
logout and automatically restarts on failure.

## Why systemd?

The hKask daemon is a persistent background process that serves P4 OCAP gate
verification to MCP server binaries. Without it, MCP servers fall back to
direct mode, bypassing OCAP verification. Running the daemon under systemd
provides:

- **Survives logout:** The daemon stays running when the user logs out.
- **Auto-restart:** If the daemon crashes, systemd restarts it within 5 seconds.
- **Logging:** systemd-journal captures all daemon output.
- **Dependency ordering:** The daemon starts after D-Bus and gnome-keyring.

## User-level setup (recommended for single-user machines)

```bash
# Copy the unit file
mkdir -p ~/.config/systemd/user
cp deploy/systemd/hkask-daemon-user.service ~/.config/systemd/user/hkask-daemon.service

# Edit to set your DB passphrase (or use an EnvironmentFile)
nano ~/.config/systemd/user/hkask-daemon.service

# Reload systemd
systemctl --user daemon-reload

# Enable and start
systemctl --user enable hkask-daemon
systemctl --user start hkask-daemon

# Check status
systemctl --user status hkask-daemon

# View logs
journalctl --user -u hkask-daemon -f
```

To ensure the daemon starts at boot (even before login):

```bash
loginctl enable-linger $USER
```

## System-level setup (for multi-user or server deployments)

```bash
# Copy the template unit
sudo cp deploy/systemd/hkask-daemon@.service /etc/systemd/system/

# Enable for a specific user (e.g., "alice")
sudo systemctl enable hkask-daemon@alice
sudo systemctl start hkask-daemon@alice

# Check status
sudo systemctl status hkask-daemon@alice
```

## Environment configuration

The daemon needs `HKASK_DB_PASSPHRASE` to decrypt the SQLCipher database.
Three options:

1. **Keychain (preferred):** Run `kask init` once to store the passphrase in
   the OS keychain. The daemon resolves it automatically.

2. **Environment variable in the unit file:**
   ```ini
   Environment=HKASK_DB_PASSPHRASE=your-passphrase
   ```

3. **EnvironmentFile:**
   ```bash
   echo "HKASK_DB_PASSPHRASE=your-passphrase" > ~/.config/hkask/env
   chmod 600 ~/.config/hkask/env
   ```
   Then uncomment the `EnvironmentFile` line in the unit.

## Verification

```bash
# Check the daemon is running
kask daemon status

# Run the bootstrap check
kask doctor --bootstrap
```

## Cross-references

- [ADR-035: UserPod Server Mode](../architecture/ADRs/ADR-035-userpod-server-mode.md)
- [Getting Started](getting-started.md)
- REPL Bootstrap Gap Post-Mortem
