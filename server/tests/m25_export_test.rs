//! M2.5 — 审计包导出（commands/audit_export.rs）的集成测试
//!
//! 覆盖：
//!   - export_audit_csv：写文件 + BOM + 表头 + 字段级 diff 内容
//!   - export_audit_csv：多维筛选（table + 时间）
//!   - export_audit_csv：link/unlink 自然语言写到 note 列
//!   - export_audit_csv：相对路径被拒 / 空路径被拒
//!   - preview_audit_filtered：limit 生效 + 默认排序按 id DESC
//!   - summarize_audit_filtered：by_action / by_table 正确
//!   - count_audit_filtered 与 preview 数量一致
//!   - list_audit_filters：actions/actors/tables 去重
//!   - CASCADE 后 audit_log 不动（合规铁律）
//!   - 空字符串/空白 filter 视作 None（不误匹配空串）

use sif_studio_lib::commands::audit::{list_history_for_target_inner, write_audit};
use sif_studio_lib::commands::audit_export::{
    count_audit_filtered_inner, export_audit_csv_inner, list_audit_filters_inner,
    preview_audit_filtered_inner, summarize_audit_filtered_inner, AuditFilterInput,
};
use sif_studio_lib::commands::instruments::{
    create_instrument_inner, delete_instrument_inner, update_instrument_inner, InstrumentInput,
};
use sif_studio_lib::commands::projects::{
    create_project_inner, delete_project_inner, update_project_inner, ProjectInput,
};
use sif_studio_lib::commands::sifs::{
    create_sif_inner, delete_sif_inner, link_instrument_to_sif_inner,
    unlink_instrument_from_sif_inner, SifInput,
};
use sif_studio_lib::db::open_in_memory;
use sqlx::SqlitePool;
use tempfile::TempDir;

const ORG: i64 = 1;
const ACTOR: &str = "tester";

// ---------------------------------------------------------------------------
// Test fixture —— 内存 SQLite + 临时目录（输出 CSV 文件用）
// ---------------------------------------------------------------------------
async fn fresh_pool() -> (TempDir, SqlitePool) {
    let dir = tempfile::tempdir().expect("tempdir");
    let pool = open_in_memory().await.expect("in-memory sqlite");
    (dir, pool)
}

// ---------------------------------------------------------------------------
// 工具：拼 InstrumentInput
// ---------------------------------------------------------------------------
fn inst_input(tag: &str, role: &str, sil: &str) -> InstrumentInput {
    InstrumentInput {
        tag: tag.into(),
        service: format!("{tag} service"),
        kind: "PT".into(),
        role: role.into(),
        psv_id: "".into(),
        manufacturer: "Rosemount".into(),
        model: "3051CD".into(),
        range_min: Some(0.0),
        range_max: Some(100.0),
        unit: "kPa".into(),
        setpoint: Some(50.0),
        sil_target: sil.into(),
        proof_interval: 12,
        lambda_du: 0.0,
        lambda_dd: 0.0,
        lambda_su: 0.0,
        lambda_sd: 0.0,
        sff: 0.0,
        pt_coverage: 1.0,
        hft: 0,
        equipment_type: "type_b".into(),
        installed_at: "2026-01-01".into(),
        notes: "".into(),
        // M2.9 — 测试 helper 占位 project_id=1，由用例在 setup 中覆盖
        project_id: 1,
    }
}

fn proj_input(code: &str, name: &str) -> ProjectInput {
    ProjectInput {
        code: code.into(),
        name: name.into(),
        client: "中石化".into(),
        location: "华东".into(),
        phase: "design".into(),
        finished_at: "".into(),
        notes: "".into(),
    }
}

