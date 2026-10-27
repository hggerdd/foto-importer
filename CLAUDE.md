# CLAUDE.md

Guidance for Claude Code in this repository. Also follow [AGENTS.md](AGENTS.md).

## Project Overview

Foto-Importer is a local Python 3.13+/Tkinter application for camera-file import,
optional Adobe DNG conversion, a persistent photo collection, metadata browsing,
and local face recognition with group naming and review. The current architecture
is documented in [ADR/README.md](ADR/README.md).

## Architecture Decisions (Mandatory)

- Before planning or implementing changes, read the ADR index and the accepted
  records covering the affected area. Name relevant ADR numbers in the working
  plan or change description, and account for their constraints in code and tests.
- Introduce one ADR per new or materially changed architectural decision, using
  [ADR/TEMPLATE.md](ADR/TEMPLATE.md) and the next unused four-digit number. This
  includes technology/dependencies, data storage and identity, interfaces,
  concurrency, privacy, runtime/deployment, and defining user workflows. Cosmetic
  edits and local details within existing decisions do not need another ADR.
- Record context, alternatives, consequences, limitations, related ADRs, and
  concrete implementation/test evidence. Separate proposals from implemented
  behavior; do not invent historical decision dates or past evaluations.
- An accepted ADR must not be silently contradicted. If the authorized work
  changes the decision, add a replacement, explain why, cross-link both, mark the
  old record superseded, and preserve its original rationale. Follow
  [ADR-0001](ADR/0001-adr-prozess.md) and the lifecycle in the index.
- Keep records, index, implementation, tests, and user documentation consistent
  in the same change. Cite relevant/new/superseded ADRs and validation in commit/PR
  descriptions. A recorded design is not proof of complete implementation.

## Development Commands

**Install dependencies:**

```bash
uv sync
```

**Run the application:**

```bash
uv run python main.py
```

**Run tests:**

```bash
uv run python -m pytest
```

**Package management:**

```bash
uv add <package> # Add a new dependency
uv lock          # Update uv.lock
```

## Architecture

Use these boundaries rather than putting domain logic in widgets
([ADR-0002](ADR/0002-modulare-desktop-anwendung.md)):

### Core Package (`core/`)

Business logic and data management:

- `file_manager.py`, `import_plan.py`, `copy_worker.py`: source scans, date selection,
  validation and non-destructive import.
- `task_scheduler.py`, `dng_worker.py`, `dng_updates.py`: cooperative background
  scheduling, optional external conversion and update information.
- `collection_index.py`, `collection_metadata.py`, `metadata_browser.py`: SQLite
  inventory, stable photo identities, read-only metadata extraction and browsing.
- `face_store.py`, `face_models.py`, `face_images.py`, `face_service.py`: face schema,
  manifest-based CPU models, source-aware image handling and resumable analysis.
- `face_algorithms.py`, `face_dataset.py`, `face_naming.py`, `person_graph.py`:
  matching/references, export/calibration, atomic naming/undo and relationships.

### UI Package (`ui/`)

User interface components (all tkinter-based):

- `main_window.py`, `layout.py`: coordinate the import workspace.
- `controllers.py`, `selection_controller.py`, `import_preflight.py`,
  `dng_controller.py`: marshal asynchronous operations.
- `dispatcher.py`: delivers worker results on Tk's thread; never access Tk
  variables or widgets from workers.
- `preview_widget.py`: virtualized import gallery; `date_list_widget.py` holds
  date selections; `progress_manager.py` displays task state/history.
- `collection_controller.py`, `metadata_view.py`: collection operations and queries.
- `face_controller.py`, `face_workspace.py`, `face_group_row.py`, `face_view.py`:
  group-first naming/review and advanced tools.

### Config Package (`config/`)

Application configuration:

- `settings.py`: runtime defaults and local user JSON settings. Collection options
  live in SQLite; model input specifications and licenses live in `models/*.json`
  ([ADR-0013](ADR/0013-konfigurationsbereiche.md)).

### Entry Point

- `main.py` creates the root window and starts the event loop; keep it lightweight.

### Documentation and Tests

- `ADR/`: decision index, records and template; consult before development.
- `docs/`: specifications, user guides and reproducible UI screenshots.
- `tests/core/`, `tests/ui/`: filesystem, service and real Tk regression tests.
- `scripts/`, `benchmarks/`: model installation/evaluation, isolated previews and
  explicit performance measurements.

## Application Workflow

1. Choose source and target; scan asynchronously using the selected date source.
2. Check date groups, browse all previews through lazy loading and choose a name.
3. Validate destination and indexed duplicate status, then copy in the background.
4. Optionally convert copied NEFs to DNG and refresh the collection index.
5. Browse collection metadata or analyse faces. Name a group inline with Enter,
   deselect outliers and review prefilled proposals using the same workflow.

## Key Features

- Preserve originals and existing destination files. See ADR-0005 and ADR-0007.
- Keep interactive work responsive; bounded copy/DNG lanes yield at checkpoints.
  See ADR-0003 and ADR-0004.
- Use stable photo/person IDs, not filenames or names as identity. See ADR-0009
  and ADR-0019. Respect the collection writer lock and schema migrations (ADR-0008).
- Face inference is local CPU work. Verified models stay interchangeable, and
  only confirmed high-quality labels become references. See ADR-0014–0017.
- Main face workflows are group rows, inline names, Enter/Tab, outlier rejection
  and undo; administration remains in the options menu (ADR-0018).
- Tests normally run offline. GUI tests need a graphical session; real model
  consistency tests need installed weights and private reference photos.
  Do not claim measured recognition targets from synthetic tests (ADR-0020).

## Code Style

- Follow PEP 8, use modern annotations and type public APIs.
- Keep UI operations on Tk's thread and filesystem/data operations in services.
- Document side effects and cancellation in concise docstrings; extract cohesive
  modules when needed. There is no claim that all existing files are small.
- Create a feature branch before new feature code and commit/push useful checkpoints.
- Include relevant validation and ADR references with changes; preserve historical ADRs.
