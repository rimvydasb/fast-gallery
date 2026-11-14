# Architecture Decision Record

## ADR-001: `TaskRunner` runs 1 exclusive task for the whole app

- **Status:** Accepted
- **Context:** The bad photo scan must lock its folder tree against file changes while the user browses elsewhere. Future tasks (for example a duplicate search) need the same lock, progress, and cancel.
- **Decision:** `TaskRunner` in `AppState` holds at most 1 running task. The task has a `TaskKind`, a source window, a root, a task lock, a `CancelToken`, and progress. `TaskRunner` emits `task-progress` (at most every 100 ms) and `task-done` to all windows.
- **Rejected:** A lock for each feature - each feature repeats the busy logic, and 2 features compete for the decode threads.
- **Rejected:** Several tasks at once on separate trees - the decode threads, the single "Bad photos" collection, and the single progress indicator do not allow it.
- **Rejected:** A global block of navigation - the user rejected it: a scan is attached to its folder, and the user can browse while it runs.
- **Consequences:** A second task start returns `busy`. A close of the source window cancels its task. A future idle task (for example an index build) needs a new non-exclusive task class.

## ADR-002: The image scan pipeline is generic over `ImageAnalyzer` and `ResultSink`

- **Status:** Accepted
- **Context:** The walk, the cap, the cache, the bounded feeder, cancel, and progress are the same for each analyzer; the user stated that the pipeline will be reused.
- **Decision:** `run_pipeline` connects a `TreeWalker`, an `AnalysisCache`, an `ImageAnalyzer`, and a `ResultSink`, with generic functions (static dispatch). Photo quality supplies only `QualityAnalyzer` and `BadPhotoSink`.
- **Rejected:** A photo-quality-only pipeline - the next feature copies it.
- **Rejected:** Dynamic plugins (`Box<dyn ImageAnalyzer>`) - the analyzers are known at compile time, and generics are simpler.
- **Consequences:** The pipeline decodes the input (`InputSpec`), not the analyzer, so 2 analyzers with the same input can later share 1 decode. At most `perf_cores() / 2` jobs are in the decode pool, so a thumbnail job waits for at most 1 analysis job.

## ADR-003: `AnalysisCache` stores the results of all analyzers in one table

- **Status:** Accepted
- **Context:** Each analyzer needs a persistent cache with a stale check.
- **Decision:** The SQLite table `analysis` in `PreviewIndex` has the key `(dir, name, analyzer)`. Each row holds `version`, `mtime`, `size`, and the result as JSON. A row is stale if one of the 3 values differs.
- **Rejected:** A table for each analyzer, like `taken` - each analyzer then needs a schema change and its own stale logic.
- **Consequences:** The cache stores only metrics; thresholds apply in the sink, so a sensitivity change needs no new analysis. `clear_cache` also clears the table.

## ADR-004: The action registry is the only place that decides if an action is enabled

- **Status:** Accepted
- **Context:** Before this decision, `Toolbar.tsx`, `ContextMenu.tsx`, `keyboard.ts`, `Sidebar.tsx`, `Breadcrumbs.tsx`, `GridView.tsx`, and `App.tsx` each called store functions with their own checks. A task lock must block the same actions on every path.
- **Decision:** `actions.ts` defines each action once with a category, target paths for file changes, `enabled`, and `run`. All call sites call `runAction` and read `isEnabled` or `blockedReason`; a blocked shortcut shows the reason as a toast.
- **Rejected:** A `disabled={busy()}` check at each call site - it duplicates the rule, and a shortcut or a native menu item can bypass it.
- **Consequences:** The task rule depends only on the category and the targets. Viewer-internal controls (zoom, rotate, film strip, slideshow) and cursor movement stay local to the views.

## ADR-005: A `Location` abstraction hides the difference between disk folders and virtual folders

- **Status:** Accepted
- **Context:** Navigation, history, and the grid used a disk path string; the user stated that virtual folders and navigation will be reused.
- **Decision:** A window shows a `Location` (`disk` or `collection`). A `LocationProvider` for each kind lists entries, builds breadcrumbs, and declares `LocationCaps`. `navigation.ts` holds the current location and the history as `Location` values.
- **Rejected:** A virtual path string with prefix checks at each use - each new feature adds scattered checks.
- **Consequences:** Collection entries carry `Entry.p` (absolute path) and `Entry.b` (badge codes); `entryPath` and the selection key (`entryKey`) use `p`, because names can repeat in a collection.

## ADR-006: The Rust core also enforces the busy state

- **Status:** Accepted
- **Context:** A frontend bug or a second window with an old busy state must not change files in the tree that a scan reads.
- **Decision:** `trash_files`, `undo_trash`, and `rename_file` call `TaskRunner::guard_paths` and return `locked`. `clear_cache`, `remove_collection`, and each task start command call `TaskRunner::guard` and return `busy`.
- **Rejected:** A frontend-only block - it depends on each window having the current busy state.
- **Consequences:** A path conflicts with a locked tree if it is the root or inside the tree. A folder that contains the root also conflicts. Finder and other apps can still change the tree; the pipeline treats a missing file as failed.
