# Linux Installer Build Guide

## Prerequisites

- Rust toolchain installed
- `cargo-deb` (auto-installed by build script)
- `cargo-generate-rpm` (auto-installed by build script)

## Build Packages

### Automated Build (Recommended)

```bash
bash installers/linux/build_packages.sh
```

This will create both:
- `buttre_<version>_amd64.deb` (Debian/Ubuntu)
- `buttre-<version>.x86_64.rpm` (Fedora/RHEL/CentOS)

### Manual Build

#### Build .deb (Debian/Ubuntu)

```bash
# Install cargo-deb
cargo install cargo-deb

# Build package (from repo root)
cargo build --release -p buttre-platform
cargo deb -p buttre-platform --no-build
```

Output: `target/debian/buttre_<version>_amd64.deb`

#### Build .rpm (Fedora/RHEL)

```bash
# Install cargo-generate-rpm
cargo install cargo-generate-rpm

# Build package (from repo root)
cargo build --release -p buttre-platform
cargo generate-rpm -p crates/buttre-platform
```

Output: `target/generate-rpm/buttre-<version>.x86_64.rpm`

## Installation

### Debian/Ubuntu

```bash
sudo apt install ./buttre_<version>_amd64.deb
```

Note: The "./" is required for `apt` to distinguish file path and package name.

### Fedora/RHEL/CentOS

```bash
sudo rpm -i buttre-<version>.x86_64.rpm
# Or with dnf:
sudo dnf install buttre-<version>.x86_64.rpm
```

### Arch Linux (Manual)

For Arch, you can create a PKGBUILD or install manually:

```bash
# Build binary
cargo build --release --package buttre-platform

# Install manually
sudo install -m 755 target/release/buttre /usr/bin/
sudo install -m 644 installers/linux/buttre.xml /usr/share/ibus/component/buttre.xml
sudo mkdir -p /usr/share/buttre/keyboards
sudo install -m 644 target/release/buttre_nom.db /usr/share/buttre/
sudo install -m 644 keyboards/*.toml /usr/share/buttre/keyboards/

# Restart IBus
killall ibus-daemon
ibus-daemon -drx &
```

TODO: Manual installation should use `/usr/local/bin/` and `/usr/local/share/` instead of `/usr/bin/` and `/usr/share/`, as the latter paths are reserved for system package managers.

## Post-Installation

### 1. Restart IBus

```bash
killall ibus-daemon
ibus-daemon -drx &
```

### 2. Add Input Method

**GNOME/Ubuntu:**
1. Open "Language Supports" app, then set "Keyboard input method system" to "ibus".
1. Open "Settings" app → Keyboard
2. Click "+" under Input Sources
3. Select "Vietnamese → buttre"

**KDE:**
1. Open System Settings → Input Devices → Keyboard
2. Click "Add Input Method"
3. Select "Vietnamese → buttre"

**Command Line:**

Not recommended if you are using GNOME.

```bash
ibus-setup
# Then click "Add" → "Vietnamese" → "buttre"
```

### 3. Switch Input Method

Default hotkey: `Super+Space` (or configured in IBus settings)

## File Locations

```
/usr/bin/
└── buttre                      # Main binary (IBus engine + tray)

/usr/share/ibus/component/
└── buttre.xml                  # IBus component definition

/usr/share/buttre/
├── buttre_nom.db               # Nôm character database
└── keyboards/                  # Keyboard layout definitions (*.toml)

/usr/share/doc/buttre/
├── README.md
└── LICENSE
```

## Troubleshooting

### Input method not showing up

1. Verify installation:
   ```bash
   ls -la /usr/bin/buttre
   ls -la /usr/share/ibus/component/buttre.xml
   ```

2. Restart IBus:
   ```bash
   killall ibus-daemon
   ibus-daemon -drx &
   ```

3. Check IBus logs:
   ```bash
   journalctl -xe | grep ibus
   ```

### Permission issues

```bash
sudo chmod +x /usr/bin/buttre
```

### Database not found

```bash
ls -la /usr/share/buttre/buttre_nom.db
# If missing, reinstall package
```

## Uninstallation

### Debian/Ubuntu
```bash
sudo apt remove buttre
```

### Fedora/RHEL
```bash
sudo rpm -e buttre
# Or with dnf:
sudo dnf remove buttre
```

## Distribution

### Upload to COPR (Fedora)

```bash
# Create COPR project at copr.fedorainfracloud.org
# Upload .src.rpm
copr-cli build your-project buttre-0.1.0-1.src.rpm
```

## Development

To test changes without reinstalling:

```bash
# Build and run directly
cargo run --release --package buttre-platform
```
