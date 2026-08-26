use rusqlite::functions::FunctionFlags;
use rusqlite::{named_params, Connection, OpenFlags};
use std::hint::black_box;
use std::path::Path;
use std::time::{Duration, Instant};

const ROOT_DIR_ID: &str = "00000000-0000-0000-0000-000000000000";
const BROWSING_SQL: &str = include_str!("../../library-sqlite/src/database/sql/browsing.sql");

#[derive(Clone, Copy)]
struct Measurement {
    elapsed: Duration,
    rows: usize,
}

fn named_query(name: &str) -> &'static str {
    let marker = format!("-- name: {name}?");
    let tail = BROWSING_SQL.split_once(&marker).unwrap_or_else(|| panic!("missing query {name}")).1;
    tail.split("\n-- name: ").next().expect("query body").trim()
}

fn open_database(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    conn.busy_timeout(Duration::from_secs(10))?;
    conn.create_scalar_function("normalize_search_text", 1, FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC, |context| Ok(library_model::normalize_search_text(&context.get::<String>(0)?)))?;
    Ok(conn)
}

fn child_rows(conn: &Connection, folder_id: &str, normalized_query: &str) -> rusqlite::Result<Measurement> {
    let started = Instant::now();
    let query_name = if normalized_query.is_empty() { "get_immediate_sub_dirs" } else { "get_sub_dirs" };
    let mut statement = conn.prepare(named_query(query_name))?;
    let mut rows = if normalized_query.is_empty() {
        statement.query(named_params! {
            ":dir_id": folder_id,
            ":chip_sort": 0,
        })?
    } else {
        statement.query(named_params! {
            ":dir_id": folder_id,
            ":normalized_query": normalized_query,
            ":file_types": 0,
            ":chip_sort": 0,
            ":languages": "",
        })?
    };
    let mut count = 0;
    while let Some(row) = rows.next()? {
        black_box((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?, row.get::<_, Option<String>>(4)?));
        count += 1;
    }
    Ok(Measurement { elapsed: started.elapsed(), rows: count })
}

fn book_rows(conn: &Connection, folder_id: &str, normalized_query: &str) -> rusqlite::Result<Measurement> {
    let started = Instant::now();
    let mut statement = conn.prepare(named_query("get_book_cards_in_dir"))?;
    let mut rows = statement.query(named_params! {
        ":dir_id": folder_id,
        ":downloaded_only": 0,
        ":normalized_query": normalized_query,
        ":file_types": 0,
        ":book_sort": 0,
        ":languages": "",
    })?;
    let mut count = 0;
    while let Some(row) = rows.next()? {
        black_box((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<f32>>(4)?,
            row.get::<_, bool>(5)?,
            row.get::<_, bool>(6)?,
            row.get::<_, bool>(7)?,
            row.get::<_, Option<i64>>(8)?,
            row.get::<_, Option<i64>>(9)?,
            row.get::<_, i64>(10)?,
            row.get::<_, Option<String>>(11)?,
        ));
        count += 1;
    }
    Ok(Measurement { elapsed: started.elapsed(), rows: count })
}

fn combined(conn: &Connection, folder_id: &str, normalized_query: &str) -> rusqlite::Result<Measurement> {
    let started = Instant::now();
    let children = child_rows(conn, folder_id, normalized_query)?;
    let books = book_rows(conn, folder_id, normalized_query)?;
    Ok(Measurement { elapsed: started.elapsed(), rows: children.rows + books.rows })
}

fn percentile(sorted: &[Duration], fraction: f64) -> Duration {
    let index = ((sorted.len() - 1) as f64 * fraction).ceil() as usize;
    sorted[index]
}

