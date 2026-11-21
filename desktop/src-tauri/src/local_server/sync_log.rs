use chrono::Utc;
use motodesk_core::entity::EntityType;
use motodesk_core::sync::{ChangeLogEntry, FieldMetaMap, OpKind};
use motodesk_core::sync::FieldStamp;
use motodesk_sync::field_meta::{apply_patch_stamps, merge_field_meta, stamps_for_payload};
use serde_json::Value;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::local_server::db;
use crate::local_server::error::LocalResult;

/// Append a change-log entry inside a transaction so the per-device `seq` is
/// monotonic even under concurrent writes.
pub async fn append(
    pool: &SqlitePool,
    device_id: Uuid,
    entity_type: EntityType,
    entity_id: Uuid,
    op: OpKind,
    payload: Value,
    field_meta: Option<FieldMetaMap>,
) -> LocalResult<ChangeLogEntry> {
    // An edit claims only the fields it changed. Stamping the whole row "now" made a
    // rename undo a price changed on another PC a moment earlier (the row sent still
    // carried the price it last saw).
    let own_stamps = field_meta.is_none() && op == OpKind::Update;
    let meta = match field_meta {
        Some(meta) => meta,
        None if op == OpKind::Update => {
            let before = announced_state(pool, entity_type, entity_id).await?;
            changed_stamps(before.as_ref(), &payload, device_id, Utc::now())
        }
        None => stamps_for_payload(&payload, device_id, Utc::now()),
    };
    let id = Uuid::new_v4();
    let created_at = Utc::now();

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let seq = db::next_seq(&mut tx, device_id).await?;
    sqlx::query(
        r#"
        INSERT INTO change_log (id, device_id, seq, entity_type, entity_id, op, payload, field_meta, created_at, synced_to_cloud, relayed)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 0, 0)
        "#,
    )
    .bind(id.to_string())
    .bind(device_id.to_string())
    .bind(seq)
    .bind(entity_type.as_str())
    .bind(entity_id.to_string())
    .bind(op.as_str())
    .bind(serde_json::to_string(&payload).unwrap_or_else(|_| "{}".into()))
    .bind(serde_json::to_string(&meta).unwrap_or_else(|_| "{}".into()))
    .bind(created_at.to_rfc3339())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    // The row has to know it was just edited too, or a change from elsewhere that is
    // older than this edit would be applied over it.
    if own_stamps {
        remember_stamps(pool, entity_type, entity_id, &meta).await?;
    }

    Ok(ChangeLogEntry {
        id,
        device_id,
        seq,
        entity_type,
        entity_id,
        op,
        payload,
        field_meta: meta,
        created_at,
        synced_to_cloud: false,
        relayed: false,
    })
}

/// What this device last knew of a row, rebuilt from the changes it holds for it: the
/// payloads of the latest few, oldest first, laid over one another.
async fn announced_state(
    pool: &SqlitePool,
    entity_type: EntityType,
    entity_id: Uuid,
) -> LocalResult<Option<serde_json::Map<String, Value>>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT payload FROM change_log
          WHERE entity_type = $1 AND entity_id = $2 AND op != 'delete'
          ORDER BY created_at DESC, seq DESC LIMIT 20",
    )
    .bind(entity_type.as_str())
    .bind(entity_id.to_string())
    .fetch_all(pool)
    .await?;
    if rows.is_empty() {
        return Ok(None);
    }
    let mut state = serde_json::Map::new();
    for (payload,) in rows.into_iter().rev() {
        if let Ok(Value::Object(fields)) = serde_json::from_str::<Value>(&payload) {
            state.extend(fields);
        }
    }
    Ok(Some(state))
}

/// Stamps for the fields of `after` that differ from `before`. With nothing to compare
/// to, every field counts as changed (the old behaviour). An edit that changed nothing
/// gets a marker so the receiver, which treats stamps as the list of changed fields,
/// does not read its silence as "everything".
pub fn changed_stamps(
    before: Option<&serde_json::Map<String, Value>>,
    after: &Value,
    device_id: Uuid,
    now: chrono::DateTime<Utc>,
) -> FieldMetaMap {
    let Some(before) = before else {
        return stamps_for_payload(after, device_id, now);
    };
    let all = stamps_for_payload(after, device_id, now);
    let mut changed: FieldMetaMap = all
        .into_iter()
        .filter(|(field, _)| before.get(field) != after.get(field))
        .collect();
    if changed.is_empty() {
        changed.insert("_unchanged".into(), FieldStamp::new(now, device_id));
    }
    changed
}

