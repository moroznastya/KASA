"""
NIKO GUI Utils — допоміжні функції для UI.
"""

import json
import os
from datetime import datetime
from pathlib import Path

# Шлях до файлу історії чату
HISTORY_DIR = Path(__file__).parent / "history"
HISTORY_FILE = HISTORY_DIR / "chat_history.json"


def ensure_history_dir():
    """Створює директорію для історії, якщо її немає."""
    HISTORY_DIR.mkdir(parents=True, exist_ok=True)


def save_chat_history(messages: list) -> None:
    """Зберігає історію чату в JSON файл."""
    ensure_history_dir()
    try:
        with open(HISTORY_FILE, "w", encoding="utf-8") as f:
            json.dump(messages, f, ensure_ascii=False, indent=2)
    except Exception as e:
        print(f"[Utils] Помилка збереження історії: {e}")


def load_chat_history() -> list:
    """Завантажує історію чату з JSON файлу."""
    ensure_history_dir()
    if not HISTORY_FILE.exists():
        return []
    try:
        with open(HISTORY_FILE, "r", encoding="utf-8") as f:
            return json.load(f)
    except Exception as e:
        print(f"[Utils] Помилка завантаження історії: {e}")
        return []


def clear_chat_history() -> None:
    """Очищує файл історії чату."""
    ensure_history_dir()
    try:
        with open(HISTORY_FILE, "w", encoding="utf-8") as f:
            json.dump([], f)
    except Exception as e:
        print(f"[Utils] Помилка очищення історії: {e}")


def timestamp() -> str:
    """Повертає поточний час у форматі HH:MM:SS."""
    return datetime.now().strftime("%H:%M:%S")


def truncate_text(text: str, max_length: int = 100) -> str:
    """Обрізає текст до max_length символів, додаючи ..."""
    if len(text) <= max_length:
        return text
    return text[:max_length - 3] + "..."