fn benchmark(label: &str, iterations: usize, mut operation: impl FnMut() -> rusqlite::Result<Measurement>) -> rusqlite::Result<()> {
    for _ in 0..10 {
        black_box(operation()?);
    }
    let mut samples = Vec::with_capacity(iterations);
    let mut rows = 0;
    for _ in 0..iterations {
        let measurement = operation()?;
        rows = measurement.rows;
        samples.push(measurement.elapsed);
    }
    samples.sort_unstable();
    println!(
        "{label:<24} rows={rows:<5} min={:>8.3} ms median={:>8.3} ms p95={:>8.3} ms max={:>8.3} ms",
        samples[0].as_secs_f64() * 1_000.0,
        percentile(&samples, 0.50).as_secs_f64() * 1_000.0,
        percentile(&samples, 0.95).as_secs_f64() * 1_000.0,
        samples[samples.len() - 1].as_secs_f64() * 1_000.0,
    );
    Ok(())
}

fn print_child_plan(conn: &Connection, folder_id: &str) -> rusqlite::Result<()> {
    let sql = format!("EXPLAIN QUERY PLAN {}", named_query("get_immediate_sub_dirs"));
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(
        named_params! {
            ":dir_id": folder_id,
            ":chip_sort": 0,
        },
        |row| row.get::<_, String>(3),
    )?;
    println!("\nroot children query plan:");
    for row in rows {
        println!("  {}", row?);
    }
    Ok(())
}

fn print_book_plan(conn: &Connection, folder_id: &str) -> rusqlite::Result<()> {
    let sql = format!("EXPLAIN QUERY PLAN {}", named_query("get_book_cards_in_dir"));
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(
        named_params! {
            ":dir_id": folder_id,
            ":downloaded_only": 0,
            ":normalized_query": "",
            ":file_types": 0,
            ":book_sort": 0,
            ":languages": "",
        },
        |row| row.get::<_, String>(3),
    )?;
    println!("\nlargest-folder books query plan:");
    for row in rows {
        println!("  {}", row?);
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let database = arguments.next().ok_or("usage: folder_query_benchmark <library.db> [iterations] [--plan]")?;
    let iterations = arguments.next().map(|value| value.to_string_lossy().parse()).transpose()?.unwrap_or(100);
    let show_plan = arguments.next().is_some_and(|value| value == "--plan");
    if iterations == 0 {
        return Err("iterations must be greater than zero".into());
    }
    let conn = open_database(Path::new(&database))?;
    let (books, placements, directories): (i64, i64, i64) = conn.query_row(
        "SELECT
            (SELECT COUNT(*) FROM book WHERE deleted_at IS NULL AND hidden_at IS NULL),
            (SELECT COUNT(*) FROM book_dir WHERE deleted_at IS NULL),
            (SELECT COUNT(*) FROM dir WHERE deleted_at IS NULL)",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    let (folder_id, folder_name, direct_books): (String, String, i64) = conn.query_row(
        "SELECT directory.id,directory.name,COUNT(placement.book_row_id)
         FROM dir AS directory
         LEFT JOIN book_dir AS placement ON placement.dir_id=directory.id AND placement.deleted_at IS NULL
         WHERE directory.deleted_at IS NULL
         GROUP BY directory.id,directory.name
         ORDER BY COUNT(placement.book_row_id) DESC,directory.id
         LIMIT 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;

    println!("database={} books={books} placements={placements} directories={directories} iterations={iterations}", Path::new(&database).display());
    println!("largest direct folder={folder_name:?} id={folder_id} direct_books={direct_books}");
    benchmark("root children", iterations, || child_rows(&conn, ROOT_DIR_ID, ""))?;
    benchmark("largest children", iterations, || child_rows(&conn, &folder_id, ""))?;
    benchmark("largest books", iterations, || book_rows(&conn, &folder_id, ""))?;
    benchmark("largest combined", iterations, || combined(&conn, &folder_id, ""))?;
    benchmark("root search children", iterations, || child_rows(&conn, ROOT_DIR_ID, "a"))?;
    benchmark("root search books", iterations, || book_rows(&conn, ROOT_DIR_ID, "a"))?;
    benchmark("root search combined", iterations, || combined(&conn, ROOT_DIR_ID, "a"))?;
    if show_plan {
        print_child_plan(&conn, ROOT_DIR_ID)?;
        print_book_plan(&conn, &folder_id)?;
    }
    Ok(())
}
