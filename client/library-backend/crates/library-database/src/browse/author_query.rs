//! Relational author selection. Only heading aggregates cross into Rust;
//! each requested author's ordered membership is loaded on demand.
use super::{BrowseOptions, BrowseRow, invalid_integer_column, parse_uuid_column, popularity};
use crate::{Database, DatabaseError};
use book_model::{AUTHOR_MARC_RELATOR_CODE, AuthorId, normalize_search_text};
use library_model::{BrowseBookSort, BrowseChipSort, BrowseLocation, FINISHED_PROGRESS_THRESHOLD};
use rusqlite::{Connection, Row, types::Value};
use std::collections::HashMap;

const LIBRARY: &str = r#"
members AS (
 SELECT DISTINCT credit.author_identity_id AS author_id,credit.book_row_id
 FROM book_contributor credit
 WHERE credit.role=:role AND trim(credit.name)!='' {member_scope}
),
names AS (
 SELECT identity.id AS author_id,identity.stable_id AS identity,min(credit.name) AS name
 FROM book_contributor credit JOIN author_identity identity ON identity.id=credit.author_identity_id
 WHERE credit.role=:role AND trim(credit.name)!='' {name_scope}
 GROUP BY identity.id
),
eligible AS (
 SELECT book.row_id,book.search_title,book.title,book.content_hash AS key,book.read_at,
        presence.downloaded,0 AS source_priority,book.popularity_rating_count AS score,
        CASE WHEN :search='' THEN 0 ELSE instr(book.search_title,:search)>0 OR instr(book.search_author,:search)>0 END AS text_match
 FROM book JOIN library_book_presence presence ON presence.book_row_id=book.row_id
 WHERE book.deleted_at IS NULL
   AND (:downloaded=0 OR presence.downloaded=1)
   AND (:formats=0 OR (:formats & book.format)!=0)
   AND (:languages='' OR EXISTS(SELECT 1 FROM book_language language
        WHERE language.book_row_id=book.row_id AND instr(:languages,',' || language.language || ',')>0))
   AND (:finished=0 OR coalesce(book.read_progress,0)<:threshold)
)
"#;

const CATALOG: &str = r#"
members AS (
 SELECT credit.author_row_id AS author_id,browse.row_id AS book_row_id
 FROM book_author credit {membership_index} JOIN catalog_browse_search browse ON browse.book_row_id=credit.book_row_id
 WHERE 1 {member_scope}
),
names AS (SELECT row_id AS author_id,author_id AS identity,name FROM author WHERE 1 {name_scope}),
eligible AS (
 SELECT browse.row_id,browse.search_title,source.title,source.key,NULL AS read_at,
        0 AS downloaded,browse.source_priority,browse.popularity_score AS score,
        CASE WHEN :search='' THEN 0 ELSE instr(browse.search_title,:search)>0 OR instr(browse.search_author,:search)>0 END AS text_match
 FROM catalog_browse_search browse JOIN book source ON source.row_id=browse.book_row_id
 WHERE (:formats=0 OR (:formats & source.format)!=0)
   AND (:languages='' OR EXISTS(SELECT 1 FROM book_language language
        WHERE language.book_row_id=source.row_id AND instr(:languages,',' || lower(CASE WHEN instr(language.language,'-')>0 THEN substr(language.language,1,instr(language.language,'-')-1) ELSE language.language END) || ',')>0))
   AND (source.external=1 OR lower(hex(source.key)) NOT IN (SELECT value FROM json_each(:held)))
)
"#;

pub(super) struct AuthorQuery<'a> {
    connection: &'a Connection,
    catalog: bool,
    options: &'a BrowseOptions,
    held: String,
    empty: bool,
}

impl Database {
    pub(super) fn author_query<'a>(&'a self, catalog: Option<&'a Connection>, options: &'a BrowseOptions) -> Result<AuthorQuery<'a>, DatabaseError> {
        let held = if catalog.is_some() { serde_json::to_string(&self.library_book_hashes()?.iter().map(ToString::to_string).collect::<Vec<_>>()).map_err(DatabaseError::operation)? } else { String::new() };
        let empty = catalog.map(|catalog| catalog.query_row("SELECT NOT EXISTS(SELECT 1 FROM catalog_browse_revision)", [], |row| row.get::<_, bool>(0))).transpose()?.unwrap_or(false);
        Ok(AuthorQuery { connection: catalog.unwrap_or(&self.connection), catalog: catalog.is_some(), options, held, empty })
    }
}

