# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Camera File Organizer - A GUI application for organizing camera files by date with preview and batch operations. Built with Python 3.13 and tkinter, using a clean modular architecture.

## Development Commands

**Install dependencies:**
```bash
uv sync
```

**Run the application:**
```bash
python main.py
# or via uv
uv run main.py
```

**Package management:**
```bash
uv add <package> # Add a new dependency
uv lock          # Update uv.lock
```

## Architecture

The application follows a clean, modular architecture organized into three main packages:

### Core Package (`core/`)
Business logic and data management:
- **file_manager.py** - File scanning and date grouping. Scans folders recursively, groups by creation date, filters by supported extensions
- **copy_worker.py** - Background file copy operations using threading. Creates organized folder structure: `target/custom_name/extension/files`

### UI Package (`ui/`)
User interface components (all tkinter-based):
- **main_window.py** - Main application window (~280 lines). Coordinates all UI components and business logic
- **folder_selector.py** - Widget for source/target folder selection with remembered paths
- **date_list_widget.py** - Listbox widget for displaying and selecting date groups
- **preview_widget.py** - Scrollable thumbnail grid for image previews
- **progress_manager.py** - Manages progress bars for concurrent copy operations

### Config Package (`config/`)
Application configuration:
- **settings.py** - Persistent settings management (JSON-based). Stores last used folders, window geometry, preferences

### Entry Point
- **main.py** - Application entry point (17 lines). Simply creates root window and starts the app

## Application Workflow

1. User selects source folder (last used folder remembered)
2. App scans and groups files by creation date
3. User selects date group(s) and sees image previews (configurable count)
4. User assigns custom name to selected groups
5. Click "Execute" to copy files to target folder with organized structure
6. Copy runs in background; completed groups are removed from list immediately
7. Progress bars show real-time copy status
8. User continues organizing remaining dates while copies run

## Key Features

- **Persistent settings**: Last used folders, window size, preferences saved automatically
- **Supported formats**: JPG, PNG, RAW (NEF, CR2, CR3, ARW, DNG, etc.), videos (MP4, MOV, AVI)
- **Non-destructive**: Files are copied, not moved or deleted
- **Organized output**: Subfolders created by file extension
- **Duplicate handling**: Automatic filename conflict resolution
- **Background processing**: Threading for responsive UI during copies
- **Multiple concurrent jobs**: Progress bars for each active copy operation
- **Smart confirmation**: Only asks confirmation if target folder already exists

## Code Style

- **Modern Python 3.13**: Uses `from __future__ import annotations`, pipe union types, built-in generics
- **Full type hints**: All functions, parameters, and return types annotated
- **Clean separation**: UI, business logic, and config are completely separated
- **Small files**: No file exceeds 280 lines; most are under 150 lines