fn table_of(entity_type: EntityType) -> Option<&'static str> {
    Some(match entity_type {
        EntityType::Client => "clients",
        EntityType::Vehicle => "vehicles",
        EntityType::Category => "categories",
        EntityType::Product => "products",
        EntityType::DocTemplate => "doc_templates",
        EntityType::Sale => "sales",
        EntityType::SaleLine => "sale_lines",
        EntityType::Payment => "payments",
        EntityType::Supplier => "suppliers",
        EntityType::WorkshopOrder => "workshop_orders",
        EntityType::StockMovement => "stock_movements",
        _ => return None,
    })
}

/// Record on the row itself which fields this device just wrote, and when.
async fn remember_stamps(
    pool: &SqlitePool,
    entity_type: EntityType,
    entity_id: Uuid,
    stamps: &FieldMetaMap,
) -> LocalResult<()> {
    let Some(table) = table_of(entity_type) else {
        return Ok(());
    };
    let held: Option<(String,)> =
        sqlx::query_as(&format!("SELECT field_meta FROM {table} WHERE id = $1"))
            .bind(entity_id.to_string())
            .fetch_optional(pool)
            .await?;
    let Some((held,)) = held else {
        return Ok(());
    };
    let current: FieldMetaMap = serde_json::from_str(&held).unwrap_or_default();
    let merged = merge_field_meta(&current, stamps);
    sqlx::query(&format!("UPDATE {table} SET field_meta = $2 WHERE id = $1"))
        .bind(entity_id.to_string())
        .bind(serde_json::to_string(&merged).unwrap_or_else(|_| "{}".into()))
        .execute(pool)
        .await?;
    Ok(())
}

pub fn field_meta_for_patch(
    existing_meta: &FieldMetaMap,
    patch: &Value,
    device_id: Uuid,
) -> FieldMetaMap {
    apply_patch_stamps(existing_meta, patch, device_id, Utc::now())
}

pub async fn unsynced_ops(pool: &SqlitePool, limit: i64) -> LocalResult<Vec<ChangeLogEntry>> {
    let rows = sqlx::query_as::<_, ChangelogRow>(
        r#"
        SELECT id, device_id, seq, entity_type, entity_id, op, payload, field_meta, created_at, synced_to_cloud, relayed
        FROM change_log WHERE synced_to_cloud = 0 ORDER BY device_id ASC, seq ASC LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().filter_map(|r| r.into_entry()).collect())
}

/// Changes to send to the cloud now: not delivered yet, and not waiting out the
/// back-off that follows a refusal. A refused change steps aside instead of sitting at
/// the front of the queue, where enough of them would hide everything behind.
pub async fn due_ops(pool: &SqlitePool, limit: i64) -> LocalResult<Vec<ChangeLogEntry>> {
    let rows = sqlx::query_as::<_, ChangelogRow>(
        r#"
        SELECT id, device_id, seq, entity_type, entity_id, op, payload, field_meta, created_at, synced_to_cloud, relayed
        FROM change_log
        WHERE synced_to_cloud = 0 AND (retry_after IS NULL OR retry_after <= $2)
        ORDER BY device_id ASC, seq ASC LIMIT $1
        "#,
    )
    .bind(limit)
    .bind(Utc::now().to_rfc3339())
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().filter_map(|r| r.into_entry()).collect())
}

/// How long a change waits after its `attempts`-th refusal: a minute, five, half an
/// hour, two hours, six, then once a day. It is never given up on (the cause is often
/// elsewhere, a missing earlier change or an administrator who has not signed in yet),
/// but a day's wait costs the cloud one change a day.
pub fn backoff_after(attempts: i64) -> chrono::Duration {
    const MINUTES: [i64; 6] = [1, 5, 30, 120, 360, 1_440];
    let step = (attempts.max(1) as usize - 1).min(MINUTES.len() - 1);
    chrono::Duration::minutes(MINUTES[step])
}

