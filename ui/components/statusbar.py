"""
NIKO StatusBar — нижня панель статусу системи.
Індикатор підключення до Core, статус LLM, кількість повідомлень, версія, аварійна зупинка.
"""

import customtkinter as ctk
from ..styles import get_color, get_font


class StatusBar(ctk.CTkFrame):
    """
    Нижня панель статусу системи.
    Відображає: статус Core, LLM, кількість повідомлень, версію, кнопку аварійної зупинки.
    """

    def __init__(self, parent, on_emergency_stop=None, **kwargs):
        super().__init__(parent, **kwargs)
        self._on_emergency_stop = on_emergency_stop
        self._build_ui()

    def _build_ui(self):
        """Створює інтерфейс статусбару."""
        self.configure(height=36)

        # --- Індикатор Core ---
        self.core_frame = ctk.CTkFrame(self, fg_color="transparent")
        self.core_frame.pack(side="left", padx=(10, 5))

        self.core_indicator = ctk.CTkLabel(
            self.core_frame,
            text="●",
            font=get_font("body"),
            text_color=get_color("success"),
            width=16,
        )
        self.core_indicator.pack(side="left")

        self.core_label = ctk.CTkLabel(
            self.core_frame,
            text="Core",
            font=get_font("small"),
            text_color=get_color("text_secondary"),
        )
        self.core_label.pack(side="left", padx=(3, 0))

        # --- Статус LLM ---
        self.llm_frame = ctk.CTkFrame(self, fg_color="transparent")
        self.llm_frame.pack(side="left", padx=(15, 5))

        self.llm_indicator = ctk.CTkLabel(
            self.llm_frame,
            text="●",
            font=get_font("body"),
            text_color=get_color("success"),
            width=16,
        )
        self.llm_indicator.pack(side="left")

        self.llm_label = ctk.CTkLabel(
            self.llm_frame,
            text="LLM: готовий",
            font=get_font("small"),
            text_color=get_color("text_secondary"),
        )
        self.llm_label.pack(side="left", padx=(3, 0))

        # --- Кількість повідомлень ---
        self.msg_frame = ctk.CTkFrame(self, fg_color="transparent")
        self.msg_frame.pack(side="left", padx=(15, 5))

        self.msg_icon = ctk.CTkLabel(
            self.msg_frame,
            text="💬",
            font=get_font("small"),
            width=20,
        )
        self.msg_icon.pack(side="left")

        self.msg_label = ctk.CTkLabel(
            self.msg_frame,
            text="0",
            font=get_font("small"),
            text_color=get_color("text_secondary"),
        )
        self.msg_label.pack(side="left", padx=(3, 0))

        # --- Версія ---
        self.version_label = ctk.CTkLabel(
            self,
            text="v1.0.0",
            font=get_font("small"),
            text_color=get_color("text_secondary"),
        )
        self.version_label.pack(side="left", padx=(15, 5))

        # --- Розтяжка ---
        self.spacer = ctk.CTkFrame(self, fg_color="transparent")
        self.spacer.pack(side="left", fill="x", expand=True)

        # --- Кнопка аварійної зупинки ---
        self.emergency_btn = ctk.CTkButton(
            self,
            text="🛑",
            font=get_font("body"),
            fg_color=get_color("danger"),
            hover_color="#b02a37",
            text_color="white",
            width=32,
            height=28,
            corner_radius=4,
            command=self._on_emergency_stop if self._on_emergency_stop else lambda: None,
        )
        self.emergency_btn.pack(side="right", padx=(5, 10))

    # --- Публічні методи ---

    def set_core_status(self, status: str):
        """
        Встановлює статус підключення до Core.
        status: "connected" (зелений), "degraded" (жовтий), "disconnected" (червоний)
        """
        colors = {
            "connected": get_color("success"),
            "degraded": get_color("warning"),
            "disconnected": get_color("danger"),
        }
        labels = {
            "connected": "Core: підключено",
            "degraded": "Core: деградація",
            "disconnected": "Core: відключено",
        }
        color = colors.get(status, get_color("danger"))
        label = labels.get(status, "Core: невідомо")
        self.core_indicator.configure(text_color=color)
        self.core_label.configure(text=label)

    def set_llm_status(self, provider: str, available: bool):
        """Встановлює статус LLM."""
        color = get_color("success") if available else get_color("danger")
        text = f"LLM: {provider}" if available else "LLM: недоступний"
        self.llm_indicator.configure(text_color=color)
        self.llm_label.configure(text=text)

    def set_message_count(self, count: int):
        """Встановлює кількість повідомлень у поточній сесії."""
        self.msg_label.configure(text=str(count))

    def set_version(self, version: str):
        """Встановлює версію системи."""
        self.version_label.configure(text=f"v{version}")
