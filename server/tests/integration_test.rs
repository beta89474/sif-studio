//! SIF Studio 集成测试 —— 仅依赖 sqlx 内存池。
//! 不依赖 Tauri runtime，所以 cargo test 静态跑可通。
//!
//! 设计原则：
//! - 每个用例先 open_in_memory()，再走业务路径
//! - 验证跨图汇总（list_sifs）的 SQL 聚合语义 + CASCADE
//! - 验证 SIF ↔ Instrument 双向 link/unlink
//! - 验证 M2.1 仪表批量导入（CSV + 三种冲突策略 + 审计写入）

use sif_studio_lib::commands::bypasses::{
    count_overdue_bypasses_inner, create_bypass_inner, delete_bypass_inner, list_bypasses_inner,
    restore_bypass_inner, update_bypass_inner, BypassInput, BypassUpdate,
};
use sif_studio_lib::commands::imports::{
    commit_import_instruments_inner, CommitImportInput, MappingEntry,
};
use sif_studio_lib::commands::instruments::{
    create_instrument_inner, delete_instrument_inner, list_instrument_history_inner,
    update_instrument_inner, InstrumentInput,
};
use sif_studio_lib::db::open_in_memory;
use sif_studio_lib::import::{parse_file, ParsedSheet};
use sqlx::{Pool, Row, Sqlite};

/// B 阶段统一组织夹具（004 迁移后内存库自带 org id=1）
const ORG: i64 = 1;
const ACTOR: &str = "tester";

// ===========================================================================
// 共享 helper
// ===========================================================================

async fn setup_scenario() -> (Pool<Sqlite>, i64, i64, i64, i64, i64) {
    let pool = open_in_memory().await.expect("open");

    let pid: i64 =
        sqlx::query("INSERT INTO project (org_id, code, name) VALUES (1, 'PRJ-T', '测试项目')")
            .execute(&pool)
            .await
            .unwrap()
            .last_insert_rowid();

    let did1: i64 = sqlx::query(
        "INSERT INTO diagram (org_id, project_id, code, name) VALUES (1, ?, 'D1', '第一图')",
    )
    .bind(pid)
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_rowid();
    let did2: i64 = sqlx::query(
        "INSERT INTO diagram (org_id, project_id, code, name) VALUES (1, ?, 'D2', '第二图')",
    )
    .bind(pid)
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_rowid();

    let inst1: i64 = sqlx::query(
        "INSERT INTO instrument (org_id, tag, kind, role, sil_target, project_id)
         VALUES (1, 'PT-101', 'PT', 'detector', 'C', ?)",
    )
    .bind(pid)
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_rowid();
    let inst2: i64 = sqlx::query(
        "INSERT INTO instrument (org_id, tag, kind, role, sil_target, project_id)
         VALUES (1, 'XV-101', 'XV', 'final', 'C', ?)",
    )
    .bind(pid)
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_rowid();

    let sif_id: i64 = sqlx::query(
        "INSERT INTO sif (org_id, project_id, code, name, sil_design)
         VALUES (1, ?, 'SIF-101', '反应器超压联锁', 'C')",
    )
    .bind(pid)
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_rowid();

    sqlx::query(
        "INSERT INTO sif_instrument (org_id, sif_id, instrument_id, role, port_index, diagram_id)
         VALUES (1, ?, ?, 'detector', 1, ?)",
    )
    .bind(sif_id)
    .bind(inst1)
    .bind(did1)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO sif_instrument (org_id, sif_id, instrument_id, role, port_index, diagram_id)
         VALUES (1, ?, ?, 'final', 1, ?)",
    )
    .bind(sif_id)
    .bind(inst2)
    .bind(did2)
    .execute(&pool)
    .await
    .unwrap();

    (pool, pid, did1, did2, sif_id, inst2)
}

/// 临时 CSV 路径 helper
fn tmp_csv(name: &str, body: &str) -> std::path::PathBuf {
    let mut f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
    std::io::Write::write_all(&mut f, body.as_bytes()).unwrap();
    let p = f.into_temp_path().keep().unwrap();
    // 重命名以便测试失败时容易定位
    if let Some(parent) = p.parent() {
        let new_path = parent.join(format!("import-test-{name}.csv"));
        std::fs::rename(&p, &new_path).unwrap();
        return new_path;
    }
    p
}

// ===========================================================================
// 原有 7 个测试（保留）
// ===========================================================================

#[tokio::test]
async fn test_db_version_is_one() {
    let pool = open_in_memory().await.unwrap();
    let v: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await
        .unwrap();
    // 迁移当前到 v016（015 LOPA 保护层分析 / 016 检验规程 SOP）
    assert_eq!(v, 16, "v001..v016 全部迁移都该跑过");
}

// ... 其余 6 个原有测试保持原样
#[tokio::test]
async fn test_instrument_tag_unique() {
    let pool = open_in_memory().await.unwrap();
    // M2.9 — 需要先建项目（project_id 现在必填）
    let pid: i64 = sqlx::query(
        "INSERT INTO project (org_id, code, name) VALUES (1, 'PRJ-UQ', '唯一性测试项目')",
    )
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_rowid();
    sqlx::query("INSERT INTO instrument (org_id, tag, kind, role, project_id) VALUES (1, 'PT-1', 'PT', 'detector', ?)")
        .bind(pid)
        .execute(&pool)
        .await
        .unwrap();
    let res = sqlx::query(
        "INSERT INTO instrument (org_id, tag, kind, role, project_id) VALUES (1, 'PT-1', 'PT', 'detector', ?)",
    )
    .bind(pid)
    .execute(&pool)
    .await;
    let err = res.expect_err("should conflict");
    assert!(err
        .as_database_error()
        .map(|e| e.is_unique_violation())
        .unwrap_or(false));
}

#[tokio::test]
async fn test_instrument_role_check_constraint() {
    let pool = open_in_memory().await.unwrap();
    let pid: i64 =
        sqlx::query("INSERT INTO project (org_id, code, name) VALUES (1, 'PRJ-CHK', '约束测试')")
            .execute(&pool)
            .await
            .unwrap()
            .last_insert_rowid();
    let res = sqlx::query(
        "INSERT INTO instrument (org_id, tag, kind, role, project_id) VALUES (1, 'PT-2', 'PT', 'evilrole', ?)",
    )
    .bind(pid)
    .execute(&pool)
    .await;
    assert!(res.is_err(), "role enum should reject invalid value");
}

