"""Image preview widget for displaying thumbnails."""
from __future__ import annotations

import tkinter as tk
from tkinter import ttk
from PIL import Image, ImageTk
from pathlib import Path


class PreviewWidget(ttk.Frame):
    """Widget to display image thumbnails in a scrollable area."""

    def __init__(self, parent: ttk.Frame, thumbnail_size: tuple[int, int] = (150, 150)) -> None:
        super().__init__(parent)
        self.thumbnail_size: tuple[int, int] = thumbnail_size
        self.image_references: list[ImageTk.PhotoImage] = []  # Keep references to prevent garbage collection

        self._setup_ui()

    def _setup_ui(self) -> None:
        """Setup the UI components."""
        # Create canvas with scrollbar
        self.canvas = tk.Canvas(self, bg='#f0f0f0', highlightthickness=0)
        self.scrollbar = ttk.Scrollbar(self, orient='vertical', command=self.canvas.yview)
        self.scrollable_frame = ttk.Frame(self.canvas)

        self.scrollable_frame.bind(
            "<Configure>",
            lambda e: self.canvas.configure(scrollregion=self.canvas.bbox("all"))
        )

        self.canvas.create_window((0, 0), window=self.scrollable_frame, anchor="nw")
        self.canvas.configure(yscrollcommand=self.scrollbar.set)

        # Pack components
        self.canvas.pack(side='left', fill='both', expand=True)
        self.scrollbar.pack(side='right', fill='y')

        # Bind mouse wheel
        self.canvas.bind_all("<MouseWheel>", self._on_mousewheel)

    def _on_mousewheel(self, event: tk.Event) -> None:
        """Handle mouse wheel scrolling."""
        self.canvas.yview_scroll(int(-1 * (event.delta / 120)), "units")

    def clear_previews(self) -> None:
        """Clear all preview images."""
        for widget in self.scrollable_frame.winfo_children():
            widget.destroy()
        self.image_references.clear()

    def load_previews(self, image_paths: list[Path], columns: int = 5) -> None:
        """
        Load and display image thumbnails.

        Args:
            image_paths: List of image file paths
            columns: Number of columns in the grid
        """
        self.clear_previews()

        if not image_paths:
            label = ttk.Label(
                self.scrollable_frame,
                text="No images to preview",
                font=('Arial', 12)
            )
            label.grid(row=0, column=0, padx=20, pady=20)
            return

        # Display images in grid
        for idx, img_path in enumerate(image_paths):
            row = idx // columns
            col = idx % columns

            try:
                # Create thumbnail frame
                frame = ttk.Frame(self.scrollable_frame, relief='solid', borderwidth=1)
                frame.grid(row=row, column=col, padx=5, pady=5)

                # Load and resize image
                img = Image.open(img_path)
                img.thumbnail(self.thumbnail_size, Image.Resampling.LANCZOS)

                # Convert to PhotoImage
                photo = ImageTk.PhotoImage(img)
                self.image_references.append(photo)  # Keep reference

                # Create label with image
                img_label = ttk.Label(frame, image=photo)
                img_label.pack(padx=2, pady=2)

                # Add filename below image
                name_label = ttk.Label(
                    frame,
                    text=img_path.name,
                    font=('Arial', 8),
                    wraplength=self.thumbnail_size[0]
                )
                name_label.pack()

            except Exception as e:
                # If image can't be loaded, show error
                error_frame = ttk.Frame(self.scrollable_frame, relief='solid', borderwidth=1)
                error_frame.grid(row=row, column=col, padx=5, pady=5)

                error_label = ttk.Label(
                    error_frame,
                    text=f"Error loading\n{img_path.name}",
                    font=('Arial', 8),
                    foreground='red',
                    wraplength=self.thumbnail_size[0]
                )
                error_label.pack(padx=10, pady=10)

        # Update scroll region
        self.scrollable_frame.update_idletasks()
        self.canvas.configure(scrollregion=self.canvas.bbox("all"))

    def destroy(self) -> None:
        """Clean up resources."""
        self.canvas.unbind_all("<MouseWheel>")
        super().destroy()
