"""
NIKO ChatArea — центральна область чату.
Балонні повідомлення (користувач справа, агенти зліва),
поле вводу з Shift+Enter/Enter, кнопка відправки, індикатор набору тексту.
"""

import customtkinter as ctk
from datetime import datetime
from ..styles import get_color, get_font


class MessageBubble(ctk.CTkFrame):
    """
    Окремий балон повідомлення.
    Користувач — справа з синім фоном.
    Агент — зліва з сірим фоном.
    """

    def __init__(self, parent, sender: str, text: str, is_user: bool = False,
                 agent_color: str = None, timestamp: str = None, **kwargs):
        super().__init__(parent, fg_color="transparent", **kwargs)

        self._is_user = is_user
        self._agent_color = agent_color or get_color("accent")

        anchor = "e" if is_user else "w"
        bubble_color = get_color("user_bubble") if is_user else get_color("agent_bubble")
        text_color = "white" if is_user else get_color("text")

        self.bubble_frame = ctk.CTkFrame(
            self,
            fg_color=bubble_color,
            corner_radius=12,
        )

        sender_name = "Ви" if is_user else sender
        sender_color = self._agent_color if not is_user else "rgba(255,255,255,0.8)"

        self.sender_label = ctk.CTkLabel(
            self.bubble_frame,
            text=sender_name,
            font=get_font("small_bold"),
            text_color=sender_color,
            anchor=anchor,
        )
        self.sender_label.pack(fill="x", padx=12, pady=(8, 2))

        self.text_label = ctk.CTkLabel(
            self.bubble_frame,
            text=text,
            font=get_font("body"),
            text_color=text_color,
            anchor=anchor,
            justify="left",
            wraplength=500,
        )
        self.text_label.pack(fill="x", padx=12, pady=(0, 4))

        ts = timestamp or datetime.now().strftime("%H:%M:%S")
        self.time_label = ctk.CTkLabel(
            self.bubble_frame,
            text=ts,
            font=get_font("tiny"),
            text_color=get_color("text_secondary"),
            anchor=anchor,
        )
        self.time_label.pack(fill="x", padx=12, pady=(0, 6))

        self.bubble_frame.pack(
            anchor=anchor,
            padx=(50 if is_user else 10, 10 if is_user else 50),
            pady=4,
            fill="x",
        )


class TypingIndicator(ctk.CTkFrame):
    """Індикатор набору тексту агентом."""

    def __init__(self, parent, **kwargs):
        super().__init__(parent, fg_color="transparent", **kwargs)

        self.dot_label = ctk.CTkLabel(
            self,
            text="\xe2\x97\x8f \xe2\x97\x8f \xe2\x97\x8f",
            font=get_font("body"),
            text_color=get_color("text_secondary"),
        )
        self.dot_label.pack(side="left", padx=(10, 5))

        self.text_label = ctk.CTkLabel(
            self,
            text="агент друкує...",
            font=get_font("small"),
            text_color=get_color("text_secondary"),
        )
        self.text_label.pack(side="left")


class ChatArea(ctk.CTkFrame):
    """
    Центральна область чату.
    Містить: список повідомлень, поле вводу, кнопку відправки, індикатор набору.
    """

    def __init__(self, parent, on_send=None, **kwargs):
        super().__init__(parent, **kwargs)
        self._on_send = on_send
        self._messages = []
        self._typing_widget = None
        self._build_ui()

    def _build_ui(self):
        """Створює інтерфейс області чату."""
        self.messages_frame = ctk.CTkScrollableFrame(
            self,
            fg_color=get_color("chat_bg"),
            scrollbar_button_color=get_color("scrollbar"),
            scrollbar_button_hover_color=get_color("hover"),
        )
        self.messages_frame.pack(fill="both", expand=True, padx=0, pady=0)

        self.bottom_frame = ctk.CTkFrame(self, fg_color="transparent")
        self.bottom_frame.pack(fill="x", padx=10, pady=(0, 10))

        self.typing_indicator = TypingIndicator(self.bottom_frame)

        self.input_frame = ctk.CTkFrame(
            self.bottom_frame,
            fg_color=get_color("input_bg"),
            border_width=1,
            border_color=get_color("border"),
            corner_radius=8,
        )
        self.input_frame.pack(fill="x", pady=(5, 0))

        self.input_textbox = ctk.CTkTextbox(
            self.input_frame,
            font=get_font("body"),
            fg_color="transparent",
            text_color=get_color("text"),
            border_width=0,
            height=40,
            wrap="word",
        )
        self.input_textbox.pack(side="left", fill="both", expand=True, padx=(10, 5), pady=8)
        self.input_textbox.bind("<Return>", self._on_enter)
        self.input_textbox.bind("<Shift-Return>", self._on_shift_enter)
        self.input_textbox.bind("<KeyRelease>", self._on_key_release)

        self.send_button = ctk.CTkButton(
            self.input_frame,
            text="\xe2\x9e\xa4",
            font=get_font("body"),
            fg_color=get_color("accent"),
            hover_color=get_color("hover"),
            text_color="white",
            width=36,
            height=36,
            corner_radius=6,
            command=self._send_message,
        )
        self.send_button.pack(side="right", padx=(5, 8), pady=6)

    def _on_enter(self, event):
        self._send_message()
        return "break"

    def _on_shift_enter(self, event):
        self.input_textbox.insert("insert", "\n")
        return "break"

    def _on_key_release(self, event):
        line_count = int(self.input_textbox.index("end-1c").split(".")[0])
        new_height = min(max(40, line_count * 20), 120)
        self.input_textbox.configure(height=new_height)

    def _send_message(self):
        text = self.input_textbox.get("0.0", "end-1c").strip()
        if not text:
            return
        self.input_textbox.delete("0.0", "end")
        self.input_textbox.configure(height=40)
        if self._on_send:
            self._on_send(text)

    def add_message(self, sender: str, text: str, is_user: bool = False,
                    agent_color: str = None, timestamp: str = None):
        if not is_user:
            self.hide_typing()

        bubble = MessageBubble(
            self.messages_frame,
            sender=sender,
            text=text,
            is_user=is_user,
            agent_color=agent_color,
            timestamp=timestamp,
        )
        bubble.pack(fill="x", pady=2)

        msg = {
            "sender": sender,
            "text": text,
            "is_user": is_user,
            "timestamp": timestamp or datetime.now().strftime("%H:%M:%S"),
            "agent_color": agent_color or get_color("accent"),
        }
        self._messages.append(msg)
        self.messages_frame._parent_canvas.yview_moveto(1.0)

    def show_typing(self, agent_name: str = "агент"):
        if self._typing_widget is None:
            self._typing_widget = TypingIndicator(self.bottom_frame)
        self._typing_widget.text_label.configure(text=f"{agent_name} друкує...")
        self._typing_widget.pack(fill="x", before=self.input_frame, pady=(0, 2))

    def hide_typing(self):
        if self._typing_widget is not None:
            self._typing_widget.pack_forget()

    def clear(self):
        for widget in self.messages_frame.winfo_children():
            widget.destroy()
        self._messages = []
        self.hide_typing()

    def get_history(self) -> list:
        return self._messages.copy()

    def load_history(self, messages: list):
        self.clear()
        for msg in messages:
            self.add_message(
                sender=msg.get("sender", "Невідомо"),
                text=msg.get("text", ""),
                is_user=msg.get("is_user", False),
                agent_color=msg.get("agent_color"),
                timestamp=msg.get("timestamp"),
            )
