# DivineClient — Tauri (Rust)

The launcher, rewritten: same UI, same engine, now Rust + Tauri instead of Electron/Node.
It still installs and starts Minecraft by itself (no Prism, no other launcher).

## Graphics are the same files

`src/` is a byte-identical copy of the Electron frontend — `index.html`, `style.css`,
`stars.js`, `app.js`, `assets/` — with only two additions:

- `src/bridge.js` — defines the same `window.eclipse` object the Electron preload exposed,
  backed by Tauri commands instead of `ipcRenderer`.
- in `index.html`: one `<script src="bridge.js">` tag, and `connect-src ipc: http://ipc.localhost`
  in the CSP so the webview can talk to Rust.

Nothing else in the markup or styles changed, so the launcher looks exactly as before.

## Run

    cd src-tauri
    cargo run

First build takes a few minutes (Tauri crates); later builds are quick. Linux needs
`libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libsoup-3.0-dev`.

Windows/macOS: same command, the platform's webview is used.

## Tests

    cd src-tauri
    cargo test                                  # engine: status, UUID, paths, mods, Modrinth, payloads
    cargo test -- --ignored launch_189           # really starts Minecraft 1.8.9 (needs a display)
    cargo test -- --ignored install_189          # re-runs the Forge 1.8.9 install

## Layout

- `src-tauri/src/main.rs` — window + the 7 Tauri commands the UI calls
  (`eclipse_status`, `eclipse_install`, `eclipse_launch`, `eclipse_get_mods`,
  `eclipse_remove_mod`, `eclipse_search_mods`, `eclipse_download_mod`).
- `src-tauri/src/engine/client.rs` — supported versions, data folder, status, install, launch.
- `src-tauri/src/engine/minecraft.rs` — Mojang manifest, rules, libraries, natives, assets, arguments.
- `src-tauri/src/engine/loaders.rs` — Fabric profile, Forge installer, legacy 1.8.9 Forge extraction.
- `src-tauri/src/engine/launch.rs` — JVM command line, offline UUID, detached process.
- `src-tauri/src/engine/java.rs` — picks Java 8 / 17 / 21 / 25 from the system.
- `src-tauri/src/engine/mods.rs` — Modrinth + CurseForge search/download, manifest, removal.
- `src-tauri/src/engine/ipc.rs` — payload shapes the web UI sends.
- `mods/<version>/` — the client's own jars, copied into an instance (next to the binary in a release).

## Same data folder as the Electron build

Game files stay in `~/.local/share/EclipseClient` (Linux), `%APPDATA%\EclipseClient` (Windows),
`~/Library/Application Support/EclipseClient` (macOS), so everything already installed (versions,
mods, worlds, configs) is reused. `ECLIPSE_HOME` overrides it, `ECLIPSE_MODS_DIR` overrides where
the bundled jars are read from.

## Support

Versions: 1.8.9 · Forge, 1.20.1 · Forge, 1.21.11 · Fabric, 26.2 · Fabric, 26.3 · Fabric.

Mod browser: Modrinth works out of the box; CurseForge needs the free API key from
console.curseforge.com pasted in Settings. Accounts are offline-only for now (online-mode
servers need Microsoft sign-in, not implemented).
