import io

p = "frontend/src-tauri/crates/torgashka-infrastructure/src/store_ctx.rs"
s = io.open(p, encoding="utf-8").read()

old_reset = """pub async fn reset_store_ctx(conn: &mut sqlx::PgConnection) -> Result<(), Error> {
    reset_config(conn).await
}"""
new_reset = """pub async fn reset_store_ctx(conn: &mut sqlx::PgConnection) -> Result<(), Error> {
    crate::embedded_pg::pg_log(
        "PROBE",
        &format!("RESET-hook: req_ctx={}", current_request_tx().is_some()),
    );
    reset_config(conn).await
}"""
assert old_reset in s
s = s.replace(old_reset, new_reset, 1)

old_fin = """    pub async fn finish(self) -> Result<(), Error> {
        self.tx"""
new_fin = """    pub async fn finish(self) -> Result<(), Error> {
        crate::embedded_pg::pg_log("PROBE", "FINISH: контекст запиту закрито");
        self.tx"""
assert old_fin in s
s = s.replace(old_fin, new_fin, 1)

old_open = """        let allowed = row.2;
        Ok(("""
new_open = """        let allowed = row.2;
        crate::embedded_pg::pg_log("PROBE", "OPEN: контекст відкрито");
        Ok(("""
assert old_open in s
s = s.replace(old_open, new_open, 1)

io.open(p, "w", encoding="utf-8").write(s)
print("PROBE markers:", s.count("PROBE"))
