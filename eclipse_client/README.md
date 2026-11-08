# Eclipse Client

Launcher for the Eclipse Client mod, with its own install and launch engine: no Prism, no other
launcher. It downloads Minecraft, Forge or Fabric, libraries and assets, installs the client mod
and starts the game.

Supported versions (only these appear in the app):
1.8.9 · Forge 1.8.9-11.15.1.2318
1.20.1 · Forge 1.20.1-47.4.28
1.21.11 · Fabric (latest stable loader)
26.2 · Fabric (latest stable loader)
26.3 · Fabric (latest stable loader)

## Run
    npm install
    npm start

## Build
    npm run dist:win      # run on Windows
    npm run dist:linux    # run on Linux (AppImage + .deb)

## How launching works
- `launcher.js` — supported versions, install/launch orchestration.
- `engine/minecraft.js` — Mojang manifest, libraries, assets, natives, arguments.
- `engine/loaders.js` — Fabric profile and Forge installer (legacy 1.8.9 handled without its GUI).
- `engine/java.js` — picks a system Java per version (8, 17, 21, 25).
- `engine/launch.js` — builds the JVM command and starts the game detached.
- `mods/<version>/` — jars copied into the instance's `mods` folder (client mod + Fabric API).

Game files live in `~/.local/share/EclipseClient` on Linux, `%APPDATA%\EclipseClient` on Windows
and `~/Library/Application Support/EclipseClient` on macOS.

## Mods (Edit profile)
In Profiles → **Edit** a profile you get a **Mods** section:

- **Mods** — the jars of that profile's instance (client mod, Fabric API and everything you
  downloaded). Each downloaded jar has a ✕ to remove it; the client's own jars are read-only.
- **Download mods** — search and install straight into the instance folder, per version and
  loader: Modrinth works out of the box, CurseForge needs a free API key
  (console.curseforge.com) pasted in Settings → CurseForge API key. Required dependencies of a
  Modrinth mod are downloaded with it.

Mods are stored in `instances/<version>/mods` of the data folder above, so they survive a
reinstall of the version.

## Accounts
The launcher currently starts the game in offline mode with the username from Settings: singleplayer,
LAN and offline-mode servers work, online-mode servers (Hypixel) need Microsoft sign-in, which is
not implemented yet.

## UI files
- `main.js` — Electron window + IPC
- `preload.js` — the only bridge exposed to the page
- `src/index.html` — pages: Home, Profiles, Versions, Settings, About
- `src/style.css` — client-style theme
- `src/stars.js` — twinkling stars + shooting stars (canvas)
- `src/app.js` — profiles, versions, install progress and Play