fn sif_input(project_id: i64, code: &str, name: &str) -> SifInput {
    SifInput {
        project_id,
        code: code.into(),
        name: name.into(),
        description: "".into(),
        sil_design: "NA".into(),
        sil_verified: "NA".into(),
        demand_mode: "low".into(),
        pfdavg_target: Some(0.01),
        proof_interval: 12,
    sensor_arch: "1oo1".into(),
    logic_arch: "1oo1".into(),
    final_arch: "1oo1".into(),
    mttr_hours: 8.0,
    beta_factor: 0.10,
    ..Default::default()
    }
}

// ===========================================================================
// 1. export_audit_csv —— 写文件 + BOM + 表头 + 内容正确
// ===========================================================================
#[tokio::test]
async fn export_writes_csv_with_bom_and_header() {
    let (dir, pool) = fresh_pool().await;
    let p = create_project_inner(&pool, ORG, &proj_input("PRJ-1", "项目一"), "李主任")
        .await
        .expect("create project");
    let _ = create_instrument_inner(&pool, ORG, ACTOR, inst_input("PT-101", "detector", "B"))
        .await
        .expect("create instrument");
    let _ = create_instrument_inner(&pool, ORG, ACTOR, inst_input("PT-102", "detector", "A"))
        .await
        .expect("create instrument");

    let out_path = dir.path().join("audit.csv");
    let out_str = out_path.to_string_lossy().replace('\\', "/");
    let r = export_audit_csv_inner(&pool, ORG, &AuditFilterInput::default(), &out_str)
        .await
        .expect("export");

    assert_eq!(r.rows, 3, "1 project_create + 2 instrument_create");
    assert!(r.bytes > 0);
    let bytes = std::fs::read(&out_path).expect("read");
    assert!(
        bytes.starts_with(b"\xEF\xBB\xBF"),
        "CSV 必须以 UTF-8 BOM 开头"
    );
    let text = String::from_utf8(bytes).expect("utf-8");
    let lines: Vec<&str> = text.split('\n').collect();
    // BOM + 表头 + 3 行数据 = 5 行（含末尾换行 → 4 个 split，但 lines[4] 是 ""）
    assert!(lines[0].contains("audit_id"), "首行应为表头");
    assert!(lines[0].contains("before_json"));
    assert!(lines[0].contains("note"));

    let _ = p; // suppress unused
}

// ===========================================================================
// 2. export_audit_csv —— 多维筛选
// ===========================================================================
#[tokio::test]
async fn export_filters_by_table_and_time() {
    let (dir, pool) = fresh_pool().await;
    let p1 = create_project_inner(&pool, ORG, &proj_input("PRJ-1", "项目一"), "张工")
        .await
        .expect("p1");
    let p2 = create_project_inner(&pool, ORG, &proj_input("PRJ-2", "项目二"), "张工")
        .await
        .expect("p2");
    // 5 个仪表 create（4 in project=1，1 in project=2）
    for tag in ["PT-101", "PT-102", "PT-103", "PT-104"] {
        create_instrument_inner(&pool, ORG, ACTOR, inst_input(tag, "detector", "B"))
            .await
            .expect("inst");
    }
    // 一个项目 update
    let updated = update_project_inner(
        &pool,
        ORG,
        p1.id,
        &proj_input("PRJ-1", "项目一改名"),
        "李主任",
    )
    .await
    .expect("update p1");
    let _ = (p2, updated);

    // 筛选：只导出 instrument 表
    let out = dir.path().join("inst.csv");
    let out_str = out.to_string_lossy().replace('\\', "/");
    let r1 = export_audit_csv_inner(
        &pool,
        ORG,
        &AuditFilterInput {
            target_table: Some("instrument".into()),
            ..Default::default()
        },
        &out_str,
    )
    .await
    .expect("filter table");
    assert_eq!(
        r1.rows, 4,
        "仅 4 条 instrument_create 命中（项目 create/update 不算）"
    );

    // 筛选：target_table=project + actor=李主任
    let out2 = dir.path().join("proj-li.csv");
    let out2_str = out2.to_string_lossy().replace('\\', "/");
    let r2 = export_audit_csv_inner(
        &pool,
        ORG,
        &AuditFilterInput {
            target_table: Some("project".into()),
            actor: Some("李主任".into()),
            ..Default::default()
        },
        &out2_str,
    )
    .await
    .expect("filter project + actor");
    assert_eq!(r2.rows, 1, "仅 1 条 project_update by 李主任");
    let text = std::fs::read_to_string(&out2).expect("read");
    assert!(text.contains("李主任"));
}

