# Repository Guidelines

## Project Structure & Module Organization
- `main.py` bootstraps the Tkinter app and should stay lightweight, delegating logic to other modules.
- `core/` contains non-UI services such as `file_manager.py` for filesystem orchestration and `copy_worker.py` for long-running transfer tasks; keep new domain logic here.
- `ui/` hosts Tkinter widgets (`main_window.py`, `folder_selector.py`, etc.); new UI components should mirror this modular pattern.
- `config/settings.py` centralizes runtime defaults (folder locations, filters); prefer adjusting configuration here rather than sprinkling constants.
- `tests/core/` and `tests/ui/` contain pytest regression tests, including real Tkinter interactions.
- `ADR/` records architecture decisions and the constraints governing future development; start with [ADR/README.md](ADR/README.md).

## Architecture Decisions (Mandatory)
- Before planning or implementing a change, read [ADR/README.md](ADR/README.md) and the accepted ADRs relevant to the affected modules and workflows. Identify their numbers in the working plan or change description and preserve their constraints in implementation and validation.
- Create one ADR for each new or materially changed architectural decision: technology or dependency choices, persistence and identity, interfaces, concurrency, privacy, deployment/runtime behavior, or defining user workflows. Use [ADR/TEMPLATE.md](ADR/TEMPLATE.md), the next unused four-digit number, and a descriptive filename. Cosmetic edits and local implementation details consistent with existing ADRs do not need a new ADR.
- Document context, decision, alternatives, consequences, limitations, affected ADRs, and concrete code/test references. Distinguish proposed behavior from implemented behavior; do not invent historical dates or evaluations when documenting decisions retrospectively.
- Do not silently contradict an accepted ADR. When the authorized change requires a different decision, add a replacement ADR, explain the reason, link both records, mark the previous record as superseded, and retain its original rationale. Follow the lifecycle in [ADR-0001](ADR/0001-adr-prozess.md).
- Keep the ADR index, affected records, implementation, tests, and user documentation consistent in the same change. Mention relevant/new/superseded ADRs and validation in the commit/PR description. An ADR is a design constraint, not proof that all code already meets it.

## Build, Test, and Development Commands
- `uv sync` installs the locked dependencies defined in `pyproject.toml`/`uv.lock`.
- `uv run python main.py` launches the GUI with the resolved environment; use this for local iteration.
- `uv run python -m pytest` is the test entry point; keep relevant tests green before opening a PR. GUI tests require a graphical session; real model tests are opt-in (see ADR-0020).

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
- Reference the applicable ADR numbers and explain any added or superseded decision records.
- create a new branch before changing any code for a new feature
- commit and push steps with possible breaks for good way backs
