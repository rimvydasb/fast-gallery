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
- **Consequences:** The pipeline decodes the input (`InputSpec`), not the analyzer, so 2 analyzers can share 1 decode (`Pair`, used by the bad photo scan for quality and screenshot detection). At most `perf_cores() / 2` jobs are in the decode pool, so a thumbnail job waits for at most 1 analysis job.

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

## ADR-007: The Rust core arranges a listing; groups are ranges of a flat list

- **Status:** Accepted
- **Context:** The grid must divide the images into groups (date, size, type, quality, similarity) and sort by quality and by similarity. The selection, the viewer, the keyboard, and the file actions use indexes into 1 flat list of entries.
- **Decision:** `list_dir` and `list_collection` get a `group` argument. `scan::arrange` sorts the images, puts the images of each group side by side, and returns `groups` (label and count, in order) with the flat `entries`. The folders stay first, with no header. `GridView` lays out each group as a section with a header line; the grouping is stored per location in `FolderPrefs`.
- **Rejected:** Groups computed in the frontend - quality and similarity need the analysis cache, and the date taken is only in the Rust core.
- **Rejected:** A nested listing (groups that contain entries) - each consumer of `entries()` needs a change.
- **Consequences:** The direction button also reverses the order of date groups and size groups. A row does not cross a group, so Up and Down use `gridMove`, not index ± columns.

## ADR-008: A sort or grouping that reads analysis results starts a task for the location

- **Status:** Accepted
- **Context:** Quality and similarity need a decode of each image. A listing must stay fast.
- **Decision:** A listing reads only the `analysis` cache and reports `missing` and `need`. The frontend (`analysis.ts`) then starts the task `AnalyzePhotos` for the images of the location (a folder without its subfolders, or a collection) and lists again on `task-done`. Images without a result are in the group "Not analyzed". `run_pipeline` reads an `ImageSource`: a `TreeWalker` or a `ListSource`.
- **Rejected:** Analysis in `list_dir` - no progress, no cancel, and a slow first listing.
- **Consequences:** ADR-001 applies: the analysis waits while another task runs, and it locks the folder against file changes until it ends. An image that cannot be decoded stays "Not analyzed"; the task does not start again for it by itself.

## ADR-009: Similarity is a score from 2 perceptual hashes, a color layout, and a color histogram

- **Status:** Accepted
- **Context:** Groups by similarity must not use the name, size, or type of a file. Photos of a hand-held series have hashes that differ like the hashes of unrelated photos (measured on `examples/example gallery/somebursts`).
- **Decision:** `SimilarityAnalyzer` stores a `Signature` for each image: pHash and dHash (64 bits each), a 4 × 4 RGB layout, and a 64-bin RGB histogram, from a 128 px decode. The similarity of 2 signatures is 0.25 × structure (hashes) + 0.75 × layout correlation × common palette. The levels are thresholds: `same` 0.90 (and at most 8 different hash bits), `high` 0.70, `medium` 0.55, `low` 0.40. A dense burst needs `low` and a time gap that fits the rhythm of the series; a sparse burst needs `medium` and no time limit.
- **Rejected:** Hash distance only - it does not find the example burst without many false groups.
- **Rejected:** A neural embedding - a model file and a runtime for a first version.
- **Consequences:** Assumption: the weights and thresholds are estimates from a few example photos. A change of the signature needs a new `ALGO_VERSION`; a change of a threshold needs no new analysis. The comparisons are O(n²) in the worst case.
