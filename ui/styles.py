"""
NIKO GUI Styles — кольори, шрифти, теми.
"""

COLORS = {
    "dark": {
        "bg": "#1a1a2e",
        "sidebar": "#16213e",
        "chat_bg": "#0f0f23",
        "user_bubble": "#0d6efd",
        "agent_bubble": "#2d2d44",
        "text": "#e0e0e0",
        "text_secondary": "#a0a0b0",
        "accent": "#5285ec",
        "success": "#28a745",
        "warning": "#ffc107",
        "danger": "#dc3545",
        "border": "#2a2a4a",
        "input_bg": "#1e1e3a",
        "hover": "#2a2a5a",
        "scrollbar": "#3a3a5a",
    },
    "light": {
        "bg": "#f8f9fa",
        "sidebar": "#e9ecef",
        "chat_bg": "#ffffff",
        "user_bubble": "#0d6efd",
        "agent_bubble": "#f0f0f0",
        "text": "#212529",
        "text_secondary": "#6c757d",
        "accent": "#0d6efd",
        "success": "#28a745",
        "warning": "#ffc107",
        "danger": "#dc3545",
        "border": "#dee2e6",
        "input_bg": "#ffffff",
        "hover": "#e2e6ea",
        "scrollbar": "#ced4da",
    },
}

FONTS = {
    "heading": ("Ubuntu", 20, "bold"),
    "subheading": ("Ubuntu", 14),
    "body": ("Ubuntu", 13),
    "body_bold": ("Ubuntu", 13, "bold"),
    "mono": ("Ubuntu Mono", 13),
    "small": ("Ubuntu", 11),
    "small_bold": ("Ubuntu", 11, "bold"),
    "tiny": ("Ubuntu", 10),
}

_current_theme = "dark"


def get_color(name: str) -> str:
    return COLORS[_current_theme][name]


def get_font(name: str) -> tuple:
    return FONTS[name]


def set_theme(theme: str) -> None:
    global _current_theme
    if theme in COLORS:
        _current_theme = theme


def get_theme() -> str:
    return _current_theme