/// Remember that the cloud refused these changes, and why, and push their next try out.
pub async fn record_refusals(pool: &SqlitePool, refusals: &[(Uuid, String)]) -> LocalResult<()> {
    if refusals.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    for (id, reason) in refusals {
        let held: Option<(i64,)> =
            sqlx::query_as("SELECT push_attempts FROM change_log WHERE id = $1")
                .bind(id.to_string())
                .fetch_optional(&mut *tx)
                .await?;
        let attempts = held.map(|(n,)| n).unwrap_or(0) + 1;
        sqlx::query(
            "UPDATE change_log SET push_attempts = $2, push_error = $3, retry_after = $4 WHERE id = $1",
        )
        .bind(id.to_string())
        .bind(attempts)
        .bind(reason)
        .bind((Utc::now() + backoff_after(attempts)).to_rfc3339())
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Try the refused changes again now (the owner asks, usually after fixing the cause).
pub async fn retry_refused(pool: &SqlitePool) -> LocalResult<u64> {
    let done = sqlx::query(
        "UPDATE change_log SET retry_after = NULL, push_attempts = 0
          WHERE synced_to_cloud = 0 AND push_attempts > 0",
    )
    .execute(pool)
    .await?;
    Ok(done.rows_affected())
}

#[derive(Debug, Default, serde::Serialize)]
pub struct RefusedSummary {
    /// Changes the cloud has refused and that are still waiting to be sent.
    pub count: i64,
    /// The latest reason, with what it was about, to show the owner.
    pub latest: Option<String>,
}

pub async fn refused_summary(pool: &SqlitePool) -> LocalResult<RefusedSummary> {
    let (count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM change_log WHERE synced_to_cloud = 0 AND push_attempts > 0",
    )
    .fetch_one(pool)
    .await?;
    let latest: Option<(String, Option<String>)> = sqlx::query_as(
        "SELECT entity_type, push_error FROM change_log
          WHERE synced_to_cloud = 0 AND push_attempts > 0
          ORDER BY retry_after DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    Ok(RefusedSummary {
        count,
        latest: latest.map(|(entity, why)| format!("{entity} : {}", why.unwrap_or_else(|| "refusé par le cloud".into()))),
    })
}

pub async fn mark_synced(pool: &SqlitePool, ids: &[Uuid]) -> LocalResult<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    for id in ids {
        sqlx::query("UPDATE change_log SET synced_to_cloud = 1 WHERE id = $1")
            .bind(id.to_string())
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// The highest sequence number held per originating device.
pub async fn version_vector(pool: &SqlitePool) -> LocalResult<std::collections::HashMap<Uuid, i64>> {
    let rows: Vec<(String, i64)> =
        sqlx::query_as("SELECT device_id, MAX(seq) FROM change_log GROUP BY device_id")
            .fetch_all(pool)
            .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(d, s)| Some((Uuid::parse_str(&d).ok()?, s)))
        .collect())
}

/// Every device that has a change in this log.
pub async fn known_devices(pool: &SqlitePool) -> LocalResult<Vec<Uuid>> {
    let rows: Vec<(String,)> = sqlx::query_as("SELECT DISTINCT device_id FROM change_log ORDER BY device_id")
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().filter_map(|(d,)| Uuid::parse_str(&d).ok()).collect())
}

/// The changes of one device beyond a sequence number, oldest first.
pub async fn ops_after(
    pool: &SqlitePool,
    device: Uuid,
    after_seq: i64,
    limit: i64,
) -> LocalResult<Vec<ChangeLogEntry>> {
    let rows = sqlx::query_as::<_, ChangelogRow>(
        r#"
        SELECT id, device_id, seq, entity_type, entity_id, op, payload, field_meta, created_at, synced_to_cloud, relayed
        FROM change_log WHERE device_id = $1 AND seq > $2 ORDER BY seq ASC LIMIT $3
        "#,
    )
    .bind(device.to_string())
    .bind(after_seq)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().filter_map(|r| r.into_entry()).collect())
}