impl AuthorQuery<'_> {
    fn read(&self, sql: &str, author: Option<AuthorId>, selected: &[AuthorId], mut read: impl FnMut(&Row<'_>) -> rusqlite::Result<()>) -> Result<(), DatabaseError> {
        if self.empty {
            return Ok(());
        }
        let (member, identity, table, key) = if self.catalog { ("credit.author_row_id", "author_id", "author", "row_id") } else { ("credit.author_identity_id", "identity.stable_id", "author_identity", "id") };
        let stable = if self.catalog { "author_id" } else { "stable_id" };
        let name_scope = if author.is_some() {
            format!("AND {identity}=:author")
        } else if !selected.is_empty() {
            format!("AND {identity} IN (SELECT {} FROM json_each(:authors))", if self.catalog { "unhex(value)" } else { "value" })
        } else {
            String::new()
        };
        let member_scope = name_scope.replace(identity, &format!("{member}"));
        let member_scope = if name_scope.is_empty() {
            member_scope
        } else {
            let predicate = name_scope.replace(identity, stable).trim_start_matches("AND ").to_owned();
            format!("AND {member} IN (SELECT {key} FROM {table} WHERE {predicate})")
        };
        // A full index scan reads books repeatedly. Walk credits in book order
        // for locality; scoped requests use the reverse author membership index.
        let membership_index = if author.is_none() && selected.is_empty() { "INDEXED BY sqlite_autoindex_book_author_1" } else { "" };
        let source = (if self.catalog { CATALOG } else { LIBRARY }).replace("{member_scope}", &member_scope).replace("{name_scope}", &name_scope).replace("{membership_index}", membership_index);
        let sql = format!("WITH {source} {sql}");
        let mut statement = self.connection.prepare_cached(&sql)?;
        let author = author.map(|id| if self.catalog { Value::Blob(id.as_bytes().to_vec()) } else { Value::Text(id.to_string()) }).unwrap_or(Value::Null);
        let parameters = [
            (":author", author),
            (":authors", Value::Text(serde_json::to_string(&selected.iter().map(|id| if self.catalog { id.simple().to_string() } else { id.to_string() }).collect::<Vec<_>>()).map_err(DatabaseError::operation)?)),
            (":role", Value::Blob(AUTHOR_MARC_RELATOR_CODE.0.to_vec())),
            (":held", Value::Text(self.held.clone())),
            (":search", Value::Text(self.options.search.clone())),
            (":languages", Value::Text(self.options.languages.clone())),
            (":formats", Value::Integer(self.options.file_types.into())),
            (":downloaded", Value::Integer(self.options.downloaded_only.into())),
            (":finished", Value::Integer(self.options.hide_finished.into())),
            (":threshold", Value::Real(FINISHED_PROGRESS_THRESHOLD.into())),
        ];
        for (name, value) in &parameters {
            if let Some(index) = statement.parameter_index(name)? {
                statement.raw_bind_parameter(index, value)?;
            }
        }
        if log::log_enabled!(target: "browse_query_plan", log::Level::Debug) {
            let mut plan = self.connection.prepare_cached(&format!("EXPLAIN QUERY PLAN {sql}"))?;
            for (name, value) in &parameters {
                if let Some(index) = plan.parameter_index(name)? {
                    plan.raw_bind_parameter(index, value)?;
                }
            }
            let mut rows = plan.raw_query();
            while let Some(row) = rows.next()? {
                log::debug!(target: "browse_query_plan", "{}", row.get::<_, String>(3)?);
            }
        }
        let mut rows = statement.raw_query();
        while let Some(row) = rows.next()? {
            read(row)?;
        }
        Ok(())
    }

    fn identity(&self, row: &Row<'_>, column: usize) -> rusqlite::Result<AuthorId> {
        if self.catalog {
            let bytes: Vec<u8> = row.get(column)?;
            AuthorId::from_slice(&bytes).map_err(|error| rusqlite::Error::FromSqlConversionFailure(column, rusqlite::types::Type::Blob, Box::new(error)))
        } else {
            parse_uuid_column(row.get(column)?, column)
        }
    }

    pub(super) fn headings(&self) -> Result<Vec<BrowseRow>, DatabaseError> {
        let popularity = self.options.chip_sort == BrowseChipSort::Popularity;
        let stats = if popularity {
            r#",credited AS (
                 SELECT members.author_id,eligible.*,
                        min(source_priority) OVER(PARTITION BY members.author_id) AS tier
                 FROM members JOIN eligible ON eligible.row_id=members.book_row_id
                 {name_filter}
               ),stats AS (
                 SELECT author_id,count(*) AS total,sum(downloaded) AS downloaded,
                        min(source_priority) AS priority,
                        sum(CASE WHEN source_priority=tier THEN score END) AS score
                 FROM credited GROUP BY author_id
               )"#
        } else {
            r#",stats AS (
                 SELECT members.author_id,count(*) AS total,sum(downloaded) AS downloaded,
                        min(source_priority) AS priority,NULL AS score
                 FROM members JOIN eligible ON eligible.row_id=members.book_row_id {name_filter} GROUP BY members.author_id
               )"#
        };
        let name_filter = if self.options.search.is_empty() { "" } else { "WHERE members.author_id IN (SELECT author_id FROM names WHERE instr(normalize_search_text(name),:search)>0)" };
        let stats = stats.replace("{name_filter}", name_filter);
        let sql = format!("{stats} SELECT names.identity,names.name,stats.total,stats.downloaded,stats.priority,stats.score FROM names JOIN stats USING(author_id)");
        let mut rows = Vec::new();
        let mut scores = HashMap::new();
        let mut priorities = HashMap::new();
        self.read(&sql, None, &[], |row| {
            let name: String = row.get(1)?;
            if !self.options.search.is_empty() && !normalize_search_text(&name).contains(&self.options.search) {
                return Ok(());
            }
            let id = self.identity(row, 0)?;
            let location = if self.catalog { BrowseLocation::CatalogAuthor(Some(id)) } else { BrowseLocation::Author(Some(id)) };
            rows.push(BrowseRow { id: location, name, path: None, book_count: count(row, 2)?, downloaded_book_count: count(row, 3)? });
            priorities.insert(location, row.get::<_, u8>(4)?);
            scores.insert(location, row.get::<_, Option<f64>>(5)?);
            Ok(())
        })?;
        rows.sort_unstable_by_key(|row| row.id);
        popularity::sort_rows(&mut rows, self.options.chip_sort, |row| scores[&row.id]);
        if self.catalog {
            if popularity {
                rows.sort_by_key(|row| priorities[&row.id]);
            }
            rows.sort_by_key(|row| priorities[&row.id] != 0);
        }
        Ok(rows)
    }

    pub(super) fn name(&self, author: AuthorId) -> Result<Option<String>, DatabaseError> {
        let mut name = None;
        self.read("SELECT name FROM names WHERE identity=:author", Some(author), &[], |row| {
            name = Some(row.get(0)?);
            Ok(())
        })?;
        Ok(name)
    }

    pub(super) fn orders(&self, selected: &[AuthorId]) -> Result<HashMap<AuthorId, Vec<i64>>, DatabaseError> {
        let mut orders = selected.iter().map(|&id| (id, Vec::new())).collect::<HashMap<_, _>>();
        if selected.is_empty() {
            return Ok(orders);
        }
        let sql = format!("SELECT names.identity,eligible.row_id FROM members JOIN eligible ON eligible.row_id=members.book_row_id JOIN names USING(author_id) ORDER BY {}", self.order());
        self.read(&sql, None, selected, |row| {
            let id = self.identity(row, 0)?;
            orders.get_mut(&id).expect("requested author").push(row.get(1)?);
            Ok(())
        })?;
        Ok(orders)
    }

    fn order(&self) -> &'static str {
        if self.catalog {
            if self.options.book_sort == BrowseBookSort::Popularity { "(source_priority=0) DESC,source_priority,score IS NULL,score DESC,search_title,title,key" } else { "(source_priority=0) DESC,search_title,title,key" }
        } else {
            match self.options.book_sort {
                BrowseBookSort::RecentlyRead => "read_at DESC,search_title,title,key",
                BrowseBookSort::Popularity => "score IS NULL,score DESC,search_title,title,key",
                _ => "search_title,title,key",
            }
        }
    }

    pub(super) fn all_ordered(&self) -> Result<Vec<i64>, DatabaseError> {
        self.ordered_scope(None, false)
    }

    pub(super) fn ordered(&self, author: Option<AuthorId>) -> Result<Vec<i64>, DatabaseError> {
        self.ordered_scope(author, true)
    }

    fn ordered_scope(&self, author: Option<AuthorId>, credited_only: bool) -> Result<Vec<i64>, DatabaseError> {
        let order = self.order();
        let (from, scope) = if author.is_some() {
            ("members JOIN eligible ON eligible.row_id=members.book_row_id", "1")
        } else if credited_only {
            ("eligible", "EXISTS(SELECT 1 FROM members WHERE members.book_row_id=eligible.row_id)")
        } else {
            ("eligible", "1")
        };
        let sql = format!("SELECT eligible.row_id FROM {from} WHERE {scope} {} ORDER BY {order}", if self.options.search.is_empty() { "" } else { "AND text_match=1" });
        let mut books = Vec::new();
        self.read(&sql, author, &[], |row| {
            books.push(row.get(0)?);
            Ok(())
        })?;
        Ok(books)
    }
}

fn count(row: &Row<'_>, column: usize) -> rusqlite::Result<usize> {
    usize::try_from(row.get::<_, i64>(column)?).map_err(|_| invalid_integer_column(column, "author count must fit in usize"))
}
