# Cobalt API Server Migration Implementation Plan

**Goal:** Run this repository's Cobalt API on `47.241.10.142` and point the desktop client at it without interrupting services already running on that host.

**Architecture:** Inventory the host before mutation, then install into a dedicated `/opt/cobalt-api` release directory. Bind the Node API only to an unused loopback port and expose it through an isolated Nginx listener or route chosen after inspecting existing virtual hosts. Manage it with a uniquely named systemd unit so no existing PM2, Nginx site, database, or application process is restarted.

**Tech Stack:** Node.js 22, pnpm, systemd, Nginx, Rust/Tauri client configuration, SSH/SCP.

---

### Task 1: Inventory and isolation

**Files:**
- Inspect remote Nginx, PM2, systemd, ports, firewall, disk and runtime versions.

1. Confirm an unused internal and public port.
2. Record existing Nginx configuration and validate it before changes.
3. Choose unique names: `/opt/cobalt-api`, `cobalt-api.service`, and a dedicated Nginx configuration.

### Task 2: Package and upload

**Files:**
- Deploy: `api/`

1. Run API unit tests locally.
2. Create an archive excluding dependencies, tests and transient files.
3. Upload to a temporary release directory on the server.
4. Install production dependencies with the existing Node.js/pnpm runtime.

### Task 3: Configure and start the isolated service

**Files:**
- Create remote: `/etc/cobalt-api.env`
- Create remote: `/etc/systemd/system/cobalt-api.service`
- Create remote: isolated Nginx configuration if required.

1. Bind the API to the selected loopback port.
2. Configure `API_URL`, rate limits and production environment.
3. Start only `cobalt-api.service`.
4. Validate the loopback health endpoint and a representative POST request.
5. Validate and reload Nginx without restarting unrelated application processes.

### Task 4: Switch the desktop client

**Files:**
- Modify: `src-tauri/src/lib.rs`

1. Replace the old default API URL with the verified new endpoint.
2. Run Rust tests, Svelte checks and the release check.
3. Confirm the diff contains no credentials or unrelated changes.

### Task 5: Verify and document rollback

1. Check the public endpoint from the local machine.
2. Confirm pre-existing listeners and services remain active.
3. Record service status, paths and rollback commands.
4. Remove temporary archives and local credential helper files.
