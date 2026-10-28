use crate::error::AppError;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KnowledgeItem {
    pub id: String,
    pub title: String,
    pub content: String,
    pub kind: String,
    pub category: String,
    pub tags: String,
    #[serde(default)]
    pub item_date: Option<String>,
    pub confidence: f64,
    #[serde(default)]
    pub provenance: String,
    #[serde(default)]
    pub created_by: String,
    #[serde(default)]
    pub source_entry_id: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KnowledgeLink {
    pub id: String,
    pub source_id: String,
    pub target_id: String,
    pub relation: String,
    pub created_by: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KnowledgeLinkRow {
    pub id: String,
    pub direction: String, // "out" => this item is the source; "in" => this item is the target
    pub relation: String,
    pub other_id: String,
    pub other_title: String,
    pub created_by: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KnowledgeItemDetail {
    pub item: KnowledgeItem,
    pub links: Vec<KnowledgeLinkRow>,
}

// SELECT column order for knowledge_items — keep in sync with row_to_item and the SQL strings above.
const ITEM_COLS: &str = "id, title, content, kind, category, tags, item_date, confidence, provenance, created_by, source_entry_id, created_at, updated_at";

fn row_to_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<KnowledgeItem> {
    Ok(KnowledgeItem {
        id: row.get(0)?,
        title: row.get(1)?,
        content: row.get(2)?,
        kind: row.get(3)?,
        category: row.get(4)?,
        tags: row.get(5)?,
        item_date: row.get(6)?,
        confidence: row.get(7)?,
        provenance: row.get(8)?,
        created_by: row.get(9)?,
        source_entry_id: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

pub fn validate_kind(v: &str) -> bool {
    matches!(v, "memory" | "information" | "knowledge")
}

pub fn validate_category(v: &str) -> bool {
    matches!(
        v,
        "preference" | "habit" | "constraint" | "person" | "goal" | "other"
    )
}

pub fn validate_relation(v: &str) -> bool {
    matches!(
        v,
        "related-to" | "part-of" | "contradicts" | "cause-of" | "reminds-of"
    )
}

pub fn list_items(
    conn: &Connection,
    kind: Option<&str>,
    category: Option<&str>,
    search: Option<&str>,
) -> Result<Vec<KnowledgeItem>, AppError> {
    let mut sql = format!("SELECT {ITEM_COLS} FROM knowledge_items");
    let mut conds: Vec<String> = Vec::new();
    let mut vals: Vec<String> = Vec::new();

    if let Some(k) = kind {
        if !k.is_empty() {
            conds.push("kind = ?".into());
            vals.push(k.to_string());
        }
    }
    if let Some(c) = category {
        if !c.is_empty() {
            conds.push("category = ?".into());
            vals.push(c.to_string());
        }
    }
    if let Some(s) = search {
        if !s.is_empty() {
            conds.push("(title LIKE ? OR content LIKE ?)".into());
            let term = format!("%{}%", s);
            vals.push(term.clone());
            vals.push(term);
        }
    }
    if !conds.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&conds.join(" AND "));
    }
    sql.push_str(" ORDER BY updated_at DESC");

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(vals.iter()), |row| {
        row_to_item(row)
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn get_item(conn: &Connection, id: &str) -> Result<Option<KnowledgeItem>, AppError> {
    let sql = format!("SELECT {ITEM_COLS} FROM knowledge_items WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map(params![id], |row| row_to_item(row))?;
    match rows.next() {
        Some(Ok(item)) => Ok(Some(item)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

pub fn get_item_detail(
    conn: &Connection,
    id: &str,
) -> Result<Option<KnowledgeItemDetail>, AppError> {
    let Some(item) = get_item(conn, id)? else {
        return Ok(None);
    };

    let mut links = Vec::new();
    let mut stmt_out = conn.prepare(
        "SELECT kl.id, kl.relation, kl.created_by, kl.created_at, ki.id, ki.title
         FROM knowledge_links kl
         JOIN knowledge_items ki ON ki.id = kl.target_id
         WHERE kl.source_id = ?1
         ORDER BY kl.created_at ASC",
    )?;
    let out_rows = stmt_out.query_map(params![id], |row| {
        Ok(KnowledgeLinkRow {
            id: row.get(0)?,
            direction: "out".to_string(),
            relation: row.get(1)?,
            other_id: row.get(4)?,
            other_title: row.get(5)?,
            created_by: row.get(2)?,
            created_at: row.get(3)?,
        })
    })?;
    for r in out_rows {
        links.push(r?);
    }

    let mut stmt_in = conn.prepare(
        "SELECT kl.id, kl.relation, kl.created_by, kl.created_at, ki.id, ki.title
         FROM knowledge_links kl
         JOIN knowledge_items ki ON ki.id = kl.source_id
         WHERE kl.target_id = ?1
         ORDER BY kl.created_at ASC",
    )?;
    let in_rows = stmt_in.query_map(params![id], |row| {
        Ok(KnowledgeLinkRow {
            id: row.get(0)?,
            direction: "in".to_string(),
            relation: row.get(1)?,
            other_id: row.get(4)?,
            other_title: row.get(5)?,
            created_by: row.get(2)?,
            created_at: row.get(3)?,
        })
    })?;
    for r in in_rows {
        links.push(r?);
    }

    Ok(Some(KnowledgeItemDetail { item, links }))
}

pub fn create_item(conn: &Connection, item: &KnowledgeItem) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO knowledge_items
            (id, title, content, kind, category, tags, item_date, confidence, provenance, created_by, source_entry_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            item.id, item.title, item.content, item.kind, item.category,
            item.tags, item.item_date, item.confidence, item.provenance,
            item.created_by, item.source_entry_id, item.created_at, item.updated_at,
        ],
    )?;
    Ok(())
}

pub fn update_item(conn: &Connection, item: &KnowledgeItem) -> Result<(), AppError> {
    conn.execute(
        "UPDATE knowledge_items
         SET title = ?1, content = ?2, kind = ?3, category = ?4,
             tags = ?5, item_date = ?6, confidence = ?7, updated_at = ?8
         WHERE id = ?9",
        params![
            item.title,
            item.content,
            item.kind,
            item.category,
            item.tags,
            item.item_date,
            item.confidence,
            item.updated_at,
            item.id,
        ],
    )?;
    Ok(())
}

pub fn delete_item(conn: &Connection, id: &str) -> Result<(), AppError> {
    conn.execute("DELETE FROM knowledge_items WHERE id = ?1", params![id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::migrations::run_pending(&mut conn).unwrap();
        conn
    }

    #[test]
    fn list_items_keeps_recently_updated_items_first() {
        let conn = test_conn();
        conn.execute(
            "INSERT INTO knowledge_items
             (id, title, content, kind, category, tags, confidence, provenance, created_by, created_at, updated_at)
             VALUES ('old-high', 'Old', 'old fact', 'information', 'other', '[]', 1.0, 'manual', 'user', '2026-01-01', '2026-01-01'),
                    ('recent-edit', 'Edited', 'updated fact', 'information', 'other', '[]', 0.5, 'manual', 'user', '2025-01-01', '2026-10-28')",
            [],
        )
        .unwrap();

        let items = list_items(&conn, None, None, None).unwrap();
        assert_eq!(items[0].id, "recent-edit");
    }
}

pub fn link_items(conn: &Connection, link: &KnowledgeLink) -> Result<(), AppError> {
    let changed = conn.execute(
        "INSERT INTO knowledge_links (id, source_id, target_id, relation, created_by, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(source_id, target_id, relation) DO NOTHING",
        params![
            link.id,
            link.source_id,
            link.target_id,
            link.relation,
            link.created_by,
            link.created_at
        ],
    )?;
    if changed == 0 {
        return Err(AppError::InvalidInput(format!(
            "Link already exists: {} -> {} ({})",
            link.source_id, link.target_id, link.relation
        )));
    }
    Ok(())
}

pub fn unlink_item(conn: &Connection, link_id: &str) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM knowledge_links WHERE id = ?1",
        params![link_id],
    )?;
    Ok(())
}
