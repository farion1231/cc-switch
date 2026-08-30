# CC-Switch for Ubuntu 20.04 - Installation Guide

## Current Status

✅ **Successfully built**: Ubuntu 20.04-compatible CC-Switch (v3.20.0) Flatpak
📦 **Artifact**: `dist-flatpak/CC-Switch-Linux.flatpak`
🔧 **Runtime**: GNOME 49 (bundles newer glibc, so it runs on Ubuntu 20.04)

## Why Flatpak?

The native AppImage/.deb requires GLIBC 2.32+ and GLIBCXX 3.4.29+, which Ubuntu
20.04 (glibc 2.31) doesn't have. Flatpak solves this by bundling its own runtime
with all required libraries, completely isolated from the host system.

## Build (on Ubuntu 22.04, via Docker)

```bash
scripts/build-flatpak-artifact.sh
```

This builds the `.deb` in an Ubuntu 22.04 Docker container and wraps it into the
Flatpak artifact at `dist-flatpak/CC-Switch-Linux.flatpak`.

## Installation Steps (on Ubuntu 20.04)

### Option 1: Online Installation (Requires Network Access to Flathub)

```bash
# Step 1: Install the GNOME 49 runtime from Flathub
flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
flatpak install -y --user flathub org.gnome.Platform//49

# Step 2: Install CC-Switch
flatpak install --user ./dist-flatpak/CC-Switch-Linux.flatpak

# Step 3: Launch
flatpak run com.ccswitch.desktop
```

### Option 2: Offline Installation (Local Runtime)

If Flathub is unreachable, you'll need the GNOME 49 runtime bundle first. Contact
the developer for the runtime bundle.

```bash
# Install runtime from bundle
flatpak install /path/to/org.gnome.Platform-49-x86_64.flatpak

# Then install app
flatpak install --user ./dist-flatpak/CC-Switch-Linux.flatpak
```

## Troubleshooting

### "Runtime not found" Error

```
error: The application com.ccswitch.desktop/x86_64/master requires the runtime
org.gnome.Platform/x86_64/49 which was not found
```

**Solution**: Install the runtime first:
```bash
flatpak install --user flathub org.gnome.Platform//49
```

If Flathub is slow/unreachable, wait a moment and retry, or try a different network.

### "Flathub not found" Error

Ensure Flathub remote is registered:
```bash
flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
```

### Desktop Menu Not Showing App

After installation, restart your session:
```bash
# Log out and log back in, or run:
source /etc/profile.d/flatpak.sh
```

## Technical Details

- **Build Platform**: Ubuntu 22.04 (has the required newer libraries)
- **Target Platform**: Ubuntu 20.04 (lacks newer libraries; Flatpak provides them)
- **Flatpak Runtime**: GNOME 49 (self-contained with glib, GTK, webkit, etc.)
- **App Isolation**: Full sandbox with home directory access for config/data

## Uninstallation

```bash
# Remove CC-Switch
flatpak uninstall com.ccswitch.desktop

# Remove GNOME 49 runtime (optional, shared by other apps)
flatpak uninstall org.gnome.Platform//49
```

## Questions?

- **Flatpak Docs**: https://docs.flatpak.org/
- **GNOME Runtimes**: https://flathub.org/