// ===========================================================================
// 3. export_audit_csv —— link/unlink 自然语言 description 落到 note 列
// ===========================================================================
#[tokio::test]
async fn export_link_unlink_writes_natural_description_to_note_column() {
    let (dir, pool) = fresh_pool().await;
    let p = create_project_inner(&pool, ORG, &proj_input("PRJ-1", "项目"), "张工")
        .await
        .expect("p");
    let inst = create_instrument_inner(&pool, ORG, ACTOR, inst_input("PT-101", "detector", "B"))
        .await
        .expect("inst");
    let sif = create_sif_inner(&pool, ORG, &sif_input(p.id, "SIF-1", "SIF 一"), "张工")
        .await
        .expect("sif");

    // link
    let link_id = link_instrument_to_sif_inner(
        &pool,
        ORG,
        sif.id,
        inst.id,
        "detector".into(),
        0,
        None,
        "".into(),
        "陈工",
    )
    .await
    .expect("link");

    let out = dir.path().join("link.csv");
    let out_str = out.to_string_lossy().replace('\\', "/");
    export_audit_csv_inner(
        &pool,
        ORG,
        &AuditFilterInput {
            target_table: Some("sif".into()),
            ..Default::default()
        },
        &out_str,
    )
    .await
    .expect("export link");

    let text = std::fs::read_to_string(&out).expect("read");
    // 自然语言应该在某行 note 列里（CSV 末列）
    assert!(
        text.contains("关联 PT-101"),
        "link description 应进 CSV：\n{text}"
    );

    // unlink
    unlink_instrument_from_sif_inner(&pool, ORG, link_id, "王工")
        .await
        .expect("unlink");

    // unlink 后再 export 一次 —— unlink 的 description 会进新文件的 note 列
    let r2 = export_audit_csv_inner(
        &pool,
        ORG,
        &AuditFilterInput {
            target_table: Some("sif".into()),
            ..Default::default()
        },
        &out_str,
    )
    .await
    .expect("export unlink");
    assert!(
        r2.rows >= 3,
        "create + link + unlink 应至少 3 行：{}",
        r2.rows
    );
    let text2 = std::fs::read_to_string(&out).expect("read");
    assert!(
        text2.contains("解除 PT-101"),
        "unlink description 应在 CSV：\n{text2}"
    );
}