/// What a network PC still has to push to its LAN peers: everything logged after its cursor
/// (with no cloud, nothing is ever "synced", so "not synced yet" would never shrink), and the
/// cursor value to store once it has been sent.
pub async fn unpushed_lan_ops(pool: &SqlitePool, limit: i64) -> LocalResult<(Vec<ChangeLogEntry>, i64)> {
    let (cursor,): (i64,) = sqlx::query_as("SELECT lan_pushed_rowid FROM sync_state WHERE id = 1")
        .fetch_one(pool)
        .await?;
    let rows = sqlx::query_as::<_, ChangelogRow>(
        r#"
        SELECT id, device_id, seq, entity_type, entity_id, op, payload, field_meta, created_at, synced_to_cloud, relayed
        FROM change_log WHERE rowid > $1 ORDER BY rowid ASC LIMIT $2
        "#,
    )
    .bind(cursor)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    let (last,): (Option<i64>,) = sqlx::query_as(
        "SELECT MAX(r) FROM (SELECT rowid AS r FROM change_log WHERE rowid > $1 ORDER BY rowid ASC LIMIT $2)",
    )
    .bind(cursor)
    .bind(limit)
    .fetch_one(pool)
    .await?;
    Ok((rows.into_iter().filter_map(|r| r.into_entry()).collect(), last.unwrap_or(cursor)))
}

pub async fn mark_lan_pushed(pool: &SqlitePool, rowid: i64) -> LocalResult<()> {
    sqlx::query("UPDATE sync_state SET lan_pushed_rowid = $1 WHERE id = 1 AND lan_pushed_rowid < $1")
        .bind(rowid)
        .execute(pool)
        .await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct ChangelogRow {
    id: String,
    device_id: String,
    seq: i64,
    entity_type: String,
    entity_id: String,
    op: String,
    payload: String,
    field_meta: String,
    created_at: String,
    synced_to_cloud: i32,
    relayed: i32,
}

impl ChangelogRow {
    fn into_entry(self) -> Option<ChangeLogEntry> {
        let entity_type = EntityType::parse(&self.entity_type)?;
        let op = OpKind::parse(&self.op)?;
        let payload: Value = serde_json::from_str(&self.payload).ok()?;
        let field_meta: FieldMetaMap = serde_json::from_str(&self.field_meta).unwrap_or_default();
        let created_at = chrono::DateTime::parse_from_rfc3339(&self.created_at)
            .ok()?
            .with_timezone(&Utc);

        Some(ChangeLogEntry {
            id: Uuid::parse_str(&self.id).ok()?,
            device_id: Uuid::parse_str(&self.device_id).ok()?,
            seq: self.seq,
            entity_type,
            entity_id: Uuid::parse_str(&self.entity_id).ok()?,
            op,
            payload,
            field_meta,
            created_at,
            synced_to_cloud: self.synced_to_cloud != 0,
            relayed: self.relayed != 0,
        })
    }
}

#[cfg(test)]
mod stamp_tests {
    use super::*;
    use serde_json::json;

    fn before() -> serde_json::Map<String, Value> {
        json!({ "id": "x", "name": "Filtre", "price": 100, "notes": null })
            .as_object()
            .unwrap()
            .clone()
    }

    #[test]
    fn only_the_fields_that_differ_are_stamped() {
        let device = Uuid::new_v4();
        let after = json!({ "id": "x", "name": "Filtre à huile", "price": 100, "notes": null });
        let stamps = changed_stamps(Some(&before()), &after, device, Utc::now());
        let mut keys: Vec<_> = stamps.keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, vec!["name"]);

        let after = json!({ "id": "x", "name": "Filtre", "price": 120, "notes": "ok" });
        let mut keys: Vec<_> = changed_stamps(Some(&before()), &after, device, Utc::now()).into_keys().collect();
        keys.sort();
        assert_eq!(keys, vec!["notes", "price"]);
    }

    #[test]
    fn an_edit_that_changed_nothing_says_so_rather_than_saying_nothing() {
        let after = Value::Object(before());
        let stamps = changed_stamps(Some(&before()), &after, Uuid::new_v4(), Utc::now());
        assert_eq!(stamps.keys().collect::<Vec<_>>(), vec!["_unchanged"]);
    }

    #[test]
    fn with_nothing_to_compare_to_every_field_counts_as_changed() {
        let after = json!({ "id": "x", "name": "Filtre", "price": 100 });
        let stamps = changed_stamps(None, &after, Uuid::new_v4(), Utc::now());
        assert!(stamps.contains_key("name") && stamps.contains_key("price"));
    }
}

#[cfg(test)]
mod outbox_tests {
    use super::*;
    use crate::local_server::test_support::shop;
    use motodesk_core::entity::EntityType;
    use serde_json::json;

