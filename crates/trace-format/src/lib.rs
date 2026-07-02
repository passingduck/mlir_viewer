pub mod schema;

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    #[test]
    fn schema_applies_cleanly() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(crate::schema::SCHEMA_SQL).unwrap();
        // All three core tables exist.
        for table in ["meta", "ir_blob", "pass_execution"] {
            let n: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "missing table {table}");
        }
        assert_eq!(crate::schema::FORMAT_VERSION, "1");
    }
}
