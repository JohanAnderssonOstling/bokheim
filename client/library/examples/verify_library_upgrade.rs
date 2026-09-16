//! Open a disposable database copy using the production schema initialization.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("usage: verify_library_upgrade DATABASE_COPY")?;
    let conn = rusqlite::Connection::open(path)?;
    client_library::schema::init_db(&conn)?;
    let mut query = conn.prepare(include_str!("sql/verify_library_upgrade/main_select.sql"))?;
    let mut books = 0;
    for blob in query.query_map([], |r| r.get::<_, Vec<u8>>(0))? {
        let blob = blob?;
        if !blob.is_empty() {
            let _: library_model::PublicationMetadata = sync_common::wire::decode(&blob, usize::MAX)?;
        }
        books += 1;
    }
    let pending = client_library::sync_state::get_pending_mutations(&conn)?;
    let check: String = conn.query_row(include_str!("sql/verify_library_upgrade/main_pragma.sql"), [], |r| r.get(0))?;
    if check != "ok" { return Err(check.into()); }
    if conn.prepare(include_str!("sql/verify_library_upgrade/main_pragma_2.sql"))?.exists([])? { return Err("foreign key violations".into()); }
    println!("schema initialized; {books} book metadata records and {} pending mutations decoded; integrity checks passed", pending.len());
    Ok(())
}
