## Backlog Tasks

### High Priority
- [x] Add a selector on the UI to select if the filesystem date or meta data (e.g., EXIF or IPTC) photo creation date should be used. If the filesystem option is selected, keep the current implementation; if meta data is selected, add the code to parse photo metadata (EXIF, IPTC, etc.) to determine the original capture date.
- [x] Surface scan progress and optional cancellation in the UI by extending `SourceScanController` and wiring indicators into the layout.
- Refine copy-job cancellation so restored date groups avoid re-sorting on every cancel (shared store or dedicated model).

### Medium Priority
- Normalize scroll bindings in `ui/preview_widget.py` to avoid global grabs and add Linux support.
- Debounce settings writes in `config/settings.py` and introduce atomic file writes.

### Low Priority
- Capture scan metrics (file counts, duration) for telemetry/logging.
