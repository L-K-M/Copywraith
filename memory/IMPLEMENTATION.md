# Copywraith Implementation Plan

## Phase 1: Project Scaffolding

### 1.1 Initialize Rust Workspace
- Create `Cargo.toml` workspace with members: `server`, `crates/copywraith-core`, `src-tauri`
- Set up shared dependencies in workspace

### 1.2 Initialize SvelteKit Frontend
- `package.json` with `@lkmc/system7-ui`, `@tauri-apps/api`, Svelte 5, SvelteKit
- `svelte.config.js` with `adapter-static`
- `vite.config.js` for Tauri/SvelteKit dev server integration
- `tsconfig.json`
- Root layout with system7-ui CSS import and SSR disabled

### 1.3 Initialize Tauri Desktop Client
- `src-tauri/tauri.conf.json` with popup window (settings is a modal dialog)
- `src-tauri/Cargo.toml` with plugins: clipboard, global-shortcut, dialog
- `src-tauri/capabilities/default.json` with required permissions
- Basic `main.rs` and `lib.rs`

### 1.4 Initialize Server
- `server/Cargo.toml` with axum, rusqlite, tokio, serde
- `server/Dockerfile` and `docker-compose.yml`
- Basic `main.rs` with health check endpoint

## Phase 2: Shared Core Library

### 2.1 Data Models (`crates/copywraith-core/`)
- `ClipboardEntry` struct with all fields
- `ContentType` enum
- API request/response types
- Content hashing utilities

## Phase 3: Server Implementation

### 3.1 Storage Layer
- SQLite database initialization with migrations
- CRUD operations for entries
- Blob storage for binary content (images)
- Full-text search using SQLite FTS5

### 3.2 API Layer
- Axum router with all REST endpoints
- Entry creation with deduplication
- Pagination and filtering
- Binary content upload/download
- Search endpoint
- Password authentication with at-rest encryption (Argon2id + AES-256-GCM)

### 3.3 Server Admin UI
- Svelte + Vite SPA served as static files via `tower-http::ServeDir`
- Uses `@lkmc/system7-ui` components
- Built to `server/ui/dist/`, served at `/`
- Fallback HTML shown if UI not built
- Browse entries with pagination
- Preview text, HTML, and images
- Star/unstar and delete entries
- Search functionality

### 3.4 Docker Deployment
- Multi-stage Dockerfile (build Rust + embed UI assets)
- `docker-compose.yml` with volume mounts for data persistence
- Environment variable configuration

## Phase 4: Desktop Client - Rust Backend

### 4.1 Clipboard Monitoring
- Use `tauri-plugin-clipboard` for monitoring
- Detect content type changes (text, image, HTML, RTF, files)
- Hash content for deduplication
- Store in local SQLite cache

### 4.2 Local Storage
- SQLite database matching server schema
- Fast local lookups for popup display
- Sync status tracking

### 4.3 Server Sync
- Background task to sync new entries to server
- Pull starred status changes from server
- Handle offline gracefully (queue and retry)
- Conflict resolution (server wins for star status)

### 4.4 Paste Simulation
- Write selected content to system clipboard
- Simulate Cmd+V keystroke via accessibility APIs
- Support plaintext paste (strip formatting)

## Phase 5: Desktop Client - Svelte UI

### 5.1 Popup Window
- Floating window with system7-ui TitleBar
- Auto-focus filter field on show
- Entry list with DataTable
- Content previews (text truncation, image thumbnails)
- Star toggle per entry
- Keyboard navigation (arrow keys, Enter to paste)
- Click to paste, Option+Click for plaintext

### 5.2 Settings
- Primary + fallback server URL configuration
- Password configuration
- Hotkey customization
- Startup preferences (launch at login)

### 5.3 Status & Notifications
- Connection status indicator
- Sync status
- Toast notifications for errors

## Phase 6: Global Shortcuts

### 6.1 Shortcut Registration
- `Cmd+Shift+V` to toggle paste popup
- `Cmd+Shift+B` to show starred entries only
- `Cmd+Shift+Alt+V` to paste most recent as plaintext
- Register via `tauri-plugin-global-shortcut`

### 6.2 Window Positioning
- Show popup near cursor position or centered on screen
- Hide on focus loss
- Hide after paste action

## Phase 7: Polish & Testing

### 7.1 Error Handling
- Graceful degradation when server is unreachable
- Local-only mode
- Error banners and notifications

### 7.2 Performance
- Lazy loading for long clipboard history
- Image thumbnail caching
- Debounced filter input

### 7.3 Build & Distribution
- macOS DMG packaging
- App icons
- Code signing considerations
