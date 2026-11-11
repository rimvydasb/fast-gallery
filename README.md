# Fast Gallery

A fast macOS image browser built with Tauri 2, Rust, and SolidJS. Each folder tile shows the first 4 images that the
folder contains, found recursively. See [docs/requirements.md](docs/requirements.md) for the requirements and
[docs/benchmarks.md](docs/benchmarks.md) for the measured performance budgets.

## Requirements

- macOS 12 or later
- Rust (stable, through `rustup`), with the `x86_64-apple-darwin` target for a universal build
- Node.js 20 or later

## Commands

| Command                      | Purpose                                                               |
|------------------------------|-----------------------------------------------------------------------|
| `npm install`                | Install the frontend dependencies                                     |
| `npm run tauri dev`          | Run the app with the Vite dev server                                  |
| `npm run tauri build`        | Build the release `.app` for this machine                             |
| `npm run build:universal`    | Build a universal `.app` (Apple Silicon and Intel)                    |
| `npm run bench`              | Run the performance benchmark (PERF-05, PERF-07)                      |
| `cd src-tauri && cargo test` | Run the Rust tests                                                    |
| `npm run typecheck`          | Type-check the frontend                                               |

## Keyboard Shortcuts

| Shortcut              | Action                                                    |
|-----------------------|-----------------------------------------------------------|
| `⌘O`                  | Open a folder                                             |
| `⌘N`                  | New window                                                |
| `⌘,`                  | Settings                                                  |
| `⌘[` / `⌘]`           | Back / forward                                            |
| `⌘↑`                  | Enclosing folder                                          |
| `⌘↓`, `Return`        | Open the selected folder or image                         |
| Arrow keys            | Move the selection (with `⇧` to extend it)                |
| Letters               | Select the first item whose name starts with the letters  |
| `⌘A`                  | Select all                                                |
| `⌘+` / `⌘-`           | Tile size (grid) or zoom (viewer)                         |
| `⌘⇧.`                 | Show or hide hidden files                                 |
| `⌘⌥S`                 | Show or hide the sidebar                                  |
| `⌘D`                  | Add or remove the folder in Favorites                     |
| `⌘I`                  | Information panel                                         |
| `⌘C`                  | Copy the selected files                                   |
| `⌘⇧R`                 | Reveal in Finder                                          |
| `⇧F10`                | Context menu                                              |
| `⌘E`                  | Edit mode on or off                                       |
| `⌘⌫`                  | Move to Trash (edit mode only)                            |
| `⌘Z`                  | Undo the last move to Trash (edit mode only)              |
| `→`, `Space`          | Viewer: next image                                        |
| `←`, `⇧Space`         | Viewer: previous image                                    |
| `Home` / `End`        | Viewer: first / last image                                |
| `⌘0` / `⌘1`           | Viewer: fit / 100 %                                       |
| `⌘R` / `⌘L`           | Viewer: rotate the view right / left                      |
| `F`                   | Viewer: film strip                                        |
| `S`                   | Viewer: slideshow                                         |
| `⌃⌘F`                 | Full screen                                               |
| `Escape`              | Close the viewer                                          |

## Project Structure

```text
src/                     # frontend: SolidJS, TypeScript, CSS
├── views/               # GridView, ViewerView, SettingsView
├── components/          # Sidebar, Breadcrumbs, Tile, Toolbar, StatusBar, InfoPanel, ContextMenu
└── lib/                 # API wrappers, window state, keyboard shortcuts, thumbnail queue
src-tauri/
├── src/
│   ├── commands/        # Tauri commands, EditModeState, settings, file operations
│   ├── protocols/       # thumb:// and image:// handlers
│   ├── scan/            # DirScanner, FolderPreviewService, PreviewIndex (SQLite)
│   ├── thumbs/          # ThumbnailService, ThumbCache
│   ├── decode/          # ImageDecoder (ImageIO through objc2), EXIF thumbnail parser
│   └── watch/           # FsWatcher (FSEvents)
└── benches/core.rs      # PERF-14 benchmark
```
