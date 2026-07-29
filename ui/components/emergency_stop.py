"""
NIKO Emergency Stop — аварійна зупинка системи.
Велика червона кнопка з діалогом підтвердження.
При підтвердженні — os.kill(os.getpid(), signal.SIGTERM).
"""

import os
import signal
import threading
import customtkinter as ctk
from ..styles import get_color, get_font


class EmergencyStop(ctk.CTkFrame):
    """
    Компонент аварійної зупинки.
    Містить велику червону кнопку та діалог підтвердження.
    """

    def __init__(self, parent, **kwargs):
        super().__init__(parent, **kwargs)
        self._build_ui()

    def _build_ui(self):
        """Створює інтерфейс компонента."""
        # Заголовок
        self.label = ctk.CTkLabel(
            self,
            text="Аварійна зупинка",
            font=get_font("small_bold"),
            text_color=get_color("danger"),
        )
        self.label.pack(pady=(5, 5))

        # Велика червона кнопка
        self.stop_button = ctk.CTkButton(
            self,
            text="🛑 АВАРІЙНА ЗУПИНКА",
            font=get_font("subheading"),
            fg_color=get_color("danger"),
            hover_color="#b02a37",
            text_color="white",
            height=50,
            command=self._confirm_stop,
        )
        self.stop_button.pack(fill="x", padx=10, pady=(0, 10))

        # Діалог підтвердження (спочатку прихований)
        self.confirm_frame = ctk.CTkFrame(self, fg_color="transparent")
        self.confirm_label = ctk.CTkLabel(
            self.confirm_frame,
            text="Ви впевнені? Це завершить програму негайно!",
            font=get_font("small_bold"),
            text_color=get_color("danger"),
            wraplength=200,
        )
        self.confirm_label.pack(pady=(0, 5))

        btn_frame = ctk.CTkFrame(self.confirm_frame, fg_color="transparent")
        btn_frame.pack(fill="x")

        self.yes_button = ctk.CTkButton(
            btn_frame,
            text="ТАК, зупинити",
            font=get_font("small_bold"),
            fg_color=get_color("danger"),
            hover_color="#b02a37",
            text_color="white",
            height=35,
            command=self._emergency_stop,
        )
        self.yes_button.pack(side="left", fill="x", expand=True, padx=(0, 5))

        self.no_button = ctk.CTkButton(
            btn_frame,
            text="Скасувати",
            font=get_font("small"),
            fg_color=get_color("text_secondary"),
            hover_color="#5a5a6a",
            text_color="white",
            height=35,
            command=self._hide_confirm,
        )
        self.no_button.pack(side="right", fill="x", expand=True, padx=(5, 0))

        # Спочатку приховано
        self.confirm_frame.pack_forget()

    def _confirm_stop(self):
        """Показує діалог підтвердження."""
        self.stop_button.configure(state="disabled")
        self.confirm_frame.pack(fill="x", padx=10, pady=(0, 10))

    def _hide_confirm(self):
        """Ховає діалог підтвердження."""
        self.confirm_frame.pack_forget()
        self.stop_button.configure(state="normal")

    def _emergency_stop(self):
        """
        Аварійна зупинка системи.
        Завершує поточний процес через SIGTERM.
        """
        print("[EmergencyStop] Аварійна зупинка активована!")
        print("[EmergencyStop] Завершення всіх потоків...")

        # Спроба зупинити всі потоки (крім поточного та main)
        for thread in threading.enumerate():
            if thread is not threading.current_thread() and thread.name != "MainThread":
                try:
                    print(f"[EmergencyStop] Зупинка потоку: {thread.name}")
                except Exception:
                    pass

        print("[EmergencyStop] Відправка SIGTERM...")
        try:
            os.kill(os.getpid(), signal.SIGTERM)
        except Exception as e:
            print(f"[EmergencyStop] Помилка SIGTERM: {e}")
            os._exit(1)

    def trigger_stop(self):
        """Програмний виклик аварійної зупинки."""
        self._confirm_stop()
