use crate::error::AppError;
use chrono::Utc;
use rusqlite::{params, Connection};

#[derive(Debug, Clone)]
pub struct JournalVector {
    pub entry_id: String,
    pub vector: Vec<f32>,
    pub model: String,
    pub created_at: String,
}

pub fn upsert_vector(
    conn: &Connection,
    entry_id: &str,
    vector: &[f32],
    model: &str,
) -> Result<(), AppError> {
    let mut vector_bytes = Vec::with_capacity(vector.len() * 4);
    for &f in vector {
        vector_bytes.extend_from_slice(&f.to_le_bytes());
    }

    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO journal_vectors (entry_id, vector, model, created_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(entry_id) DO UPDATE SET vector=excluded.vector, model=excluded.model, created_at=excluded.created_at",
        params![entry_id, vector_bytes, model, now],
    ).map_err(AppError::from)?;

    Ok(())
}

pub fn get_vector(conn: &Connection, entry_id: &str) -> Result<Option<JournalVector>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT entry_id, vector, model, created_at FROM journal_vectors WHERE entry_id = ?1",
    )?;
    let mut rows = stmt.query(params![entry_id])?;

    if let Some(row) = rows.next()? {
        let entry_id: String = row.get(0)?;
        let bytes: Vec<u8> = row.get(1)?;
        let model: String = row.get(2)?;
        let created_at: String = row.get(3)?;

        let mut vector = Vec::with_capacity(bytes.len() / 4);
        for chunk in bytes.chunks_exact(4) {
            let mut arr = [0u8; 4];
            arr.copy_from_slice(chunk);
            vector.push(f32::from_le_bytes(arr));
        }

        Ok(Some(JournalVector {
            entry_id,
            vector,
            model,
            created_at,
        }))
    } else {
        Ok(None)
    }
}

pub fn list_all_vectors(conn: &Connection) -> Result<Vec<JournalVector>, AppError> {
    let mut stmt =
        conn.prepare("SELECT entry_id, vector, model, created_at FROM journal_vectors")?;
    let rows = stmt.query_map([], |row| {
        let entry_id: String = row.get(0)?;
        let bytes: Vec<u8> = row.get(1)?;
        let model: String = row.get(2)?;
        let created_at: String = row.get(3)?;

        let mut vector = Vec::with_capacity(bytes.len() / 4);
        for chunk in bytes.chunks_exact(4) {
            let mut arr = [0u8; 4];
            arr.copy_from_slice(chunk);
            vector.push(f32::from_le_bytes(arr));
        }

        Ok(JournalVector {
            entry_id,
            vector,
            model,
            created_at,
        })
    })?;

    let mut results = Vec::new();
    for r in rows {
        results.push(r?);
    }
    Ok(results)
}

pub fn cosine_similarity(v1: &[f32], v2: &[f32]) -> f32 {
    if v1.len() != v2.len() || v1.is_empty() {
        return 0.0;
    }
    let dot_product: f32 = v1.iter().zip(v2.iter()).map(|(a, b)| a * b).sum();
    let norm1: f32 = v1.iter().map(|a| a * a).sum::<f32>().sqrt();
    let norm2: f32 = v2.iter().map(|a| a * a).sum::<f32>().sqrt();

    if norm1 == 0.0 || norm2 == 0.0 {
        return 0.0;
    }
    dot_product / (norm1 * norm2)
}

pub fn semantic_search(
    conn: &Connection,
    query_vector: &[f32],
    limit: usize,
) -> Result<Vec<(String, f32)>, AppError> {
    let mut stmt = conn.prepare("SELECT entry_id, vector FROM journal_vectors")?;
    let rows = stmt.query_map([], |row| {
        let entry_id: String = row.get(0)?;
        let bytes: Vec<u8> = row.get(1)?;

        let mut vector = Vec::with_capacity(bytes.len() / 4);
        for chunk in bytes.chunks_exact(4) {
            let mut arr = [0u8; 4];
            arr.copy_from_slice(chunk);
            vector.push(f32::from_le_bytes(arr));
        }

        Ok((entry_id, vector))
    })?;

    let mut similarities = Vec::new();
    for row in rows {
        let (entry_id, vector) = row?;
        let score = cosine_similarity(query_vector, &vector);
        similarities.push((entry_id, score));
    }

    similarities.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    similarities.truncate(limit);

    Ok(similarities)
}
