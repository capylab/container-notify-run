# Distrobox Run Notify - systemd Integration for Containers

This system enables distrobox and podman containers to properly notify systemd
about their readiness status. It creates a real Unix domain socket inside the
container that forwards systemd notify calls to the host via shared files.
Both components are written in Rust for memory safety and reliability.

## Requirements

### Container Requirements:
- The `container-notify-wrapper` Rust binary
- Container must have write access to shared volume

### Host Requirements:
- The `host-notify-waiter` Rust binary
- systemd running the service (systemd-notify command NOT required - uses direct sd_notify)

## Building

1. Build both Rust binaries:
```bash
cargo build --release
```

This creates two executables:
- `target/release/container-notify-wrapper` - For use inside containers
- `target/release/host-notify-waiter` - For use on the host system

2. Copy binaries to appropriate locations:
```dockerfile
# In container image
COPY target/release/container-notify-wrapper /usr/bin/container-notify-wrapper
```

```bash
# On host system
sudo cp target/release/host-notify-waiter /usr/bin/
```

## Installation

### From RPM (openSUSE/SLES):
```bash
# Install from OBS repository (when available)
zypper install distrobox-run-notify
```

### Manual Installation:
```bash
# Install binaries
sudo cp target/release/container-notify-wrapper /usr/bin/
sudo cp target/release/host-notify-waiter /usr/bin/
```

## Usage Examples

### systemd Service:
```ini
[Unit]
Description=Niri Wayland Compositor
After=network.target

[Service]
Type=notify
Environment=SHARED_DIR=/tmp/niri-notify
ExecStart=/usr/bin/host-notify-waiter podman run --rm \
  --name=niri-container \
  -v /tmp/niri-notify:/shared:Z \
  -e SHARED_DIR=/shared \
  niri:latest /opt/container-notify-wrapper /usr/bin/niri
TimeoutStartSec=120
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
```

### Manual Testing:
```bash
# Create shared directory
mkdir -p /tmp/test-notify

# Start host waiter (in one terminal)
SHARED_DIR=/tmp/test-notify ./host-notify-waiter \
  podman run --rm \
    -v /tmp/test-notify:/shared:Z \
    -e SHARED_DIR=/shared \
    your-image:latest /usr/bin/container-notify-wrapper your-app

# For verbose logging on both host and container
SHARED_DIR=/tmp/test-notify ./host-notify-waiter --verbose \
  podman run --rm \
    -v /tmp/test-notify:/shared:Z \
    -e SHARED_DIR=/shared \
    your-image:latest /usr/bin/container-notify-wrapper --verbose your-app

# Monitor systemd status (in another terminal)
systemctl status your-service
```

### Docker Example:
```bash
SHARED_DIR=/tmp/app-notify ./host-notify-waiter \
  docker run --rm \
    -v /tmp/app-notify:/shared \
    -e SHARED_DIR=/shared \
    myapp:latest /usr/bin/container-notify-wrapper /usr/bin/myapp
```

## How It Works

1. **Host waiter** (Rust binary) starts container with shared volume mounted
2. **Container wrapper** (Rust binary) creates real Unix domain socket
3. **Socket proxy process** listens for systemd notify messages
4. **Message processor** parses notify messages and writes to status file
5. **Host waiter** monitors status file and forwards to real systemd
6. **systemd** receives proper notify signals

## Debugging

### Check if socket is created:
```bash
# Inside container
ls -la /shared/notify.sock
file /shared/notify.sock  # Should show "socket"
```

### Test socket manually:
```bash
# Inside container - Test with systemd-notify if available
systemd-notify --ready

# Or test directly with socat if installed
echo "READY=1" | socat - UNIX-SENDTO:/shared/notify.sock
```

### Monitor shared files:
```bash
# On host
watch -n 0.5 'ls -la /tmp/container-notify/; echo "=== STATUS ==="; cat /tmp/container-notify/container-status 2>/dev/null || echo "No status yet"'
```

## Environment Variables

### Host waiter:
- `SHARED_DIR`: Directory for communication files (default: /tmp/container-notify)
- `TIMEOUT`: Timeout in seconds (default: 60)

### Container wrapper:
- `SHARED_DIR`: Same directory, mounted into container (default: /shared)

## Logging

Both binaries support two log levels:
- **Normal**: Only errors are shown
- **Verbose**: Use `--verbose` flag to see detailed info messages for debugging

Example log output:

**Container wrapper:**
```
[MAIN 15:30:45] Starting container wrapper for: /usr/bin/niri
[MAIN 15:30:45] Socket proxy started with PID 1234
[PROXY 15:30:45] Socket proxy listening...
[PROXY 15:30:46] Process signaled ready!
```

**Host waiter:**
```
[MAIN 15:30:44] Starting host waiter for container command: podman run...
[MAIN 15:30:45] Container started with PID 5678
[MAIN 15:30:46] Sending READY=1 notification to systemd
```

## Troubleshooting

1. **"Binary not found"**: Ensure both Rust binaries are installed in correct locations
2. **"NOTIFY_SOCKET not set"**: This is normal - the host waiter uses direct sd_notify protocol
3. **"Permission denied"**: Check volume mount permissions and SELinux contexts
4. **"Socket not created"**: Check if shared directory is writable
5. **"Fork failed"**: Container may not support fork() - try different container runtime
6. **Verbose logging**: Use `--verbose` flag on both binaries for detailed operation logs
7. **PID issues**: This version solves PID mismatch with direct sd_notify - no `NotifyAccess=all` needed