#[tokio::test]
async fn test_sif_summary_aggregates_cross_diagram() {
    let (pool, _pid, _d1, _d2, sif_id, _) = setup_scenario().await;

    let row: (i64, i64, i64, i64, i64, String, String) = sqlx::query_as(
        "SELECT
            COUNT(DISTINCT CASE WHEN si.role='detector' THEN si.instrument_id END),
            COUNT(DISTINCT CASE WHEN si.role='final'    THEN si.instrument_id END),
            COUNT(DISTINCT CASE WHEN si.role='logic'    THEN si.instrument_id END),
            COUNT(DISTINCT CASE WHEN si.role='aux'      THEN si.instrument_id END),
            COUNT(DISTINCT si.diagram_id),
            GROUP_CONCAT(DISTINCT CASE WHEN si.role='detector' THEN i.tag END),
            GROUP_CONCAT(DISTINCT CASE WHEN si.role='final'    THEN i.tag END)
         FROM sif s
         LEFT JOIN sif_instrument si ON si.sif_id = s.id
         LEFT JOIN instrument i      ON i.id = si.instrument_id
         WHERE s.id = ?
         GROUP BY s.id",
    )
    .bind(sif_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(row.0, 1);
    assert_eq!(row.1, 1);
    assert_eq!(row.2, 0);
    assert_eq!(row.3, 0);
    assert_eq!(row.4, 2);
    assert_eq!(row.5, "PT-101");
    assert_eq!(row.6, "XV-101");
}

#[tokio::test]
async fn test_cascade_delete_instrument_unlinks() {
    let (pool, _pid, _d1, _d2, _sif, inst2) = setup_scenario().await;
    let before: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sif_instrument WHERE instrument_id = ?")
            .bind(inst2)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(before, 1);
    sqlx::query("DELETE FROM instrument WHERE id = ?")
        .bind(inst2)
        .execute(&pool)
        .await
        .unwrap();
    let after: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sif_instrument WHERE instrument_id = ?")
            .bind(inst2)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(after, 0);
}

#[tokio::test]
async fn test_cascade_delete_project() {
    let (pool, pid, _d1, _d2, _sif, _inst2) = setup_scenario().await;
    sqlx::query("DELETE FROM project WHERE id = ?")
        .bind(pid)
        .execute(&pool)
        .await
        .unwrap();
    let n_diagram: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM diagram")
        .fetch_one(&pool)
        .await
        .unwrap();
    let n_sif: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sif")
        .fetch_one(&pool)
        .await
        .unwrap();
    let n_link: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sif_instrument")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n_diagram, 0);
    assert_eq!(n_sif, 0);
    assert_eq!(n_link, 0);
}

#[tokio::test]
async fn test_link_unlink_via_role() {
    let (pool, _pid, _d1, d2, sif_id, inst2) = setup_scenario().await;
    sqlx::query(
        "INSERT INTO sif_instrument (org_id, sif_id, instrument_id, role, port_index, diagram_id)
         VALUES (1, ?, ?, 'aux', 0, ?)",
    )
    .bind(sif_id)
    .bind(inst2)
    .bind(d2)
    .execute(&pool)
    .await
    .unwrap();
    let row = sqlx::query("SELECT id, role FROM sif_instrument WHERE sif_id=? AND instrument_id=?")
        .bind(sif_id)
        .bind(inst2)
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(row.len(), 2);
    let link_id: i64 = row[1].get(0);
    sqlx::query("DELETE FROM sif_instrument WHERE id = ?")
        .bind(link_id)
        .execute(&pool)
        .await
        .unwrap();
    let row2 = sqlx::query(
        "SELECT id FROM sif_instrument WHERE sif_id=? AND instrument_id=? AND role='aux'",
    )
    .bind(sif_id)
    .bind(inst2)
    .fetch_one(&pool)
    .await;
    assert!(row2.is_err());
}

// ===========================================================================
// M2.1 — 仪表批量导入 5 例
// ===========================================================================

#[tokio::test]
async fn test_csv_import_skip_all_new() {
    let pool = open_in_memory().await.unwrap();

    // M2.9 — 仪表必须挂项目
    let pid: i64 = sqlx::query(
        "INSERT INTO project (org_id, code, name) VALUES (1, 'PRJ-IMP-S', 'skip 导入测试')",
    )
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_rowid();

    // 5 行全新 tag
    let csv = "tag,kind,role,unit,range_min,range_max\n\
               PT-201,PT,detector,kPa,0,500\n\
               TT-202,TT,detector,°C,0,300\n\
               XV-301,XV,final,,,\n\
               BYP-401,BYP,aux,,,\n\
               LSL-501,LSL,detector,%,0,100\n";
    let path = tmp_csv("skip-all-new", csv);
    let sheet = parse_file(&path).unwrap();
    assert_eq!(sheet.rows.len(), 5);
    assert_eq!(sheet.headers.len(), 6);

    // 验证列推断：tag/kind/role/unit → text；range_min/max → number
    assert_eq!(sheet.headers[0].kind, sif_studio_lib::import::ColKind::Text);
    assert_eq!(
        sheet.headers[4].kind,
        sif_studio_lib::import::ColKind::Number
    );

    // 默认 mapping（按列名子串匹配）
    assert_eq!(sheet.suggested_mapping[0].target.as_deref(), Some("tag"));
    assert_eq!(sheet.suggested_mapping[2].target.as_deref(), Some("role"));

    let input = CommitImportInput {
        sheet,
        mapping: vec![
            MappingEntry {
                column: 0,
                target: "tag".into(),
            },
            MappingEntry {
                column: 1,
                target: "kind".into(),
            },
            MappingEntry {
                column: 2,
                target: "role".into(),
            },
            MappingEntry {
                column: 3,
                target: "unit".into(),
            },
            MappingEntry {
                column: 4,
                target: "range_min".into(),
            },
            MappingEntry {
                column: 5,
                target: "range_max".into(),
            },
        ],
        on_conflict: "skip".into(),
        actor: Some("tester".into()),
        project_id: pid,
    };

    let result = commit_import_instruments_inner(&pool, ORG, ACTOR, input)
        .await
        .unwrap();
    assert_eq!(result.inserted, 5);
    assert_eq!(result.updated, 0);
    assert_eq!(result.skipped, 0);
    assert_eq!(result.failed, 0);
    assert!(result.batch_id > 0);

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM instrument")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 5);

    // 验证 audit_log：1 个 batch + 5 个 per-row
    let n_audit: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_log")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n_audit, 7, "1 batch + 5 rows + 1 import_done");

    // 验证 batch actor 是 tester
    let actor: String = sqlx::query_scalar(
        "SELECT actor FROM audit_log WHERE action='import' AND target_table='instrument_batch'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(actor, "tester");

    std::fs::remove_file(&path).ok();
}

#[tokio::test]
async fn test_csv_import_overwrite_existing() {
    let pool = open_in_memory().await.unwrap();

    // M2.9 — 仪表必须挂项目
    let pid: i64 =
        sqlx::query("INSERT INTO project (org_id, code, name) VALUES (1, 'PRJ-OW', '覆盖测试')")
            .execute(&pool)
            .await
            .unwrap()
            .last_insert_rowid();

    // 先插 PT-201 (setpoint=NULL)
    sqlx::query(
        "INSERT INTO instrument (org_id, tag, kind, role, setpoint, project_id) VALUES (1, 'PT-201', 'PT', 'detector', 100.0, ?)",
    )
    .bind(pid)
    .execute(&pool)
    .await
    .unwrap();

    // 再用 overwrite 策略导同一 tag，新 setpoint=250
    let csv = "tag,kind,role,setpoint,unit\nPT-201,PT,detector,250,kPa\n";
    let path = tmp_csv("overwrite", csv);
    let sheet = parse_file(&path).unwrap();
    let input = CommitImportInput {
        sheet,
        mapping: vec![
            MappingEntry {
                column: 0,
                target: "tag".into(),
            },
            MappingEntry {
                column: 1,
                target: "kind".into(),
            },
            MappingEntry {
                column: 2,
                target: "role".into(),
            },
            MappingEntry {
                column: 3,
                target: "setpoint".into(),
            },
            MappingEntry {
                column: 4,
                target: "unit".into(),
            },
        ],
        on_conflict: "overwrite".into(),
        actor: None,
        project_id: pid,
    };

    let result = commit_import_instruments_inner(&pool, ORG, ACTOR, input)
        .await
        .unwrap();
    assert_eq!(result.inserted, 0, "已存在 → overwrite 不算 insert");
    assert_eq!(result.updated, 1);
    assert_eq!(result.skipped, 0);

    let setpoint: f64 =
        sqlx::query_scalar("SELECT setpoint FROM instrument WHERE project_id=? AND tag='PT-201'")
            .bind(pid)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(setpoint, 250.0, "setpoint 应被覆盖");

    let unit: String =
        sqlx::query_scalar("SELECT unit FROM instrument WHERE project_id=? AND tag='PT-201'")
            .bind(pid)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(unit, "kPa");

    std::fs::remove_file(&path).ok();
}

#[tokio::test]
async fn test_csv_import_with_suffix() {
    let pool = open_in_memory().await.unwrap();

    // M2.9 — 仪表必须挂项目
    let pid: i64 =
        sqlx::query("INSERT INTO project (org_id, code, name) VALUES (1, 'PRJ-SFX', '后缀测试')")
            .execute(&pool)
            .await
            .unwrap()
            .last_insert_rowid();

    sqlx::query(
        "INSERT INTO instrument (org_id, tag, kind, role, project_id) VALUES (1, 'PT-201', 'PT', 'detector', ?)",
    )
    .bind(pid)
    .execute(&pool)
    .await
    .unwrap();

    let csv = "tag,kind,role\nPT-201,PT,detector\nPT-201,TT,detector\nPT-201,XV,final\n";
    let path = tmp_csv("suffix", csv);
    let sheet = parse_file(&path).unwrap();
    let input = CommitImportInput {
        sheet,
        mapping: vec![
            MappingEntry {
                column: 0,
                target: "tag".into(),
            },
            MappingEntry {
                column: 1,
                target: "kind".into(),
            },
            MappingEntry {
                column: 2,
                target: "role".into(),
            },
        ],
        on_conflict: "create_with_suffix".into(),
        actor: None,
        project_id: pid,
    };

    let result = commit_import_instruments_inner(&pool, ORG, ACTOR, input)
        .await
        .unwrap();
    assert_eq!(result.inserted, 3, "3 个新行：PT-201_1, PT-201_2, PT-201_3");
    assert_eq!(result.skipped, 0);

    let tags: Vec<String> = sqlx::query_scalar("SELECT tag FROM instrument ORDER BY tag")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(tags, vec!["PT-201", "PT-201_1", "PT-201_2", "PT-201_3"]);

    std::fs::remove_file(&path).ok();
}

#[tokio::test]
async fn test_csv_import_invalid_role_fails_one_row() {
    let pool = open_in_memory().await.unwrap();

    // M2.9 — 项目 ID
    let pid: i64 =
        sqlx::query("INSERT INTO project (org_id, code, name) VALUES (1, 'PRJ-INV', '角色非法')")
            .execute(&pool)
            .await
            .unwrap()
            .last_insert_rowid();

    let csv = "tag,kind,role\nPT-A,PT,detector\nPT-B,PT,badrole\nPT-C,PT,final\n";
    let path = tmp_csv("invalid-role", csv);
    let sheet = parse_file(&path).unwrap();
    let input = CommitImportInput {
        sheet,
        mapping: vec![
            MappingEntry {
                column: 0,
                target: "tag".into(),
            },
            MappingEntry {
                column: 1,
                target: "kind".into(),
            },
            MappingEntry {
                column: 2,
                target: "role".into(),
            },
        ],
        on_conflict: "skip".into(),
        actor: None,
        project_id: pid,
    };

    let result = commit_import_instruments_inner(&pool, ORG, ACTOR, input)
        .await
        .unwrap();
    assert_eq!(result.inserted, 2, "PT-A + PT-C 入库");
    assert_eq!(result.failed, 1, "PT-B 因为 badrole 失败");
    assert_eq!(result.errors.len(), 1);
    assert_eq!(
        result.errors[0].row, 3,
        "PT-B 在第 3 行（1 表头 + 1 PT-A + 1 PT-B）"
    );
    assert!(result.errors[0].reason.contains("badrole"));

    // 验证 PT-A 和 PT-C 真的入库了
    let tags: Vec<String> = sqlx::query_scalar("SELECT tag FROM instrument ORDER BY tag")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(tags, vec!["PT-A", "PT-C"]);

    std::fs::remove_file(&path).ok();
}

#[tokio::test]
async fn test_csv_import_audit_trail_complete() {
    let pool = open_in_memory().await.unwrap();

    // M2.9 — 项目 ID
    let pid: i64 =
        sqlx::query("INSERT INTO project (org_id, code, name) VALUES (1, 'PRJ-AUD', '审计测试')")
            .execute(&pool)
            .await
            .unwrap()
            .last_insert_rowid();

    let csv = "tag,kind,role\nPT-X,PT,detector\nPT-Y,TT,final\n";
    let path = tmp_csv("audit", csv);
    let sheet = parse_file(&path).unwrap();
    let input = CommitImportInput {
        sheet,
        mapping: vec![
            MappingEntry {
                column: 0,
                target: "tag".into(),
            },
            MappingEntry {
                column: 1,
                target: "kind".into(),
            },
            MappingEntry {
                column: 2,
                target: "role".into(),
            },
        ],
        on_conflict: "skip".into(),
        actor: Some("alice@company.com".into()),
        project_id: pid,
    };

    // B 阶段 actor 由服务端会话决定；此处显式传具名 actor 以断言审计留痕
    let result = commit_import_instruments_inner(&pool, ORG, "alice@company.com", input)
        .await
        .unwrap();

    // 整批 audit
    let batch: (String, String, String) = sqlx::query_as(
        "SELECT actor, action, target_table FROM audit_log
         WHERE action='import' AND target_table='instrument_batch'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(batch.0, "alice@company.com");
    assert_eq!(batch.1, "import");
    assert_eq!(batch.2, "instrument_batch");

    // 每行 create audit
    let per_row: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_log WHERE action='create' AND target_table='instrument'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(per_row, 2);

    // import_done 汇总
    let done: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_log WHERE action='import_done'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(done, 1);

    // 检查 payload_json 装的是元数据（文件名/格式/策略/总行数）
    let payload: String = sqlx::query_scalar(
        "SELECT payload_json FROM audit_log
         WHERE action='import' AND target_table='instrument_batch'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        payload.contains("audit"),
        "payload 应含文件名（不含 .csv 后缀时仍然含 'audit' 子串）"
    );
    assert!(payload.contains("onConflict"));
    assert!(payload.contains("totalRows"));

    // 避免未使用警告
    let _ = result.batch_id;

    std::fs::remove_file(&path).ok();
}

#[tokio::test]
async fn test_csv_import_rejects_missing_tag_mapping() {
    let pool = open_in_memory().await.unwrap();

    // M2.9 — 项目 ID（虽然会被前置校验拦截，但结构要求有 project_id）
    let pid: i64 = sqlx::query(
        "INSERT INTO project (org_id, code, name) VALUES (1, 'PRJ-NTM', '无 tag mapping')",
    )
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_rowid();

    let csv = "name,kind\nfoo,PT\n";
    let path = tmp_csv("no-tag", csv);
    let sheet = parse_file(&path).unwrap();
    let input = CommitImportInput {
        sheet,
        mapping: vec![
            MappingEntry {
                column: 0,
                target: "service".into(),
            },
            MappingEntry {
                column: 1,
                target: "kind".into(),
            },
        ],
        on_conflict: "skip".into(),
        actor: None,
        project_id: pid,
    };

    let err = commit_import_instruments_inner(&pool, ORG, ACTOR, input)
        .await
        .expect_err("应拒绝无 tag mapping");
    assert!(err.to_string().contains("tag"));

    std::fs::remove_file(&path).ok();
}

#[tokio::test]
async fn test_csv_import_max_rows_enforced() {
    use sif_studio_lib::commands::imports::commit_import_instruments_inner;
    let pool = open_in_memory().await.unwrap();

    // M2.9 — 项目 ID
    let pid: i64 =
        sqlx::query("INSERT INTO project (org_id, code, name) VALUES (1, 'PRJ-MAX', '超大文件')")
            .execute(&pool)
            .await
            .unwrap()
            .last_insert_rowid();

    // 构造一个 50,001 行的 sheet（实际 row 不超过 50001）
    let mut rows: Vec<Vec<String>> = Vec::with_capacity(50_001);
    for i in 0..50_001 {
        rows.push(vec![format!("PT-{i}"), "PT".into(), "detector".into()]);
    }
    let sheet = ParsedSheet {
        source_name: "huge.csv".into(),
        format: "csv".into(),
        total_rows: 50_002,
        headers: vec![],
        preview_rows: vec![],
        suggested_mapping: vec![],
        rows,
    };
    let input = CommitImportInput {
        sheet,
        mapping: vec![
            MappingEntry {
                column: 0,
                target: "tag".into(),
            },
            MappingEntry {
                column: 1,
                target: "kind".into(),
            },
            MappingEntry {
                column: 2,
                target: "role".into(),
            },
        ],
        on_conflict: "skip".into(),
        actor: None,
        project_id: pid,
    };

    let err = commit_import_instruments_inner(&pool, ORG, ACTOR, input)
        .await
        .expect_err("应拒绝超大文件");
    assert!(err.to_string().contains("too many rows"));
}

// ===========================================================================
// M2.2 — 旁路授权台账（IEC 61511-1 §11.5.2）
//
// 共享 helper：1 项目 + 1 SIF
// ===========================================================================

async fn setup_bypass_scenario() -> (Pool<Sqlite>, i64, i64) {
    let pool = open_in_memory().await.expect("open");

    let pid: i64 =
        sqlx::query("INSERT INTO project (org_id, code, name) VALUES (1, 'PRJ-B', '旁路测试')")
            .execute(&pool)
            .await
            .unwrap()
            .last_insert_rowid();
    let sif_id: i64 = sqlx::query(
        "INSERT INTO sif (org_id, project_id, code, name, sil_design)
         VALUES (1, ?, 'SIF-201', '反应器联锁', 'C')",
    )
    .bind(pid)
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_rowid();

    (pool, pid, sif_id)
}

/// 构造未来 N 小时的 ISO 8601（RFC 3339 UTC）
fn future_iso(hours_from_now: i64) -> String {
    use chrono::{Duration, Utc};
    (Utc::now() + Duration::hours(hours_from_now)).to_rfc3339()
}

fn past_iso(hours_ago: i64) -> String {
    use chrono::{Duration, Utc};
    (Utc::now() - Duration::hours(hours_ago)).to_rfc3339()
}

// ---------- 1. 必填校验 ----------
#[tokio::test]
async fn test_create_bypass_validates_required_fields() {
    let (pool, pid, sif) = setup_bypass_scenario().await;

    // approved_by 空
    let r1 = create_bypass_inner(
        &pool,
        ORG,
        "alice",
        BypassInput {
            project_id: pid,
            sif_id: sif,
            bypassed_by: "alice".into(),
            reason: "检修压力变送器".into(),
            planned_restore: future_iso(8),
            permit_no: "WP-001".into(),
            approved_by: "".into(),
        },
    )
    .await;
    assert!(r1.is_err(), "approved_by 为空应被拒绝");
    assert!(r1.unwrap_err().to_string().contains("approved_by"));

    // reason 太短
    let r2 = create_bypass_inner(
        &pool,
        ORG,
        "alice",
        BypassInput {
            project_id: pid,
            sif_id: sif,
            bypassed_by: "alice".into(),
            reason: "修".into(),
            planned_restore: future_iso(8),
            permit_no: "".into(),
            approved_by: "bob".into(),
        },
    )
    .await;
    assert!(r2.is_err(), "reason < 5 字应被拒绝");
    assert!(r2.unwrap_err().to_string().contains("reason"));

    // planned_restore 太近（30 分钟后）
    let r3 = create_bypass_inner(
        &pool,
        ORG,
        "alice",
        BypassInput {
            project_id: pid,
            sif_id: sif,
            bypassed_by: "alice".into(),
            reason: "检修压力变送器".into(),
            planned_restore: future_iso(0),
            permit_no: "".into(),
            approved_by: "bob".into(),
        },
    )
    .await;
    assert!(r3.is_err(), "planned_restore 距今 ≤ 1h 应被拒绝");
    assert!(r3.unwrap_err().to_string().contains("planned_restore"));

    // sif 不存在
    let r4 = create_bypass_inner(
        &pool,
        ORG,
        "alice",
        BypassInput {
            project_id: pid,
            sif_id: 99999,
            bypassed_by: "alice".into(),
            reason: "检修压力变送器".into(),
            planned_restore: future_iso(8),
            permit_no: "".into(),
            approved_by: "bob".into(),
        },
    )
    .await;
    assert!(r4.is_err(), "sif 不存在应被拒绝");

    // sif 不属于该项目
    let other_pid: i64 =
        sqlx::query("INSERT INTO project (org_id, code, name) VALUES (1, 'PRJ-X', '其他')")
            .execute(&pool)
            .await
            .unwrap()
            .last_insert_rowid();
    let r5 = create_bypass_inner(
        &pool,
        ORG,
        "alice",
        BypassInput {
            project_id: other_pid,
            sif_id: sif,
            bypassed_by: "alice".into(),
            reason: "检修压力变送器".into(),
            planned_restore: future_iso(8),
            permit_no: "".into(),
            approved_by: "bob".into(),
        },
    )
    .await;
    assert!(r5.is_err(), "sif 与 project 不匹配应被拒绝");
    assert!(r5.unwrap_err().to_string().contains("belongs to project"));
}

// ---------- 2. 创建成功 + 审计 ----------
#[tokio::test]
async fn test_create_bypass_writes_audit() {
    let (pool, pid, sif) = setup_bypass_scenario().await;

    let created = create_bypass_inner(
        &pool,
        ORG,
        "alice",
        BypassInput {
            project_id: pid,
            sif_id: sif,
            bypassed_by: "alice".into(),
            reason: "反应器 PT-201 变送器送检".into(),
            planned_restore: future_iso(8),
            permit_no: "WP-2026-0913".into(),
            approved_by: "bob".into(),
        },
    )
    .await
    .unwrap();

    assert!(created.id > 0);
    assert_eq!(created.status, "active");
    assert_eq!(created.sif_code, "SIF-201");
    assert_eq!(created.project_code, "PRJ-B");
    assert_eq!(created.bypassed_by, "alice");
    assert_eq!(created.approved_by, "bob");

    let row: (String, String, i64, String) = sqlx::query_as(
        "SELECT actor, action, target_id, payload_json FROM audit_log
         WHERE action='bypass_create'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    // B 阶段：actor 来自登录会话（测试中显式传入 "alice"），不再取 OS 用户
    assert_eq!(row.0, "alice");
    assert_eq!(row.1, "bypass_create");
    assert_eq!(row.2, created.id);
    assert!(row.3.contains("PT-201"));
}

// ---------- 3. restore 写 restored_at ----------
#[tokio::test]
async fn test_restore_bypass_sets_restored_at() {
    let (pool, pid, sif) = setup_bypass_scenario().await;
    let created = create_bypass_inner(
        &pool,
        ORG,
        "alice",
        BypassInput {
            project_id: pid,
            sif_id: sif,
            bypassed_by: "alice".into(),
            reason: "反应器 PT-201 送检".into(),
            planned_restore: future_iso(8),
            permit_no: "".into(),
            approved_by: "bob".into(),
        },
    )
    .await
    .unwrap();

    let restored = restore_bypass_inner(&pool, ORG, "alice", created.id, None)
        .await
        .unwrap();
    assert_eq!(restored.id, created.id);
    assert_eq!(restored.status, "restored");
    assert!(!restored.restored_at.is_empty());

    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_log WHERE action='bypass_restore' AND target_id=?",
    )
    .bind(created.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 1, "恢复必须写一条 bypass_restore 审计");
}

// ---------- 4. 已恢复不可再恢复 ----------
#[tokio::test]
async fn test_cannot_restore_already_restored() {
    let (pool, pid, sif) = setup_bypass_scenario().await;
    let created = create_bypass_inner(
        &pool,
        ORG,
        "alice",
        BypassInput {
            project_id: pid,
            sif_id: sif,
            bypassed_by: "alice".into(),
            reason: "反应器 PT-201 送检".into(),
            planned_restore: future_iso(8),
            permit_no: "".into(),
            approved_by: "bob".into(),
        },
    )
    .await
    .unwrap();

    restore_bypass_inner(&pool, ORG, "alice", created.id, None)
        .await
        .unwrap();

    let r2 = restore_bypass_inner(&pool, ORG, "alice", created.id, None).await;
    assert!(r2.is_err(), "已恢复不应再恢复");
    assert!(r2.unwrap_err().to_string().contains("already restored"));
}

// ---------- 5. list_bypasses 状态过滤 ----------
#[tokio::test]
async fn test_list_bypasses_filters_by_status() {
    let (pool, pid, sif) = setup_bypass_scenario().await;

    let _active = create_bypass_inner(
        &pool,
        ORG,
        "alice",
        BypassInput {
            project_id: pid,
            sif_id: sif,
            bypassed_by: "alice".into(),
            reason: "PT-201 送检".into(),
            planned_restore: future_iso(48),
            permit_no: "".into(),
            approved_by: "bob".into(),
        },
    )
    .await
    .unwrap();

    // 制造一条已逾期（写入 past planned_restore 绕过校验）
    sqlx::query(
        "INSERT INTO bypass_record
           (org_id, project_id, sif_id, bypassed_by, reason, planned_restore, approved_by)
         VALUES (1, ?, ?, 'carol', 'PT-301 故障旁路', '2020-01-01T00:00:00Z', 'dave')",
    )
    .bind(pid)
    .bind(sif)
    .execute(&pool)
    .await
    .unwrap();

    let _active2 = create_bypass_inner(
        &pool,
        ORG,
        "alice",
        BypassInput {
            project_id: pid,
            sif_id: sif,
            bypassed_by: "erin".into(),
            reason: "XV-101 阀检修".into(),
            planned_restore: future_iso(72),
            permit_no: "".into(),
            approved_by: "frank".into(),
        },
    )
    .await
    .unwrap();

    let all = list_bypasses_inner(&pool, ORG, None, Some("all"))
        .await
        .unwrap();
    assert_eq!(all.len(), 3);

    let active = list_bypasses_inner(&pool, ORG, None, Some("active"))
        .await
        .unwrap();
    assert_eq!(active.len(), 2);
    for b in &active {
        assert_eq!(b.status, "active");
        assert!(b.hours_to_restore > 0.0);
    }

    let overdue = list_bypasses_inner(&pool, ORG, None, Some("overdue"))
        .await
        .unwrap();
    assert_eq!(overdue.len(), 1);
    assert_eq!(overdue[0].status, "overdue");
    assert!(overdue[0].hours_to_restore < 0.0);

    let restored_list = list_bypasses_inner(&pool, ORG, None, Some("restored"))
        .await
        .unwrap();
    assert_eq!(restored_list.len(), 0);

    let proj_only = list_bypasses_inner(&pool, ORG, Some(pid), Some("all"))
        .await
        .unwrap();
    assert_eq!(proj_only.len(), 3);

    let bad = list_bypasses_inner(&pool, ORG, None, Some("bogus")).await;
    assert!(bad.is_err());
}

// ---------- 6. update_bypass 写审计 + 已恢复不可改 ----------
#[tokio::test]
async fn test_update_bypass_writes_audit_and_blocks_restored() {
    let (pool, pid, sif) = setup_bypass_scenario().await;
    let created = create_bypass_inner(
        &pool,
        ORG,
        "alice",
        BypassInput {
            project_id: pid,
            sif_id: sif,
            bypassed_by: "alice".into(),
            reason: "反应器 PT-201 送检".into(),
            planned_restore: future_iso(8),
            permit_no: "".into(),
            approved_by: "bob".into(),
        },
    )
    .await
    .unwrap();

    let updated = update_bypass_inner(
        &pool,
        ORG,
        "alice",
        created.id,
        BypassUpdate {
            reason: Some("改：PT-201 + PT-301 同时送检".into()),
            permit_no: Some("WP-2026-0914".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(updated.reason.contains("PT-301"));
    assert_eq!(updated.permit_no, "WP-2026-0914");
    assert_eq!(updated.bypassed_by, "alice", "未提供则保留旧值");

    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_log WHERE action='bypass_update'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);

    restore_bypass_inner(&pool, ORG, "alice", created.id, None)
        .await
        .unwrap();
    let r = update_bypass_inner(
        &pool,
        ORG,
        "alice",
        created.id,
        BypassUpdate {
            reason: Some("改不了的".into()),
            ..Default::default()
        },
    )
    .await;
    assert!(r.is_err());
    assert!(r.unwrap_err().to_string().contains("already restored"));
}

// ---------- 7. CASCADE：删 SIF 清旁路 + delete_bypass 行为 ----------
#[tokio::test]
async fn test_cascade_delete_sif_removes_bypasses() {
    let (pool, pid, sif) = setup_bypass_scenario().await;
    let created = create_bypass_inner(
        &pool,
        ORG,
        "alice",
        BypassInput {
            project_id: pid,
            sif_id: sif,
            bypassed_by: "alice".into(),
            reason: "反应器 PT-201 送检".into(),
            planned_restore: future_iso(8),
            permit_no: "".into(),
            approved_by: "bob".into(),
        },
    )
    .await
    .unwrap();

    sqlx::query("DELETE FROM sif WHERE id = ?")
        .bind(sif)
        .execute(&pool)
        .await
        .unwrap();

    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM bypass_record")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0, "CASCADE 应自动清掉 bypass_record");

    let r = delete_bypass_inner(&pool, ORG, "alice", created.id).await;
    assert!(r.is_err(), "不存在的 bypass_id 应 NotFound");
}

// ---------- 8. count_overdue_bypasses ----------
#[tokio::test]
async fn test_count_overdue_returns_zero_on_empty() {
    let pool = open_in_memory().await.unwrap();
    let st = count_overdue_bypasses_inner(&pool, ORG).await.unwrap();
    assert_eq!(st.count, 0, "空库不应有 overdue");
    assert!(st.oldest_overdue_hours.is_none(), "空库无最早逾期");
}

#[tokio::test]
async fn test_count_overdue_seeds_via_sql() {
    // 走 create_bypass 的 planned_restore 必须 > now + 1h 校验，所以造 overdue 数据
    // 只能直接 SQL 插（bypass 字段：restored_at 默认 '' + planned_restore 设成过去时间）
    let pool = open_in_memory().await.unwrap();
    let pid: i64 =
        sqlx::query("INSERT INTO project (org_id, code, name) VALUES (1, 'PRJ-O', '逾期测试项目')")
            .execute(&pool)
            .await
            .unwrap()
            .last_insert_rowid();
    let sid: i64 = sqlx::query("INSERT INTO sif (org_id, project_id, code, name, sil_design, sil_verified, demand_mode) VALUES (1, ?, 'SIF-O', 'SIF-O 名称', 'B', 'B', 'low')")
        .bind(pid).execute(&pool).await.unwrap().last_insert_rowid();

    // 4 条样本：active / overdue / overdue（更久）/ restored-but-past
    // ① 未来到期（active）
    sqlx::query("INSERT INTO bypass_record (org_id, project_id, sif_id, bypassed_by, reason, planned_restore, approved_by) VALUES (1, ?, ?, 'alice', '检修变送器 A 通道', ?, 'bob')")
        .bind(pid).bind(sid).bind(future_iso(8))
        .execute(&pool).await.unwrap();
    // ② 逾期 5 小时（overdue）
    sqlx::query("INSERT INTO bypass_record (org_id, project_id, sif_id, bypassed_by, reason, planned_restore, approved_by) VALUES (1, ?, ?, 'alice', '检修变送器 B 通道', ?, 'bob')")
        .bind(pid).bind(sid).bind(past_iso(5))
        .execute(&pool).await.unwrap();
    // ③ 逾期 36 小时（overdue，最久）
    sqlx::query("INSERT INTO bypass_record (org_id, project_id, sif_id, bypassed_by, reason, planned_restore, approved_by) VALUES (1, ?, ?, 'alice', '在线调校流量计 FT-301', ?, 'bob')")
        .bind(pid).bind(sid).bind(past_iso(36))
        .execute(&pool).await.unwrap();
    // ④ 已恢复（哪怕 planned 在过去也不算 overdue）
    sqlx::query("INSERT INTO bypass_record (org_id, project_id, sif_id, bypassed_by, reason, planned_restore, approved_by, restored_at) VALUES (1, ?, ?, 'alice', '紧急检修压力变送器', ?, 'bob', ?)")
        .bind(pid).bind(sid).bind(past_iso(2)).bind(future_iso(2))
        .execute(&pool).await.unwrap();

    let st = count_overdue_bypasses_inner(&pool, ORG).await.unwrap();
    assert_eq!(
        st.count, 2,
        "应只数到 2 条 overdue（③ + ②），不算 active 与已恢复"
    );
    let oldest = st.oldest_overdue_hours.expect("应有最久逾期值");
    assert!(
        (35.5..=36.5).contains(&oldest),
        "最久逾期应在 35.5~36.5h 之间（MAX），实得 {oldest}"
    );
}

#[tokio::test]
async fn test_count_overdue_ignores_restored_even_when_past() {
    let pool = open_in_memory().await.unwrap();
    let pid: i64 =
        sqlx::query("INSERT INTO project (org_id, code, name) VALUES (1, 'PRJ-R', '恢复测试')")
            .execute(&pool)
            .await
            .unwrap()
            .last_insert_rowid();
    let sid: i64 = sqlx::query("INSERT INTO sif (org_id, project_id, code, name, sil_design, sil_verified, demand_mode) VALUES (1, ?, 'SIF-R', 'SIF-R 名称', 'C', 'C', 'low')")
        .bind(pid).execute(&pool).await.unwrap().last_insert_rowid();

    // 仅 1 条：planned 在过去 + restored_at 已填 → 不算 overdue
    sqlx::query("INSERT INTO bypass_record (org_id, project_id, sif_id, bypassed_by, reason, planned_restore, approved_by, restored_at) VALUES (1, ?, ?, 'alice', '已完成恢复的旁路测试', ?, 'bob', ?)")
        .bind(pid).bind(sid).bind(past_iso(10)).bind(past_iso(2))
        .execute(&pool).await.unwrap();

    let st = count_overdue_bypasses_inner(&pool, ORG).await.unwrap();
    assert_eq!(st.count, 0, "已恢复的不应计入 overdue");
    assert!(st.oldest_overdue_hours.is_none());
}

// ===========================================================================
// M2.3 — 仪表修改历史（字段级 diff）
//
// payload 协议：{before, after, fieldsChanged}
//   - create → before=null, after=<row>, fieldsChanged=["*"]
//   - update → before=<row>, after=<row>, fieldsChanged=[changed names]
//   - delete → before=<row>, after=null, fieldsChanged=["*"]
// ===========================================================================

/// 构造一个完整 InstrumentInput（Rust 结构体需给全字段）
fn inst_input(tag: &str, setpoint: Option<f64>, service: &str, unit: &str) -> InstrumentInput {
    InstrumentInput {
        tag: tag.into(),
        service: service.into(),
        kind: "PT".into(),
        role: "detector".into(),
        psv_id: "PSV-1101".into(),
        manufacturer: "Rosemount".into(),
        model: "3051CD".into(),
        range_min: Some(0.0),
        range_max: Some(100.0),
        unit: unit.into(),
        setpoint,
        sil_target: "B".into(),
        proof_interval: 12,
        lambda_du: 0.0,
        lambda_dd: 0.0,
        lambda_su: 0.0,
        lambda_sd: 0.0,
        sff: 0.0,
        pt_coverage: 1.0,
        hft: 0,
        equipment_type: "type_b".into(),
        installed_at: "2025-06-12".into(),
        notes: String::new(),
        // M2.9 — 测试 helper 占位 project_id=1
        project_id: 1,
    }
}

/// inst_input 固定挂 project_id=1。003 迁移在全新库中预置了 PRJ-LEGACY（首个
/// project，id=1），004 又把其 org_id 回填为 1，因此该项目天然可用。
/// 此 helper 仅断言这一夹具前提，不再插入新项目。
async fn seed_default_project(pool: &Pool<Sqlite>) {
    let row: (i64, i64, String) =
        sqlx::query_as("SELECT id, org_id, code FROM project WHERE id = 1")
            .fetch_one(pool)
            .await
            .expect("003 迁移应预置 id=1 的 PRJ-LEGACY 项目");
    assert_eq!(row.0, 1);
    assert_eq!(row.1, ORG, "004 应把存量项目回填到 org 1");
    assert_eq!(row.2, "PRJ-LEGACY");
}

/// 读取某 instrument 的 audit payload（JSON 字符串）
async fn read_audit_payload(pool: &Pool<Sqlite>, action: &str, target_id: i64) -> String {
    sqlx::query_scalar::<_, String>(
        "SELECT payload_json FROM audit_log
         WHERE action = ? AND target_table = 'instrument' AND target_id = ?",
    )
    .bind(action)
    .bind(target_id)
    .fetch_one(pool)
    .await
    .expect("audit row should exist")
}

// ---------- 1. create 写 audit + 完整快照 ----------
#[tokio::test]
async fn test_create_instrument_writes_audit_with_snapshot() {
    let pool = open_in_memory().await.unwrap();
    seed_default_project(&pool).await;
    let created = create_instrument_inner(
        &pool,
        ORG,
        ACTOR,
        inst_input("PT-201", Some(85.0), "反应器进口压力", "kPa"),
    )
    .await
    .unwrap();
    assert!(created.id > 0);
    assert_eq!(created.tag, "PT-201");

    let payload = read_audit_payload(&pool, "instrument_create", created.id).await;
    let v: serde_json::Value = serde_json::from_str(&payload).unwrap();

    // before 必须是 JSON null
    assert!(
        v["before"].is_null(),
        "create 的 before 应为 null，实得 {}",
        v["before"]
    );
    // after 完整快照（含 tag/服务/量程）
    assert_eq!(v["after"]["tag"], "PT-201");
    assert_eq!(v["after"]["service"], "反应器进口压力");
    assert_eq!(v["after"]["setpoint"], 85.0);
    assert_eq!(v["after"]["unit"], "kPa");
    // fieldsChanged = ["*"]
    assert_eq!(v["fieldsChanged"], serde_json::json!(["*"]));

    // 全局审计只该有 1 条
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_log")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
}

// ---------- 2. update 记录精确字段级 diff ----------
#[tokio::test]
async fn test_update_instrument_records_field_diff() {
    let pool = open_in_memory().await.unwrap();
    seed_default_project(&pool).await;
    // 先建
    let created = create_instrument_inner(
        &pool,
        ORG,
        ACTOR,
        inst_input("PT-202", Some(85.0), "反应器进口压力", "kPa"),
    )
    .await
    .unwrap();

    // 只改 setpoint（85 → 90），其余字段完全一致
    let mut input = inst_input("PT-202", Some(90.0), "反应器进口压力", "kPa");
    input.notes = String::new();
    update_instrument_inner(&pool, ORG, ACTOR, created.id, input)
        .await
        .unwrap();

    let payload = read_audit_payload(&pool, "instrument_update", created.id).await;
    let v: serde_json::Value = serde_json::from_str(&payload).unwrap();

    assert_eq!(
        v["fieldsChanged"],
        serde_json::json!(["setpoint"]),
        "只有 setpoint 变了，实得 {}",
        v["fieldsChanged"]
    );
    assert_eq!(v["before"]["setpoint"], 85.0);
    assert_eq!(v["after"]["setpoint"], 90.0);
    // before / after 都是完整快照
    assert_eq!(v["before"]["tag"], "PT-202");
    assert_eq!(v["after"]["tag"], "PT-202");
}

// ---------- 3. update 多字段 + 中文服务描述 ----------
#[tokio::test]
async fn test_update_records_multiple_changed_fields_sorted() {
    let pool = open_in_memory().await.unwrap();
    seed_default_project(&pool).await;
    let created = create_instrument_inner(
        &pool,
        ORG,
        ACTOR,
        inst_input("PT-203", Some(85.0), "反应器进口压力", "kPa"),
    )
    .await
    .unwrap();

    // 改 3 个：service / unit / setpoint
    let input = inst_input("PT-203", Some(95.0), "反应器 R-201 进口压力", "kPaG");
    update_instrument_inner(&pool, ORG, ACTOR, created.id, input)
        .await
        .unwrap();

    let payload = read_audit_payload(&pool, "instrument_update", created.id).await;
    let v: serde_json::Value = serde_json::from_str(&payload).unwrap();
    // 已排序：service < setpoint < unit
    assert_eq!(
        v["fieldsChanged"],
        serde_json::json!(["service", "setpoint", "unit"]),
        "实得 {}",
        v["fieldsChanged"]
    );
    assert_eq!(v["after"]["service"], "反应器 R-201 进口压力");
}

// ---------- 4. update 无实质变更也写 audit（fieldsChanged=[]） ----------
#[tokio::test]
async fn test_update_no_change_still_writes_audit_with_empty_diff() {
    let pool = open_in_memory().await.unwrap();
    seed_default_project(&pool).await;
    let created = create_instrument_inner(
        &pool,
        ORG,
        ACTOR,
        inst_input("PT-204", Some(85.0), "反应器进口压力", "kPa"),
    )
    .await
    .unwrap();

    // 原样再保存一次
    let input = inst_input("PT-204", Some(85.0), "反应器进口压力", "kPa");
    update_instrument_inner(&pool, ORG, ACTOR, created.id, input)
        .await
        .unwrap();

    let payload = read_audit_payload(&pool, "instrument_update", created.id).await;
    let v: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(
        v["fieldsChanged"],
        serde_json::json!([]),
        "无实质变更 → 空数组（不是 null、不是 ['*']）"
    );

    // 「打开关闭」也留痕：audit 应该有 2 条（create + update）
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_log WHERE target_id = ?")
        .bind(created.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 2, "create + update(no-op) = 2 条");
}

// ---------- 5. delete 写 audit（after=null，before 完整） ----------
#[tokio::test]
async fn test_delete_instrument_writes_audit_with_before_snapshot() {
    let pool = open_in_memory().await.unwrap();
    seed_default_project(&pool).await;
    let created = create_instrument_inner(
        &pool,
        ORG,
        ACTOR,
        inst_input("PT-205", Some(85.0), "待删仪表", "kPa"),
    )
    .await
    .unwrap();

    let n = delete_instrument_inner(&pool, ORG, ACTOR, created.id)
        .await
        .unwrap();
    assert_eq!(n, 1);

    let payload = read_audit_payload(&pool, "instrument_delete", created.id).await;
    let v: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(v["before"]["tag"], "PT-205");
    assert_eq!(v["before"]["service"], "待删仪表");
    assert!(v["after"].is_null(), "delete 的 after 应为 null");
    assert_eq!(v["fieldsChanged"], serde_json::json!(["*"]));

    // 仪表行真的没了
    let remain: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM instrument WHERE id = ?")
        .bind(created.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(remain, 0);
}

// ---------- 6. list_instrument_history：倒序 + 按 target_id 隔离 ----------
#[tokio::test]
async fn test_list_instrument_history_is_desc_and_isolated() {
    let pool = open_in_memory().await.unwrap();
    seed_default_project(&pool).await;

    // A：create → update → update
    let a = create_instrument_inner(
        &pool,
        ORG,
        ACTOR,
        inst_input("PT-301", Some(10.0), "A 服务", "kPa"),
    )
    .await
    .unwrap();
    let in_a2 = inst_input("PT-301", Some(20.0), "A 服务", "kPa");
    update_instrument_inner(&pool, ORG, ACTOR, a.id, in_a2)
        .await
        .unwrap();
    let in_a3 = inst_input("PT-301", Some(30.0), "A 服务改", "kPa");
    update_instrument_inner(&pool, ORG, ACTOR, a.id, in_a3)
        .await
        .unwrap();

    // B：create（不应混入 A 的历史）
    let _b = create_instrument_inner(
        &pool,
        ORG,
        ACTOR,
        inst_input("PT-302", Some(50.0), "B 服务", "kPa"),
    )
    .await
    .unwrap();

    let hist = list_instrument_history_inner(&pool, ORG, a.id, None)
        .await
        .unwrap();
    assert_eq!(hist.len(), 3, "A 应有 3 条（create + 2 update）");
    // 倒序：第一条是最近一次 update
    assert_eq!(hist[0].action, "instrument_update");
    assert_eq!(hist[0].after.as_ref().unwrap()["setpoint"], 30.0);
    assert_eq!(hist[2].action, "instrument_create");
    assert!(
        hist[2].before.is_none(),
        "最早的 create 的 before 应为 None"
    );
    // 所有条目的 target 都是 A
    for h in &hist {
        assert!(h.id > 0);
        assert!(!h.ts.is_empty());
    }

    // limit 生效
    let limited = list_instrument_history_inner(&pool, ORG, a.id, Some(2))
        .await
        .unwrap();
    assert_eq!(limited.len(), 2);

    // 不存在的仪表 → 空列表（不是错误）
    let none = list_instrument_history_inner(&pool, ORG, 99999, None)
        .await
        .unwrap();
    assert!(none.is_empty());
}
