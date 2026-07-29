"""
NIKO Sidebar — ліва панель навігації.
Логотип, навігаційні кнопки, селектор агентів, Tool Logs, Context, очищення пам'яті.
"""

import customtkinter as ctk
from ..styles import get_color, get_font


class Sidebar(ctk.CTkFrame):
    """
    Ліва панель навігації.
    Містить логотип NIKO, навігацію, селектор агентів, логи, контекст.
    """

    AVAILABLE_AGENTS = [
        "NIKO (Core)",
        "Orchestrator",
        "Dev_Agent",
        "DB_Admin_Agent",
        "React_UI_UX_Agent",
        "QA_Agent",
        "System_Architect_Agent",
        "Infrastructure_Master_Agent",
        "Tauri_Agent",
        "Creator_Agent",
        "PM_Agent",
        "apiarm_agent",
    ]

    def __init__(
        self,
        parent,
        on_navigate=None,
        on_agent_change=None,
        on_clear_memory=None,
        **kwargs,
    ):
        super().__init__(parent, **kwargs)
        self._on_navigate = on_navigate
        self._on_agent_change = on_agent_change
        self._on_clear_memory = on_clear_memory
        self._current_nav = "chat"
        self._build_ui()

    def _build_ui(self):
        """Створює інтерфейс сайдбару."""
        self.configure(width=260)
        self.pack_propagate(False)

        # --- Логотип NIKO ---
        self.logo_frame = ctk.CTkFrame(self, fg_color="transparent")
        self.logo_frame.pack(fill="x", padx=15, pady=(15, 10))

        self.logo_label = ctk.CTkLabel(
            self.logo_frame,
            text="⚡ NIKO",
            font=get_font("heading"),
            text_color=get_color("accent"),
        )
        self.logo_label.pack(anchor="w")

        self.subtitle_label = ctk.CTkLabel(
            self.logo_frame,
            text="Нейромережевий Інтелект",
            font=get_font("tiny"),
            text_color=get_color("text_secondary"),
        )
        self.subtitle_label.pack(anchor="w")

        # --- Роздільник ---
        ctk.CTkFrame(self, height=1, fg_color=get_color("border")).pack(
            fill="x", padx=15, pady=(5, 10)
        )

        # --- Навігаційні кнопки ---
        self.nav_frame = ctk.CTkFrame(self, fg_color="transparent")
        self.nav_frame.pack(fill="x", padx=10, pady=(0, 10))

        self.nav_buttons = {}
        nav_items = [
            ("chat", "💬 Чат"),
            ("agents", "🤖 Агенти"),
            ("logs", "📋 Логи"),
            ("settings", "⚙️ Налаштування"),
        ]

        for nav_id, nav_text in nav_items:
            btn = ctk.CTkButton(
                self.nav_frame,
                text=nav_text,
                font=get_font("body"),
                fg_color=get_color("accent") if nav_id == "chat" else "transparent",
                hover_color=get_color("hover"),
                text_color="white" if nav_id == "chat" else get_color("text"),
                anchor="w",
                height=36,
                corner_radius=6,
                command=lambda nid=nav_id: self._navigate(nid),
            )
            btn.pack(fill="x", pady=2)
            self.nav_buttons[nav_id] = btn

        # --- Роздільник ---
        ctk.CTkFrame(self, height=1, fg_color=get_color("border")).pack(
            fill="x", padx=15, pady=(5, 10)
        )

        # --- Селектор агентів ---
        self.agent_label = ctk.CTkLabel(
            self,
            text="Активний агент",
            font=get_font("small_bold"),
            text_color=get_color("text_secondary"),
        )
        self.agent_label.pack(anchor="w", padx=15, pady=(0, 5))

        self.agent_var = ctk.StringVar(value=self.AVAILABLE_AGENTS[0])
        self.agent_combo = ctk.CTkComboBox(
            self,
            values=self.AVAILABLE_AGENTS,
            variable=self.agent_var,
            font=get_font("small"),
            dropdown_font=get_font("small"),
            fg_color=get_color("input_bg"),
            border_color=get_color("border"),
            button_color=get_color("accent"),
            button_hover_color=get_color("hover"),
            dropdown_fg_color=get_color("sidebar"),
            dropdown_hover_color=get_color("hover"),
            dropdown_text_color=get_color("text"),
            height=32,
            command=self._on_agent_change_callback,
        )
        self.agent_combo.pack(fill="x", padx=15, pady=(0, 10))

        # --- Tool Logs ---
        self.logs_label = ctk.CTkLabel(
            self,
            text="📋 Tool Logs",
            font=get_font("small_bold"),
            text_color=get_color("text_secondary"),
        )
        self.logs_label.pack(anchor="w", padx=15, pady=(0, 3))

        self.logs_textbox = ctk.CTkTextbox(
            self,
            font=get_font("tiny"),
            fg_color=get_color("input_bg"),
            text_color=get_color("text"),
            border_width=0,
            height=120,
            wrap="word",
            state="disabled",
        )
        self.logs_textbox.pack(fill="x", padx=15, pady=(0, 10))

        # --- Context ---
        self.context_label = ctk.CTkLabel(
            self,
            text="📄 Context",
            font=get_font("small_bold"),
            text_color=get_color("text_secondary"),
        )
        self.context_label.pack(anchor="w", padx=15, pady=(0, 3))

        self.context_textbox = ctk.CTkTextbox(
            self,
            font=get_font("tiny"),
            fg_color=get_color("input_bg"),
            text_color=get_color("text"),
            border_width=0,
            height=80,
            wrap="word",
            state="disabled",
        )
        self.context_textbox.pack(fill="x", padx=15, pady=(0, 10))

        # --- Кнопка очищення пам'яті ---
        self.clear_btn = ctk.CTkButton(
            self,
            text="🧹 Очистити пам'ять",
            font=get_font("small"),
            fg_color="transparent",
            hover_color=get_color("hover"),
            text_color=get_color("text"),
            border_width=1,
            border_color=get_color("border"),
            height=32,
            command=self._on_clear_memory_callback,
        )
        self.clear_btn.pack(fill="x", padx=15, pady=(0, 15))

    def _navigate(self, nav_id: str):
        """Перемикає активну навігаційну кнопку."""
        if nav_id == self._current_nav:
            return
        prev_btn = self.nav_buttons.get(self._current_nav)
        if prev_btn:
            prev_btn.configure(fg_color="transparent", text_color=get_color("text"))
        new_btn = self.nav_buttons.get(nav_id)
        if new_btn:
            new_btn.configure(fg_color=get_color("accent"), text_color="white")
        self._current_nav = nav_id
        if self._on_navigate:
            self._on_navigate(nav_id)

    def _on_agent_change_callback(self, choice: str):
        if self._on_agent_change:
            self._on_agent_change(choice)

    def _on_clear_memory_callback(self):
        if self._on_clear_memory:
            self._on_clear_memory()

    def add_tool_log(self, tool_name: str, args: str, result: str):
        self.logs_textbox.configure(state="normal")
        from datetime import datetime
        ts = datetime.now().strftime("%H:%M:%S")
        log_entry = f"[{ts}] {tool_name}({args}) -> {result}\n"
        self.logs_textbox.insert("end", log_entry)
        self.logs_textbox.see("end")
        self.logs_textbox.configure(state="disabled")

    def set_context(self, text: str):
        self.context_textbox.configure(state="normal")
        self.context_textbox.delete("0.0", "end")
        self.context_textbox.insert("0.0", text)
        self.context_textbox.configure(state="disabled")

    def refresh_agents(self, agents: list = None):
        if agents is not None:
            self.agent_combo.configure(values=agents)
        else:
            self.agent_combo.configure(values=self.AVAILABLE_AGENTS)

    def get_current_agent(self) -> str:
        return self.agent_var.get()

    def set_current_agent(self, agent_name: str):
        if agent_name in self.AVAILABLE_AGENTS:
            self.agent_var.set(agent_name)

    def clear_logs(self):
        self.logs_textbox.configure(state="normal")
        self.logs_textbox.delete("0.0", "end")
        self.logs_textbox.configure(state="disabled")
