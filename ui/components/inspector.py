"""
NIKO Inspector — права бічна панель.
Відображає контекст, план, виклики інструментів та пам'ять.
"""

import customtkinter as ctk
from ..styles import get_color, get_font


class Inspector(ctk.CTkScrollableFrame):
    """
    Права бічна панель (w=280, bg=sidebar).
    Містить секції: Context, Plan, Tool Calls, Memory.
    """

    SECTION_KEYS = ["context", "plan", "tool_calls", "memory"]

    def __init__(self, parent, **kwargs):
        super().__init__(
            parent,
            width=280,
            fg_color=get_color("sidebar"),
            scrollbar_button_color=get_color("scrollbar"),
            scrollbar_button_hover_color=get_color("hover"),
            **kwargs,
        )
        self.pack_propagate(False)
        self._sections = {}
        self._build_ui()

    def _build_ui(self):
        """Створює інтерфейс інспектора."""
        # --- Заголовок ---
        self.header_label = ctk.CTkLabel(
            self,
            text="Inspector",
            font=get_font("small"),
            text_color=get_color("text_secondary"),
        )
        self.header_label.pack(anchor="w", padx=15, pady=(15, 5))

        # --- Роздільник ---
        ctk.CTkFrame(self, height=1, fg_color=get_color("border")).pack(
            fill="x", padx=15, pady=(0, 10)
        )

        # --- Секції ---
        section_configs = [
            ("context", "Context", "mono"),
            ("plan", "Plan", "body"),
            ("tool_calls", "Tool Calls", "body"),
            ("memory", "Memory", "body"),
        ]

        for key, title, font_name in section_configs:
            self._create_section(key, title, font_name)

    def _create_section(self, key: str, title: str, font_name: str):
        """Створює одну секцію з заголовком і текстовим полем."""
        # Картка секції
        frame = ctk.CTkFrame(self, fg_color=get_color("card", "bg"), corner_radius=8)
        frame.pack(fill="x", padx=10, pady=4)

        # Заголовок секції
        label = ctk.CTkLabel(
            frame,
            text=title,
            font=get_font("small_bold"),
            text_color=get_color("text_secondary"),
        )
        label.pack(anchor="w", padx=10, pady=(8, 4))

        # Текстовий вміст
        textbox = ctk.CTkTextbox(
            frame,
            font=get_font(font_name),
            fg_color="transparent",
            text_color=get_color("text"),
            border_width=0,
            height=80,
            wrap="word",
            state="disabled",
        )
        textbox.pack(fill="x", padx=10, pady=(0, 8))

        self._sections[key] = {
            "frame": frame,
            "label": label,
            "textbox": textbox,
            "font_name": font_name,
        }

    def update_section(self, section: str, content: str):
        """
        Оновлює вміст секції.

        Args:
            section: Назва секції ('context', 'plan', 'tool_calls', 'memory')
            content: Новий текстовий вміст
        """
        section_data = self._sections.get(section)
        if not section_data:
            return

        textbox = section_data["textbox"]
        textbox.configure(state="normal")
        textbox.delete("0.0", "end")
        textbox.insert("0.0", content)
        textbox.configure(state="disabled")

    def clear_section(self, section: str):
        """Очищає вміст секції."""
        self.update_section(section, "")

    def clear_all(self):
        """Очищає всі секції."""
        for key in self.SECTION_KEYS:
            self.clear_section(key)

    def set_section_height(self, section: str, height: int):
        """Змінює висоту текстового поля секції."""
        section_data = self._sections.get(section)
        if section_data:
            section_data["textbox"].configure(height=height)
