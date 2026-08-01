"""
Smoke-тест gRPC-з'єднання з тестовим API ПРРО ДПС України (Фаза 0.2–0.3).

Перевіряє:
  1. Створення TLS-каналу до cabinet.tax.gov.ua:9443 (тестове API).
  2. Живість каналу (get_state / try_to_connect).
  3. Відправку ping (local_number=0x7FFFFFFF, check_type=SERVICECHK).
  4. Що сервер відповідає навіть без реєстрації ПРРО (статус != OK,
     але канал живий — це нормально, бо ключі ще не налаштовано).

Результат записується у docs/prro_phase0_ping.md.

Запуск:
    ./venv/bin/python -m app.infrastructure.services.prro.smoke_test
"""

from __future__ import annotations

import asyncio
import sys
from pathlib import Path

import grpc
from grpc import aio

from app.config import settings
from app.infrastructure.services.prro import prro_pb2
from app.infrastructure.services.prro.grpc_client import PrroGrpcClient


def _status_name(status: int) -> str:
    """Повертає назву статусу CheckResponse за його числовим значенням."""
    for name, value in prro_pb2.CheckResponse.Status.items():
        if value == status:
            return name
    return "UNKNOWN"


def _log(line: str = "") -> None:
    """Друкує рядок у консоль та накопичує для Markdown-звіту."""
    print(line)
    _log.lines.append(line)  # type: ignore[attr-defined]


_log.lines = []  # type: ignore[attr-defined]


async def run_smoke_test() -> bool:
    """Виконує smoke-тест та повертає True, якщо канал живий."""
    target = settings.PRRO_URL
    _log("=" * 64)
    _log("PRRO Фаза 0.2–0.3: Smoke-тест gRPC-з'єднання з тестовим API")
    _log("=" * 64)
    _log(f"1. Цільовий сервер: {target} (mode={settings.PRRO_MODE}, ssl={settings.PRRO_USE_SSL})")

    # ── 1. Створення TLS-каналу ────────────────────────────────────────────
    if settings.PRRO_USE_SSL:
        creds = grpc.ssl_channel_credentials()
        channel = aio.secure_channel(target, creds)
        _log("   ✅ TLS-канал створено (grpc.ssl_channel_credentials)")
    else:
        channel = aio.insecure_channel(target)
        _log("   ⚠  Канал без TLS (insecure)")

    try:
        state = channel.get_state(try_to_connect=True)
        _log(f"   ✅ Стан каналу (get_state): {state}")
    except Exception as exc:  # noqa: BLE001
        _log(f"   ⚠  get_state: {exc}")

    client = PrroGrpcClient(channel, rro_fn="0000000000")

    # ── 2. Перевірка живого з'єднання ─────────────────────────────────────
    _log("\n2. Перевірка встановлення TLS-з'єднання (до 15 сек)...")
    connected = False
    for _ in range(30):
        state = channel.get_state(try_to_connect=True)
        if state == grpc.ChannelConnectivity.READY:
            _log("   ✅ Канал у стані READY — TLS-з'єднання ВСТАНОВЛЕНО")
            connected = True
            break
        await asyncio.sleep(0.5)
    if not connected:
        _log(f"   ⚠  Канал у стані {channel.get_state(try_to_connect=True)} (через 15 сек)")
        _log("   → Це може бути нормою, якщо сервер повільно приймає TLS-хендшейк.")

    # ── 3. Відправка ping ──────────────────────────────────────────────────
    _log("\n3. Відправка ping (local_number=0x7FFFFFFF, check_type=SERVICECHK)...")
    try:
        resp = await client.ping(timeout=15.0)
        _log("   ✅ Отримано відповідь від сервера!")
        _log(f"   id           = {resp.id!r}")
        _log(f"   status       = {resp.status} ({_status_name(resp.status)})")
        _log(f"   error_message= {resp.error_message!r}")
        _log(f"   id_sign_len  = {len(resp.id_sign)}")
        _log(f"   data_sign_len= {len(resp.data_sign)}")

        if resp.status == prro_pb2.CheckResponse.OK:
            _log("   → Статус OK — сервер доступний, канал живий ✅")
        else:
            _log("   → Статус != OK (очікувано: ПРРО ще не зареєстровано, ключі не налаштовані)")
            _log("   → Але gRPC-канал ЖИВИЙ — TLS-з'єднання працює ✅")
    except grpc.RpcError as exc:
        code = exc.code()
        details = exc.details()
        _log(f"   ❌ gRPC помилка: code={code}, details={details}")
        if code == grpc.StatusCode.UNAVAILABLE:
            _log("   → Сервер недоступний. Перевірте мережу/фаєрвол/адресу.")
            return False
        if code == grpc.StatusCode.DEADLINE_EXCEEDED:
            _log("   → Таймаут. Можливо, TLS-з'єднання не встановлено.")
            return False
        # Інші коди (наприклад, UNIMPLEMENTED) все одно означають,
        # що TLS-канал до сервера працює.
        _log("   → Канал досяг сервера (помилка на рівні застосунку, не мережі) — канал ЖИВИЙ ✅")
    except Exception as exc:  # noqa: BLE001
        _log(f"   ❌ Невідома помилка: {type(exc).__name__}: {exc}")
        return False

    await channel.close()
    _log("\n" + "=" * 64)
    _log("SMOKE-ТЕСТ ЗАВЕРШЕНО")
    _log("=" * 64)
    return True


def write_report(success: bool) -> None:
    """Записує результат у docs/prro_phase0_ping.md."""
    repo_root = Path(__file__).resolve().parents[5]  # .../kasa
    docs_dir = repo_root / "docs"
    docs_dir.mkdir(parents=True, exist_ok=True)
    report_path = docs_dir / "prro_phase0_ping.md"

    lines = _log.lines  # type: ignore[attr-defined]
    verdict = "✅ УСПІХ — TLS-канал до тестового API ПРРО встановлюється" if success else "❌ НЕВДАЧА"
    content = [
        "# PRRO Фаза 0.2–0.3 — Smoke-тест gRPC-з'єднання",
        "",
        f"- **Дата:** {__import__('datetime').datetime.now().strftime('%Y-%m-%d %H:%M:%S')}",
        f"- **Результат:** {verdict}",
        f"- **Сервер:** {settings.PRRO_URL} (mode={settings.PRRO_MODE}, ssl={settings.PRRO_USE_SSL})",
        f"- **grpcio:** {grpc.__version__}",
        "",
        "## Вивід тесту",
        "",
        "```text",
    ]
    content.extend(lines)
    content.extend(["```", ""])

    report_path.write_text("\n".join(content), encoding="utf-8")
    _log(f"\n📄 Звіт записано: {report_path}")


async def main() -> int:
    """Головна функція smoke-тесту."""
    success = await run_smoke_test()
    write_report(success)
    return 0 if success else 1


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
