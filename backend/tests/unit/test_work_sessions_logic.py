"""
Unit tests: логіка робочих сесій (WorkSession).

Перевіряємо:
  - «живу» тривалість активної сесії (з порогом MAX_SESSION_HOURS);
  - закриття попередніх активних сесій при новому login (users.py);
  - формування відповіді GET /my (is_active + total_hours).
"""

from __future__ import annotations

from datetime import datetime, timedelta
from unittest.mock import AsyncMock, MagicMock
from uuid import uuid4

from app.api.v1.users import _close_active_work_sessions
from app.api.v1.work_sessions import (
    MAX_SESSION_HOURS,
    _effective_duration,
    _month_bounds,
)
from app.infrastructure.persistence.models.work_session import WorkSession
from app.schemas.work_session import WorkSessionResponse


def _make_session(*, login_time, logout_time=None, duration_hours=None) -> WorkSession:
    ws = WorkSession()
    ws.id = uuid4()
    ws.user_id = uuid4()
    ws.login_time = login_time
    ws.logout_time = logout_time
    ws.duration_hours = duration_hours
    return ws


class TestEffectiveDuration:
    """«Жива» тривалість активної сесії (без запису в БД)."""

    def test_closed_session_uses_db_value(self):
        ws = _make_session(
            login_time=datetime(2026, 8, 1, 9, 0),
            logout_time=datetime(2026, 8, 1, 13, 30),
            duration_hours=4.5,
        )
        assert _effective_duration(ws) == 4.5

    def test_closed_session_null_duration_returns_zero(self):
        ws = _make_session(
            login_time=datetime(2026, 8, 1, 9, 0),
            logout_time=datetime(2026, 8, 1, 10, 0),
            duration_hours=None,
        )
        assert _effective_duration(ws) == 0.0

    def test_active_session_live_duration(self):
        """Активна сесія: тривалість = now - login_time (менше порогу)."""
        now = datetime(2026, 8, 2, 12, 0)
        ws = _make_session(login_time=now - timedelta(hours=2, minutes=30))
        assert _effective_duration(ws, now) == 2.5

    def test_active_session_capped_at_max(self):
        """Активна сесія старша за MAX_SESSION_HOURS → обрізається порогом."""
        now = datetime(2026, 8, 10, 12, 0)
        ws = _make_session(login_time=now - timedelta(hours=50))
        assert _effective_duration(ws, now) == float(MAX_SESSION_HOURS)

    def test_active_session_exactly_at_max_not_capped(self):
        now = datetime(2026, 8, 2, 12, 0)
        ws = _make_session(login_time=now - timedelta(hours=MAX_SESSION_HOURS))
        assert _effective_duration(ws, now) == float(MAX_SESSION_HOURS)

    def test_month_bounds(self):
        start, end = _month_bounds(12, 2026)
        assert start == datetime(2026, 12, 1)
        assert end == datetime(2027, 1, 1)


class TestCloseActiveWorkSessions:
    """users.py: перед новим login закриваються всі активні сесії."""

    def _build(self, active_sessions: list[WorkSession], user_id):
        session = AsyncMock()
        result = MagicMock()
        result.scalars.return_value.all.return_value = active_sessions
        session.execute = AsyncMock(return_value=result)
        return session

    async def test_all_active_sessions_closed(self):
        user_id = uuid4()
        now_ref = datetime.utcnow()
        old_1 = _make_session(login_time=now_ref - timedelta(hours=2))
        old_2 = _make_session(login_time=now_ref - timedelta(hours=5))
        session = self._build([old_1, old_2], user_id)

        await _close_active_work_sessions(session, user_id)

        assert old_1.logout_time is not None
        assert old_2.logout_time is not None
        # duration_hours розраховано і заокруглено до 2 знаків
        assert old_1.duration_hours == round((old_1.logout_time - old_1.login_time).total_seconds() / 3600, 2)
        assert old_2.duration_hours == round((old_2.logout_time - old_2.login_time).total_seconds() / 3600, 2)
        assert old_1.logout_time == old_2.logout_time  # один момент часу

    async def test_no_active_sessions_no_crash(self):
        session = self._build([], uuid4())
        await _close_active_work_sessions(session, uuid4())  # не падає


class TestWorkSessionResponseSchema:
    """Схема WorkSessionResponse містить is_active."""

    def test_is_active_default_false(self):
        ws = _make_session(login_time=datetime(2026, 8, 1, 9, 0))
        resp = WorkSessionResponse.model_validate(ws)
        assert resp.is_active is False

    def test_is_active_set_manually(self):
        ws = _make_session(login_time=datetime(2026, 8, 1, 9, 0))
        resp = WorkSessionResponse.model_validate(ws)
        resp.is_active = True
        resp.duration_hours = 3.25
        assert resp.is_active is True
        assert resp.duration_hours == 3.25