    async fn queue(pool: &SqlitePool, device: Uuid, n: usize) -> Vec<Uuid> {
        let mut ids = Vec::new();
        for i in 0..n {
            let id = Uuid::new_v4();
            let entry = append(
                pool,
                device,
                EntityType::Client,
                id,
                OpKind::Insert,
                json!({ "id": id.to_string(), "last_name": format!("C{i}") }),
                None,
            )
            .await
            .unwrap();
            ids.push(entry.id);
        }
        ids
    }

    #[test]
    fn the_wait_after_a_refusal_grows_and_then_settles_at_a_day() {
        let minutes: Vec<i64> = (1..=8).map(|n| backoff_after(n).num_minutes()).collect();
        assert_eq!(minutes, vec![1, 5, 30, 120, 360, 1_440, 1_440, 1_440]);
        assert_eq!(backoff_after(0).num_minutes(), 1, "never zero");
    }

    /// Enough refused changes at the front used to hide everything behind them: the same
    /// ones were read, and refused, forever.
    #[tokio::test]
    async fn refused_changes_step_aside_so_the_ones_behind_them_get_through() {
        let state = shop().await;
        let device = state.device_id().await.unwrap();
        sqlx::query("DELETE FROM change_log").execute(&state.db).await.unwrap();

        let ids = queue(&state.db, device, 8).await;
        let (refused, good) = ids.split_at(5);
        let before = due_ops(&state.db, 3).await.unwrap();
        assert_eq!(
            before.iter().map(|o| o.id).collect::<Vec<_>>(),
            refused[..3].to_vec(),
            "the refused ones are first in line"
        );

        let refusals: Vec<(Uuid, String)> =
            refused.iter().map(|id| (*id, "le client n'existe pas".to_string())).collect();
        record_refusals(&state.db, &refusals).await.unwrap();

        // A batch of 3 now reaches the changes that were stuck behind them.
        let next = due_ops(&state.db, 3).await.unwrap();
        assert_eq!(next.iter().map(|o| o.id).collect::<Vec<_>>(), good.to_vec());

        // Refused again, they wait longer; they are still not lost.
        record_refusals(&state.db, &refusals[..1]).await.unwrap();
        let (attempts, error): (i64, String) =
            sqlx::query_as("SELECT push_attempts, push_error FROM change_log WHERE id = $1")
                .bind(refused[0].to_string())
                .fetch_one(&state.db)
                .await
                .unwrap();
        assert_eq!((attempts, error.as_str()), (2, "le client n'existe pas"));
        assert_eq!(unsynced_ops(&state.db, 100).await.unwrap().len(), 8, "all eight are still queued");

        let summary = refused_summary(&state.db).await.unwrap();
        assert_eq!(summary.count, 5);
        assert!(summary.latest.unwrap().contains("le client n'existe pas"));
    }

    #[tokio::test]
    async fn the_owner_can_send_the_refused_changes_again_now() {
        let state = shop().await;
        let device = state.device_id().await.unwrap();
        sqlx::query("DELETE FROM change_log").execute(&state.db).await.unwrap();
        let ids = queue(&state.db, device, 2).await;
        record_refusals(&state.db, &[(ids[0], "refusé".into())]).await.unwrap();
        assert_eq!(due_ops(&state.db, 10).await.unwrap().len(), 1);

        assert_eq!(retry_refused(&state.db).await.unwrap(), 1);
        assert_eq!(due_ops(&state.db, 10).await.unwrap().len(), 2);
        assert_eq!(refused_summary(&state.db).await.unwrap().count, 0);
    }

    /// A change that finally goes through is delivered like any other.
    #[tokio::test]
    async fn a_refused_change_that_later_succeeds_is_delivered() {
        let state = shop().await;
        let device = state.device_id().await.unwrap();
        sqlx::query("DELETE FROM change_log").execute(&state.db).await.unwrap();
        let ids = queue(&state.db, device, 1).await;
        record_refusals(&state.db, &[(ids[0], "pas encore".into())]).await.unwrap();
        retry_refused(&state.db).await.unwrap();
        mark_synced(&state.db, &ids).await.unwrap();
        assert!(due_ops(&state.db, 10).await.unwrap().is_empty());
        assert_eq!(refused_summary(&state.db).await.unwrap().count, 0);
    }
}
