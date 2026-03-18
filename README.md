<p align="center">
  <img src="src-tauri/icons/logo.svg" alt="Codex Quota Monitor logo" width="120" height="120">
</p>

<h1 align="center">Codex Quota Monitor</h1>

<p align="center">
  A desktop quota monitor with multi-account switching for Codex CLI, the VS Code Codex extension, and the Codex app.
</p>

<p align="center">
  Built with Tauri 2, React 19, TypeScript, and Rust.
</p>

## Overview

Codex Quota Monitor is an unofficial desktop utility for people who personally manage multiple Codex / ChatGPT accounts and want a faster way to:

- switch the active `~/.codex/auth.json`
- monitor 5-hour and 7-day quota usage
- restart supported Codex runtimes after a switch
- warm up accounts with lightweight requests
- import, export, and back up account configs

The app is designed to make account management practical without manually editing auth files every time.

## What It Can Do

- Add accounts from an existing `auth.json`
- Sign in with OAuth from inside the app
- View active account, other accounts, and plan type at a glance
- Track 5-hour and weekly usage windows
- Switch account and trigger runtime restart flows for supported environments
- Detect blocking standalone Codex CLI sessions before switching
- Send warm-up requests for one account or all accounts
- Export/import slim text payloads for quick migration
- Export/import full encrypted backup files
- View a live runtime log panel inside the app

## Runtime Support

Current switching flow is built around these runtimes:

- Standalone Codex CLI
- VS Code with the Codex extension
- Codex desktop app

Behavior:

- If a standalone Codex CLI session is still running, switching is blocked.
- If VS Code or the Codex app is open, the app will attempt to restart them after switching accounts.
- Runtime restart is best-effort and depends on the local machine setup.

## Tech Stack

- Frontend: React 19, TypeScript, Vite, Tailwind CSS 4
- Desktop shell: Tauri 2
- Backend: Rust
- HTTP: `reqwest`
- Local storage and crypto: Rust-based account storage, backup encryption, and auth file handling

## Getting Started

### Prerequisites

- Node.js 18+
- `pnpm`
- Rust toolchain
- Tauri development prerequisites for your OS

### Install Dependencies

```bash
pnpm install
```

### Run in Development

```bash
pnpm tauri dev
```

### Build for Production

```bash
pnpm tauri build
```

If you want a local production binary without installer signing:

```bash
pnpm tauri build --no-bundle --no-sign
```

Typical output paths:

- App binary: `src-tauri/target/release/codex-quota-monitor.exe`
- Bundles: `src-tauri/target/release/bundle/`

## Project Structure

```text
.
|-- src/                  React UI
|-- src-tauri/            Rust + Tauri backend
|-- public/               Static assets
|-- scripts/              Utility scripts
|-- package.json          Frontend scripts and dependencies
|-- src-tauri/Cargo.toml  Rust dependencies
```

## Main User Flows

### Add Accounts

- Import an existing `auth.json`
- Complete OAuth login inside the app

### Monitor Usage

- Refresh usage for one account or all accounts
- See quota windows and reset times

### Switch Accounts

- Choose another stored account
- App writes the target auth into `~/.codex/auth.json`
- Supported runtimes are restarted in the background when possible

### Back Up and Restore

- Export a slim text payload for quick sharing across your own machines
- Export a full encrypted backup file for restore/migration

## Development Notes

- Frontend entry: `src/main.tsx`
- Main app UI: `src/App.tsx`
- Tauri entry: `src-tauri/src/lib.rs`
- Account switching logic: `src-tauri/src/commands/account.rs`
- Runtime detection and relaunch logic: `src-tauri/src/runtime.rs`

## Maintenance Docs

For maintenance, reverse-engineering, and future upgrades, see the documents in `docs/maintenance/`:

- `docs/maintenance/README.md`
- `docs/maintenance/architecture.md`
- `docs/maintenance/api-spec.md`
- `docs/maintenance/db-spec.md`
- `docs/maintenance/feature-spec.md`
- `docs/maintenance/sequence-flow.md`
- `docs/maintenance/screen-spec.md`
- `docs/maintenance/design-analysis.md`

## Disclaimer

This project is intended only for people managing accounts they personally own.

It is not designed for:

- credential sharing between different users
- team-wide account pooling
- bypassing product limits or platform rules
- automating abusive behavior

You are responsible for how you use the software and for complying with the relevant platform terms.

## Status

This is an active desktop utility project. The runtime restart flow is practical, but still machine-dependent, especially on Windows where local VS Code installation details can vary.

If you publish this repo, consider adding:

- screenshots or a short demo GIF
- a release section with downloadable binaries
- a changelog
