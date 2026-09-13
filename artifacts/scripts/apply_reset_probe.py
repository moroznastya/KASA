import io

BASE = "frontend/src-tauri/crates/torgashka-infrastructure/src/"
CTX = BASE + "store_ctx.rs"
DB = BASE + "db.rs"

# ── 1. finish(): момент повернення з'єднання ────────────────────────────────
s = io.open(CTX, encoding="utf-8").read()
old = """    pub async fn finish(self) -> Result<(), Error> {
        self.tx"""
new = """    pub async fn finish(self) -> Result<(), Error> {
        crate::embedded_pg::pg_log("PROBE2", "finish(): дропаю з'єднання запиту");
        self.tx"""
assert old in s
s = s.replace(old, new, 1)

# ── 2. RequestTx::drop — аварійне закриття ────────────────────────────────
old = """        if let Ok(mut g) = self.conn.lock() {
            if let Some(mut c) = g.take() {
                c.close_on_drop();
            }
        }"""
new = """        if let Ok(mut g) = self.conn.lock() {
            if let Some(mut c) = g.take() {
                crate::embedded_pg::pg_log("PROBE2", "RequestTx::drop: close_on_drop (з'єднання ще в контексті)");
                c.close_on_drop();
            }
        }"""
assert old in s
s = s.replace(old, new, 1)

# ── 3. put() у стані finished ─────────────────────────────────────────────
old = """        if self.finished.load(std::sync::atomic::Ordering::SeqCst) {
            conn.close_on_drop();
            return;
        }"""
new = """        if self.finished.load(std::sync::atomic::Ordering::SeqCst) {
            crate::embedded_pg::pg_log("PROBE2", "put(): finished=true → close_on_drop");
            conn.close_on_drop();
            return;
        }"""
assert old in s
s = s.replace(old, new, 1)

# ── 4. legacy-гілки: acquire + власний reset ──────────────────────────────
old = """            let mut conn = pool.acquire().await?;
            if let Some(ctx) = &ctx {
                set_config(&mut conn, ctx, false).await?;
            }
            // Повне (eager) виконання: fetch_many → Vec. Після цього reset."""
new = """            let mut conn = pool.acquire().await?;
            crate::embedded_pg::pg_log("PROBE2", "legacy_fetch_many: acquire (гілка без контексту запиту)");
            if let Some(ctx) = &ctx {
                set_config(&mut conn, ctx, false).await?;
            }
            // Повне (eager) виконання: fetch_many → Vec. Після цього reset."""
assert old in s
s = s.replace(old, new, 1)

old = """        let mut conn = pool.acquire().await?;
        if let Some(ctx) = &ctx {
            set_config(&mut conn, ctx, false).await?;
        }
        let result = (&mut *conn).fetch_optional(query).await;"""
new = """        let mut conn = pool.acquire().await?;
        crate::embedded_pg::pg_log("PROBE2", "legacy_fetch_optional: acquire (гілка без контексту запиту)");
        if let Some(ctx) = &ctx {
            set_config(&mut conn, ctx, false).await?;
        }
        let result = (&mut *conn).fetch_optional(query).await;"""
assert old in s
s = s.replace(old, new, 1)
io.open(CTX, "w", encoding="utf-8").write(s)

# ── 5. хук пула: лічильник release-ів ────────────────────────────────────
d = io.open(DB, encoding="utf-8").read()
old = """        .after_release(|conn, _meta| {
            Box::pin(async move {
                match crate::store_ctx::reset_store_ctx(conn).await {"""
new = """        .after_release(|conn, _meta| {
            Box::pin(async move {
                {
                    use std::sync::atomic::{AtomicU64, Ordering};
                    static N: AtomicU64 = AtomicU64::new(0);
                    let n = N.fetch_add(1, Ordering::SeqCst) + 1;
                    crate::embedded_pg::pg_log(
                        "PROBE2",
                        &format!(
                            "after_release #{n}: пропозиція скинути контекст (ctx у таску={})",
                            crate::store_ctx::current_store_ctx().is_some()
                        ),
                    );
                }
                match crate::store_ctx::reset_store_ctx(conn).await {"""
assert old in d
d = d.replace(old, new, 1)
io.open(DB, "w", encoding="utf-8").write(d)
print("probes installed")
