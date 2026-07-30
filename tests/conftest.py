"""Root conftest — додає backend/ до PYTHONPATH."""

import sys
from pathlib import Path

# Додаємо backend/ до шляху імпорту
BACKEND_DIR = Path(__file__).parent.parent / "backend"
sys.path.insert(0, str(BACKEND_DIR))