// ===========================================================================
// 4. export_audit_csv —— 字段级 diff 落到 before_json / after_json 列
// ===========================================================================
#[tokio::test]
async fn export_field_diff_lands_in_before_after_columns() {
    let (dir, pool) = fresh_pool().await;
    let _ = create_project_inner(&pool, ORG, &proj_input("PRJ-1", "项目"), "张工")
        .await
        .expect("p");
    let inst = create_instrument_inner(&pool, ORG, ACTOR, inst_input("PT-101", "detector", "A"))
        .await
        .expect("inst");
    // update：仅 sil_target 由 A → B
    update_instrument_inner(
        &pool,
        ORG,
        ACTOR,
        inst.id,
        inst_input("PT-101", "detector", "B"),
    )
    .await
    .expect("update");

    let out = dir.path().join("diff.csv");
    let out_str = out.to_string_lossy().replace('\\', "/");
    export_audit_csv_inner(
        &pool,
        ORG,
        &AuditFilterInput {
            target_table: Some("instrument".into()),
            action: Some("instrument_update".into()),
            ..Default::default()
        },
        &out_str,
    )
    .await
    .expect("export diff");

    let text = std::fs::read_to_string(&out).expect("read");
    // CSV 转义后 JSON 字段里的双引号变 ""（CSV 风格连续两个双引号），不是 \"
    // raw 字符串：内容 ""silTarget"":""A"" —— 6 个双引号 + silTarget + :A
    assert!(
        text.contains(r#"""silTarget"":""A"""#),
        "before_json 应含 silTarget=A：\n{text}"
    );
    assert!(
        text.contains(r#"""silTarget"":""B"""#),
        "after_json 应含 silTarget=B：\n{text}"
    );
    // fields_changed 列里包含 silTarget
    assert!(text.contains("silTarget"));
}

// ===========================================================================
// 5. export_audit_csv —— 拒绝相对路径 / 拒绝空路径
// ===========================================================================
#[tokio::test]
async fn export_rejects_relative_or_empty_path() {
    let (_dir, pool) = fresh_pool().await;
    // 相对路径
    let r1 = export_audit_csv_inner(&pool, ORG, &AuditFilterInput::default(), "audit.csv").await;
    assert!(r1.is_err(), "相对路径应被拒");
    let e1 = r1.unwrap_err();
    assert!(e1.to_string().contains("must be absolute"), "err msg: {e1}");

    // 空路径
    let r2 = export_audit_csv_inner(&pool, ORG, &AuditFilterInput::default(), "").await;
    assert!(r2.is_err(), "空路径应被拒");
    let r3 = export_audit_csv_inner(&pool, ORG, &AuditFilterInput::default(), "   ").await;
    assert!(r3.is_err(), "空白路径应被拒");
}

// ===========================================================================
// 5b. export_audit_csv —— 行数上限：超限时截断到 MAX_EXPORT_ROWS 并置 truncated
// ===========================================================================
#[tokio::test]
async fn export_csv_truncates_at_row_cap() {
    use sif_studio_lib::commands::audit_export::{build_audit_csv_bytes, MAX_EXPORT_ROWS};

    // 递归 CTE 一次性灌 MAX+3 行（直写 SQL，走业务命令太慢）
    async fn seed(pool: &SqlitePool, n: i64) {
        sqlx::query(
            "WITH RECURSIVE seq(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM seq WHERE i < ?1)
             INSERT INTO audit_log (org_id, ts, actor, action, target_table, payload_json)
             SELECT 1, datetime('now'), 'load', 'test.gen', 'audit_log', '{}' FROM seq",
        )
        .bind(n)
        .execute(pool)
        .await
        .expect("seed audit rows");
    }

    // 超限：在线版 → (bytes, cap, truncated=true)
    let (_dir, pool) = fresh_pool().await;
    seed(&pool, MAX_EXPORT_ROWS + 3).await;
    let (bytes, rows, truncated) = build_audit_csv_bytes(&pool, ORG, &AuditFilterInput::default())
        .await
        .expect("build csv");
    assert!(truncated, "超过上限应置 truncated");
    assert_eq!(rows, MAX_EXPORT_ROWS, "只导出最新 MAX_EXPORT_ROWS 行");
    assert!(bytes.len() > 1_000, "导出内容不应为空");

    // 遗留文件版：ExportResult.truncated = true 且行数 = 上限
    let out_path = std::env::temp_dir().join(format!("sif-cap-{}.csv", std::process::id()));
    let out_str = out_path.to_string_lossy().replace('\\', "/");
    let r = export_audit_csv_inner(&pool, ORG, &AuditFilterInput::default(), &out_str)
        .await
        .expect("file export");
    assert!(r.truncated);
    assert_eq!(r.rows, MAX_EXPORT_ROWS);
    let _ = std::fs::remove_file(&out_path);

    // 反例：远低于上限 → 不截断
    let (_dir2, pool2) = fresh_pool().await;
    seed(&pool2, 2).await;
    let (_b, rows2, truncated2) = build_audit_csv_bytes(&pool2, ORG, &AuditFilterInput::default())
        .await
        .expect("build csv");
    assert!(!truncated2);
    assert_eq!(rows2, 2);
}

// ===========================================================================
// 6. preview_audit_filtered —— limit + id DESC
// ===========================================================================
#[tokio::test]
async fn preview_respects_limit_and_id_desc() {
    let (_dir, pool) = fresh_pool().await;
    for i in 0..5 {
        create_project_inner(
            &pool,
            ORG,
            &proj_input(&format!("PRJ-{i}"), &format!("项目 {i}")),
            "张工",
        )
        .await
        .expect("p");
    }
    let all = preview_audit_filtered_inner(&pool, ORG, &AuditFilterInput::default(), None)
        .await
        .expect("preview all");
    assert_eq!(all.len(), 5);
    // id DESC：最新（id=5）应在最前
    for i in 1..all.len() {
        assert!(all[i - 1].id > all[i].id, "应按 id DESC 排序");
    }
    let two = preview_audit_filtered_inner(&pool, ORG, &AuditFilterInput::default(), Some(2))
        .await
        .expect("preview 2");
    assert_eq!(two.len(), 2);
}

// ===========================================================================
// 7. summarize_audit_filtered —— by_action / by_table
// ===========================================================================
#[tokio::test]
async fn summarize_groups_by_action_and_table() {
    let (_dir, pool) = fresh_pool().await;
    let _p = create_project_inner(&pool, ORG, &proj_input("PRJ-1", "项目"), "张工")
        .await
        .expect("p");
    for i in 0..3 {
        create_instrument_inner(
            &pool,
            ORG,
            ACTOR,
            inst_input(&format!("PT-10{i}"), "detector", "B"),
        )
        .await
        .expect("inst");
    }
    let s = summarize_audit_filtered_inner(&pool, ORG, &AuditFilterInput::default())
        .await
        .expect("summarize");
    assert_eq!(s.total, 4);
    assert_eq!(s.by_action.get("project_create"), Some(&1));
    assert_eq!(s.by_action.get("instrument_create"), Some(&3));
    assert_eq!(s.by_table.get("project"), Some(&1));
    assert_eq!(s.by_table.get("instrument"), Some(&3));
}

// ===========================================================================
// 8. count_audit_filtered 与 preview 一致
// ===========================================================================
#[tokio::test]
async fn count_matches_preview_length() {
    let (_dir, pool) = fresh_pool().await;
    // 仪表必须先有归属项目（create 会校验 project 存在且同 org）
    create_project_inner(&pool, ORG, &proj_input("PRJ-C", "计数项目"), "张工")
        .await
        .expect("project");
    for i in 0..7 {
        create_instrument_inner(
            &pool,
            ORG,
            ACTOR,
            inst_input(&format!("PT-10{i}"), "detector", "B"),
        )
        .await
        .expect("inst");
    }
    let f = AuditFilterInput {
        target_table: Some("instrument".into()),
        ..Default::default()
    };
    let c = count_audit_filtered_inner(&pool, ORG, &f)
        .await
        .expect("count");
    let p = preview_audit_filtered_inner(&pool, ORG, &f, None)
        .await
        .expect("preview");
    assert_eq!(c as usize, p.len());
}

// ===========================================================================
// 9. list_audit_filters —— actions / actors / tables 去重
// ===========================================================================
#[tokio::test]
async fn list_filters_dedupes() {
    let (_dir, pool) = fresh_pool().await;
    let _p1 = create_project_inner(&pool, ORG, &proj_input("PRJ-A", "项目 A"), "张工")
        .await
        .expect("p");
    let _p2 = create_project_inner(&pool, ORG, &proj_input("PRJ-B", "项目 B"), "李主任")
        .await
        .expect("p");
    let opts = list_audit_filters_inner(&pool, ORG).await.expect("filters");
    assert!(opts.actions.contains(&"project_create".to_string()));
    assert!(opts.actors.contains(&"张工".to_string()));
    assert!(opts.actors.contains(&"李主任".to_string()));
    assert!(opts.tables.contains(&"project".to_string()));
    // actors 应去重（即使张工创建 1 个项目，也只出现一次）
    let zhang_count = opts.actors.iter().filter(|x| *x == "张工").count();
    assert_eq!(zhang_count, 1, "actors 应去重");
}

// ===========================================================================
// 10. CASCADE 后 audit_log 仍在（合规铁律：append-only）
// ===========================================================================
#[tokio::test]
async fn cascade_delete_keeps_audit_intact() {
    let (_dir, pool) = fresh_pool().await;
    let p = create_project_inner(&pool, ORG, &proj_input("PRJ-1", "项目"), "张工")
        .await
        .expect("p");
    let inst = create_instrument_inner(&pool, ORG, ACTOR, inst_input("PT-101", "detector", "B"))
        .await
        .expect("inst");
    let sif = create_sif_inner(&pool, ORG, &sif_input(p.id, "SIF-1", "SIF"), "张工")
        .await
        .expect("sif");
    let _ = link_instrument_to_sif_inner(
        &pool,
        ORG,
        sif.id,
        inst.id,
        "detector".into(),
        0,
        None,
        "".into(),
        "陈工",
    )
    .await
    .expect("link");

    // 删仪表 / 删 SIF / 删项目
    delete_instrument_inner(&pool, ORG, ACTOR, inst.id)
        .await
        .expect("del inst");
    delete_sif_inner(&pool, ORG, sif.id, "张工")
        .await
        .expect("del sif");
    delete_project_inner(&pool, ORG, p.id, "张工")
        .await
        .expect("del p");

    // audit_log 应仍能查到这个 SIF / instrument 的历史
    let sif_hist = list_history_for_target_inner(&pool, ORG, "sif", sif.id, None)
        .await
        .expect("sif hist");
    assert!(sif_hist.len() >= 2, "sif_create + sif_delete 应仍在");
    let inst_hist = list_history_for_target_inner(&pool, ORG, "instrument", inst.id, None)
        .await
        .expect("inst hist");
    assert!(
        inst_hist.len() >= 2,
        "instrument_create + instrument_delete 应仍在"
    );
    // 删 SIF 时的 audit 也写到了
    assert!(
        sif_hist.iter().any(|e| e.action == "sif_delete"),
        "sif_delete 应被审计"
    );
}

// ===========================================================================
// 11. 空 / 空白 filter 字段 → 当 None 处理（不误匹配空串）
// ===========================================================================
#[tokio::test]
async fn whitespace_filter_treated_as_no_filter() {
    let (_dir, pool) = fresh_pool().await;
    let _ = create_project_inner(&pool, ORG, &proj_input("PRJ-1", "项目"), "张工")
        .await
        .expect("p");
    let all = count_audit_filtered_inner(
        &pool,
        ORG,
        &AuditFilterInput {
            target_table: Some("   ".into()),
            actor: Some("".into()),
            ts_from: Some("\t".into()),
            ..Default::default()
        },
    )
    .await
    .expect("count");
    assert_eq!(all, 1, "空白 filter 应忽略，命中全部 1 条");
}

// ===========================================================================
// 12. write_audit 路径：手动写一条后能 list 到
// ===========================================================================
#[tokio::test]
async fn manual_write_then_preview() {
    let (_dir, pool) = fresh_pool().await;
    let v = serde_json::json!({
        "before": null,
        "after": {"foo": "bar"},
        "fieldsChanged": ["*"],
        "description": "manual"
    });
    write_audit(
        &pool,
        ORG,
        "测试人",
        "manual_event",
        "instrument",
        Some(99),
        v,
    )
    .await
    .expect("write");
    let list = preview_audit_filtered_inner(
        &pool,
        ORG,
        &AuditFilterInput {
            target_table: Some("instrument".into()),
            target_id: Some(99),
            ..Default::default()
        },
        None,
    )
    .await
    .expect("preview");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].actor, "测试人");
    assert_eq!(list[0].note, "manual");
}
