# Repository Guidelines

## Project Structure & Module Organization
- `main.py` bootstraps the Tkinter app and should stay lightweight, delegating logic to other modules.
- `core/` contains non-UI services such as `file_manager.py` for filesystem orchestration and `copy_worker.py` for long-running transfer tasks; keep new domain logic here.
- `ui/` hosts Tkinter widgets (`main_window.py`, `folder_selector.py`, etc.); new UI components should mirror this modular pattern.
- `config/settings.py` centralizes runtime defaults (folder locations, filters); prefer adjusting configuration here rather than sprinkling constants.
- No `tests/` directory is present yet—add it at the repository root when introducing automated tests.

## Build, Test, and Development Commands
- `uv sync` installs the locked dependencies defined in `pyproject.toml`/`uv.lock`.
- `uv run python main.py` launches the GUI with the resolved environment; use this for local iteration.
- `uv run python -m pytest` is the expected test entry point once tests exist; keep it green before opening a PR.

## Coding Style & Naming Conventions
- Follow PEP 8: four-space indentation, snake_case for functions/modules, UpperCamelCase for classes, and type hints for public APIs (as used throughout `core/` and `ui/`).
- Keep UI widgets thin and delegate file operations to `core/`; avoid circular imports by exposing shared helpers through `core/__init__.py` if necessary.
- Use concise docstrings that explain side effects, especially for filesystem operations and long-running tasks.

## Testing Guidelines
- Adopt `pytest` with tests placed under `tests/` mirroring the package layout (e.g., `tests/core/test_file_manager.py`).
- Use `tmp_path` fixtures to simulate source/target directories and cover edge cases such as duplicate filenames or unreadable files.
- Aim for coverage of date parsing, copy orchestration, and UI-controller interactions; add regression tests when bugs are fixed.

## Commit & Pull Request Guidelines
- Write commits in the imperative mood with ~50-character summaries (e.g., `Add preview widget for RAW files`) and include focused diffs.
- Squash experimental commits before pushing; each PR should describe the motivation, key changes, and manual test steps.
- Link issues when available, attach screenshots or screen recordings for UI changes, and restate any configuration updates touching `config/settings.py`.
- create a new branch before changing any code for a new feature
- commit and push steps with possible breaks for good way backs
