# MadLinux

A beginner-friendly Tauri installer and launcher for running Madium in an isolated GE-Proton environment on Linux.

## Development

```bash
npm install
npm run tauri dev
```

## Build

```bash
npm run tauri build
```

The produced AppImage and Debian package are placed in `src-tauri/target/release/bundle/`.

MadLinux bundles and verifies the supported `Installer-1.1.0.exe` by SHA-256 before running it. Users do not need to supply additional files. It does not disable or bypass Roblox security systems.
