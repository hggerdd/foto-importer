## Backlog Tasks

### High Priority
- [x] Surface scan progress and optional cancellation in the UI by extending `SourceScanController` and wiring indicators into the layout.
- Refine copy-job cancellation so restored date groups avoid re-sorting on every cancel (shared store or dedicated model).

### Medium Priority
- Normalize scroll bindings in `ui/preview_widget.py` to avoid global grabs and add Linux support.
- Debounce settings writes in `config/settings.py` and introduce atomic file writes.

### Low Priority
- Capture scan metrics (file counts, duration) for telemetry/logging.
