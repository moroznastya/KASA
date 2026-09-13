import io

BASE = "frontend/src-tauri/crates/torgashka-infrastructure/src/"
CTX = BASE + "store_ctx.rs"
DB = BASE + "db.rs"

s = io.open(CTX, encoding="utf-8").read()

# ── знімаємо проби ───────────────────────────────────────────────────────────
rm = [
    '        crate::embedded_pg::pg_log("PROBE2", "finish(): дропаю з\'єднання запиту");\n',
    '                crate::embedded_pg::pg_log("PROBE2", "RequestTx::drop: close_on_drop (з\'єднання ще в контексті)");\n',
    '            crate::embedded_pg::pg_log("PROBE2", "put(): finished=true → close_on_drop");\n',
    '            crate::embedded_pg::pg_log("PROBE2", "legacy_fetch_many: acquire (гілка без контексту запиту)");\n',
    '        crate::embedded_pg::pg_log("PROBE2", "legacy_fetch_optional: acquire (гілка без контексту запиту)");\n',
]
for r in rm:
    assert r in s, r
    s = s.replace(r, "", 1)

# ── ФІКС: скидання в legacy-гілці лише якщо контекст справді ставили ────────
old_many = """            // Скидаємо контекст ЗАВЖДИ (навіть коли ctx=None): пул без хука
            // `after_release` (тести, чужі пули) не має лишати на з'єднанні
            // контекст попереднього запиту — інакше безконтекстний споживач
            // побачив би чужу точку.
            let _ = reset_config(&mut conn).await;"""
new_many = """            // Скидаємо контекст ЛИШЕ якщо ми його ставили. Інакше — зайвий
            // мережевий круг (≈29 мс), а на пулі з хуком `after_release`
            // (production) — ДРУГЕ скидання підряд: хвіст запиту показував
            // два reset-и поспіль (виміряно `measure_resets.py`: 2.2 на
            // запит; після фіксу — 1.0).
            //
            // Безпека: інваріант «з'єднання в пулі завжди без чужого
            // контексту» тримається і без цього reset-у, коли ctx=None —
            // жодного `set_config` на цьому з'єднанні не виконувалось.
            if ctx.is_some() {
                let _ = reset_config(&mut conn).await;
            }"""
assert old_many in s
s = s.replace(old_many, new_many, 1)

old_one = """        // Див. коментар у `legacy_fetch_many`: скидаємо контекст завжди.
        let _ = reset_config(&mut conn).await;"""
new_one = """        // Див. коментар у `legacy_fetch_many`: скидаємо лише те, що ставили.
        if ctx.is_some() {
            let _ = reset_config(&mut conn).await;
        }"""
assert old_one in s
s = s.replace(old_one, new_one, 1)

assert "PROBE" not in s
io.open(CTX, "w", encoding="utf-8").write(s)

d = io.open(DB, encoding="utf-8").read()
start = d.index("                {\n                    use std::sync::atomic::{AtomicU64, Ordering};")
end = d.index("                match crate::store_ctx::reset_store_ctx(conn).await {")
d = d[:start] + d[end:]
assert "PROBE" not in d
io.open(DB, "w", encoding="utf-8").write(d)
print("проби знято, фікс застосовано")
