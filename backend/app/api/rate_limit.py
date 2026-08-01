"""
Спільний slowapi Limiter для auth-ендпоінтів.

Використовується:
  - app/api/v1/users.py — POST /auth/login, POST /auth/login-pin
  - app/api/v2/auth.py   — POST /auth/login, POST /auth/login-pin, POST /auth/refresh
  - app/main.py          — app.state.limiter + SlowAPIMiddleware

Єдиний екземпляр Limiter гарантує, що rate-limit не можна обійти,
викликаючи дубльовані ендпоінти v1/v2 (brute-force через v2).
"""

from slowapi import Limiter
from slowapi.util import get_remote_address

# Rate limiter для auth ендпоінтів (5 запитів на хвилину на IP)
limiter = Limiter(key_func=get_remote_address)
