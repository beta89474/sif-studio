//! M2.7 — 审计包图表（commands/audit_export.rs §5）的集成测试（B：org 作用域）
//!
//! 覆盖：
//!   - compute_daily_activity：空 / 按天分组 / action 筛选
//!   - compute_bypass_duration_buckets：空 / 5 桶分布 / 未恢复的不计
//!   - compute_sil_change_timeline：空 / sif_create / sif_update / 其他表过滤
//!   - fetch_audit_charts：3 个 inner 的合集调用

use sif_studio_lib::commands::audit_export::{
    compute_bypass_duration_buckets_inner, compute_daily_activity_inner,
    compute_sil_change_timeline_inner, AuditFilterInput,
};
use sif_studio_lib::commands::sifs::{create_sif_inner, update_sif_inner, SifInput};
use sif_studio_lib::db::open_in_memory;

const ORG: i64 = 1;

// ===========================================================================
// 1. compute_daily_activity
// ===========================================================================

#[tokio::test]
async fn daily_activity_empty_returns_empty() {
    let pool = open_in_memory().await.expect("in-memory");
    let r = compute_daily_activity_inner(&pool, ORG, &AuditFilterInput::default())
        .await
        .expect("call");
    assert!(r.is_empty(), "空库应返 0 行");
}

#[tokio::test]
async fn daily_activity_groups_by_date() {
    let pool = open_in_memory().await.expect("in-memory");
    // 直接写 audit_log：2 行 09-10，3 行 09-11，1 行 09-12 → 3 天
    for (ts, action) in [
        ("2026-09-10T08:00:00Z", "instrument_create"),
        ("2026-09-10T15:30:00Z", "instrument_update"),
        ("2026-09-11T09:00:00Z", "sif_create"),
        ("2026-09-11T12:00:00Z", "sif_update"),
        ("2026-09-11T18:00:00Z", "sif_link"),
        ("2026-09-12T03:00:00Z", "project_create"),
    ] {
        sqlx::query(
            "INSERT INTO audit_log (org_id, ts, actor, action, target_table, target_id, payload_json)
             VALUES (1, ?, '张工', ?, 'instrument', 1, '{}')",
        )
        .bind(ts)
        .bind(action)
        .execute(&pool)
        .await
        .expect("insert");
    }
    let r = compute_daily_activity_inner(&pool, ORG, &AuditFilterInput::default())
        .await
        .expect("call");
    assert_eq!(r.len(), 3, "应聚成 3 个日期");
    assert_eq!(r[0].date, "2026-09-10");
    assert_eq!(r[0].count, 2);
    assert_eq!(r[1].date, "2026-09-11");
    assert_eq!(r[1].count, 3);
    assert_eq!(r[2].date, "2026-09-12");
    assert_eq!(r[2].count, 1);
}

#[tokio::test]
async fn daily_activity_handles_mixed_ts_format() {
    // 真实场景：M2.1 写 `datetime('now')`（空格分隔），M2.3+ 写 RFC 3339（T 分隔）
    // substr(ts, 1, 10) 应对两种格式都返回 YYYY-MM-DD
    let pool = open_in_memory().await.expect("in-memory");
    sqlx::query(
        "INSERT INTO audit_log (org_id, ts, actor, action, target_table, target_id, payload_json)
         VALUES (1, '2026-09-13 06:07:35', '李工', 'instrument_create', 'instrument', 1, '{}')",
    )
    .execute(&pool)
    .await
    .expect("insert");
    sqlx::query(
        "INSERT INTO audit_log (org_id, ts, actor, action, target_table, target_id, payload_json)
         VALUES (1, '2026-09-13T07:08:36.500Z', '李工', 'instrument_update', 'instrument', 2, '{}')",
    )
    .execute(&pool)
    .await
    .expect("insert");
    let r = compute_daily_activity_inner(&pool, ORG, &AuditFilterInput::default())
        .await
        .expect("call");
    assert_eq!(r.len(), 1, "两种格式应被合并到同一日期");
    assert_eq!(r[0].date, "2026-09-13");
    assert_eq!(r[0].count, 2);
}

#[tokio::test]
async fn daily_activity_filter_by_action() {
    let pool = open_in_memory().await.expect("in-memory");
    for (ts, action) in [
        ("2026-09-10T08:00:00Z", "instrument_create"),
        ("2026-09-10T15:30:00Z", "sif_create"),
        ("2026-09-11T09:00:00Z", "instrument_create"),
    ] {
        sqlx::query(
            "INSERT INTO audit_log (org_id, ts, actor, action, target_table, target_id, payload_json)
             VALUES (1, ?, '张工', ?, 'instrument', 1, '{}')",
        )
        .bind(ts)
        .bind(action)
        .execute(&pool)
        .await
        .expect("insert");
    }
    let r = compute_daily_activity_inner(
        &pool,
        ORG,
        &AuditFilterInput {
            action: Some("instrument_create".into()),
            ..Default::default()
        },
    )
    .await
    .expect("call");
    assert_eq!(r.len(), 2);
    assert_eq!(r.iter().map(|x| x.count).sum::<i64>(), 2);
}

// ===========================================================================
// 2. compute_bypass_duration_buckets
// ===========================================================================

#[tokio::test]
async fn bypass_buckets_empty_returns_zeros() {
    let pool = open_in_memory().await.expect("in-memory");
    let r = compute_bypass_duration_buckets_inner(&pool, ORG, &AuditFilterInput::default())
        .await
        .expect("call");
    assert_eq!(r.len(), 5, "固定 5 个桶");
    assert!(r.iter().all(|b| b.count == 0));
}

#[tokio::test]
async fn bypass_buckets_distribute_across_all_five() {
    let pool = open_in_memory().await.expect("in-memory");
    let p = sif_studio_lib::commands::projects::create_project_inner(
        &pool,
        ORG,
        &sif_studio_lib::commands::projects::ProjectInput {
            code: "PRJ-1".into(),
            name: "项目".into(),
            client: "中石化".into(),
            location: "华东".into(),
            phase: "design".into(),
            finished_at: "".into(),
            notes: "".into(),
        },
        "张工",
    )
    .await
    .expect("project");
    let s = create_sif_inner(&pool, ORG, &sif_input_with_project(p.id, "SIF-201"), "张工")
        .await
        .expect("sif");
    // 5 个旁路：跨度 <1h, 1-8h, 8-24h, 1-7d, >7d
    // bypassed_at 是 datetime('now') 空格分隔；restored_at 是 RFC 3339 T 分隔
    let cases = [
        // (bypassed_at, restored_at, expected_bucket_idx)
        ("2026-09-10 00:00:00", "2026-09-10T00:30:00Z", 0), // 30 min → <1h
        ("2026-09-10 00:00:00", "2026-09-10T05:00:00Z", 1), // 5h → 1-8h
        ("2026-09-10 00:00:00", "2026-09-10T20:00:00Z", 2), // 20h → 8-24h
        ("2026-09-10 00:00:00", "2026-09-13T00:00:00Z", 3), // 3d → 1-7d
        ("2026-09-01 00:00:00", "2026-09-20T00:00:00Z", 4), // 19d → >7d
    ];
    for (b, r, _expected) in &cases {
        sqlx::query(
            "INSERT INTO bypass_record
             (org_id, project_id, sif_id, bypassed_by, bypassed_at, reason,
              planned_restore, restored_at, permit_no, approved_by)
             VALUES (1, ?, ?, '张工', ?, '联锁测试旁路临时解除用于检修',
                     '2026-12-31T00:00:00Z', ?, 'WP-001', '李主任')",
        )
        .bind(p.id)
        .bind(s.id)
        .bind(b)
        .bind(r)
        .execute(&pool)
        .await
        .expect("insert bypass");
    }
    let buckets = compute_bypass_duration_buckets_inner(&pool, ORG, &AuditFilterInput::default())
        .await
        .expect("call");
    assert_eq!(buckets.len(), 5);
    assert_eq!(buckets[0].count, 1, "<1h 桶");
    assert_eq!(buckets[1].count, 1, "1-8h 桶");
    assert_eq!(buckets[2].count, 1, "8-24h 桶");
    assert_eq!(buckets[3].count, 1, "1-7d 桶");
    assert_eq!(buckets[4].count, 1, ">7d 桶");
    // 桶顺序固定（按 lower_hours 升序）
    assert_eq!(buckets[0].label, "<1 小时");
    assert_eq!(buckets[1].label, "1–8 小时");
    assert_eq!(buckets[2].label, "8–24 小时");
    assert_eq!(buckets[3].label, "1–7 天");
    assert_eq!(buckets[4].label, ">7 天");
}

#[tokio::test]
async fn bypass_buckets_excludes_unrestored() {
    let pool = open_in_memory().await.expect("in-memory");
    let p = sif_studio_lib::commands::projects::create_project_inner(
        &pool,
        ORG,
        &sif_studio_lib::commands::projects::ProjectInput {
            code: "PRJ-1".into(),
            name: "项目".into(),
            client: "中石化".into(),
            location: "华东".into(),
            phase: "design".into(),
            finished_at: "".into(),
            notes: "".into(),
        },
        "张工",
    )
    .await
    .expect("project");
    let s = create_sif_inner(&pool, ORG, &sif_input_with_project(p.id, "SIF-201"), "张工")
        .await
        .expect("sif");
    // 已恢复 + 未恢复各 1 条
    sqlx::query(
        "INSERT INTO bypass_record
         (org_id, project_id, sif_id, bypassed_by, bypassed_at, reason,
          planned_restore, restored_at, permit_no, approved_by)
         VALUES (1, ?, ?, '张工', '2026-09-10 00:00:00', '联锁测试旁路临时解除用于检修',
                 '2026-12-31T00:00:00Z', '2026-09-10T05:00:00Z', 'WP-001', '李主任')",
    )
    .bind(p.id)
    .bind(s.id)
    .execute(&pool)
    .await
    .expect("insert 1");
    sqlx::query(
        "INSERT INTO bypass_record
         (org_id, project_id, sif_id, bypassed_by, bypassed_at, reason,
          planned_restore, restored_at, permit_no, approved_by)
         VALUES (1, ?, ?, '张工', '2026-09-11 00:00:00', '联锁测试旁路临时解除用于检修',
                 '2026-12-31T00:00:00Z', '', 'WP-002', '李主任')",
    )
    .bind(p.id)
    .bind(s.id)
    .execute(&pool)
    .await
    .expect("insert 2");
    let buckets = compute_bypass_duration_buckets_inner(&pool, ORG, &AuditFilterInput::default())
        .await
        .expect("call");
    // 只有已恢复那 1 条进 1-8h 桶
    assert_eq!(buckets[1].count, 1);
    let total: i64 = buckets.iter().map(|b| b.count).sum();
    assert_eq!(total, 1, "未恢复的 1 条不应计入");
}

// ===========================================================================
// 3. compute_sil_change_timeline
// ===========================================================================

#[tokio::test]
async fn sil_change_empty_returns_empty() {
    let pool = open_in_memory().await.expect("in-memory");
    let r = compute_sil_change_timeline_inner(&pool, ORG, &AuditFilterInput::default())
        .await
        .expect("call");
    assert!(r.is_empty());
}

#[tokio::test]
async fn sil_change_create_only_has_empty_from() {
    let pool = open_in_memory().await.expect("in-memory");
    let p = sif_studio_lib::commands::projects::create_project_inner(
        &pool,
        ORG,
        &sif_studio_lib::commands::projects::ProjectInput {
            code: "PRJ-1".into(),
            name: "项目".into(),
            client: "中石化".into(),
            location: "华东".into(),
            phase: "design".into(),
            finished_at: "".into(),
            notes: "".into(),
        },
        "张工",
    )
    .await
    .expect("project");
    create_sif_inner(&pool, ORG, &sif_input_with_project(p.id, "SIF-201"), "张工")
        .await
        .expect("sif create");

    let r = compute_sil_change_timeline_inner(&pool, ORG, &AuditFilterInput::default())
        .await
        .expect("call");
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].sif_code, "SIF-201");
    assert_eq!(r[0].from_sil, "", "create 事件的 from 应为空");
    assert_eq!(r[0].to_sil, "B", "sil_design=B → silVerified=B");
    assert_eq!(r[0].action, "sif_create");
}

#[tokio::test]
async fn sil_change_update_records_from_to() {
    let pool = open_in_memory().await.expect("in-memory");
    let p = sif_studio_lib::commands::projects::create_project_inner(
        &pool,
        ORG,
        &sif_studio_lib::commands::projects::ProjectInput {
            code: "PRJ-1".into(),
            name: "项目".into(),
            client: "中石化".into(),
            location: "华东".into(),
            phase: "design".into(),
            finished_at: "".into(),
            notes: "".into(),
        },
        "张工",
    )
    .await
    .expect("project");
    let sif = create_sif_inner(&pool, ORG, &sif_input_with_project(p.id, "SIF-201"), "张工")
        .await
        .expect("sif create");
    // update sil_verified B → C
    update_sif_inner(
        &pool,
        ORG,
        sif.id,
        &SifInput {
            project_id: p.id,
            code: "SIF-201".into(),
            name: "高压联锁".into(),
            description: "".into(),
            sil_design: "B".into(),
            sil_verified: "C".into(),
            demand_mode: "low".into(),
            pfdavg_target: Some(0.001),
            proof_interval: 12,
        sensor_arch: "1oo1".into(),
        logic_arch: "1oo1".into(),
        final_arch: "1oo1".into(),
        mttr_hours: 8.0,
        beta_factor: 0.10,
        ..Default::default()
        },
        "李工",
    )
    .await
    .expect("update");

    let r = compute_sil_change_timeline_inner(&pool, ORG, &AuditFilterInput::default())
        .await
        .expect("call");
    assert_eq!(r.len(), 2, "create + update 各 1 条");
    // 按 ts ASC：create 在前
    assert_eq!(r[0].action, "sif_create");
    assert_eq!(r[0].from_sil, "");
    assert_eq!(r[0].to_sil, "B");
    assert_eq!(r[1].action, "sif_update");
    assert_eq!(r[1].from_sil, "B");
    assert_eq!(r[1].to_sil, "C");
}

#[tokio::test]
async fn sil_change_ignores_other_tables() {
    let pool = open_in_memory().await.expect("in-memory");
    // 写 5 条 instrument_create（target_table='instrument'）
    for i in 1..=5 {
        sqlx::query(
            "INSERT INTO audit_log (org_id, ts, actor, action, target_table, target_id, payload_json)
             VALUES (1, '2026-09-10T08:00:00Z', '张工', 'instrument_create', 'instrument', ?, '{}')",
        )
        .bind(i)
        .execute(&pool)
        .await
        .expect("insert");
    }
    let r = compute_sil_change_timeline_inner(&pool, ORG, &AuditFilterInput::default())
        .await
        .expect("call");
    assert!(r.is_empty(), "non-sif 行应被过滤");
}

#[tokio::test]
async fn fetch_audit_charts_combined_call() {
    let pool = open_in_memory().await.expect("in-memory");
    let p = sif_studio_lib::commands::projects::create_project_inner(
        &pool,
        ORG,
        &sif_studio_lib::commands::projects::ProjectInput {
            code: "PRJ-1".into(),
            name: "项目".into(),
            client: "中石化".into(),
            location: "华东".into(),
            phase: "design".into(),
            finished_at: "".into(),
            notes: "".into(),
        },
        "张工",
    )
    .await
    .expect("project");
    create_sif_inner(&pool, ORG, &sif_input_with_project(p.id, "SIF-201"), "张工")
        .await
        .expect("sif");

    // fetch_audit_charts 走 State 路径，需要构造 State——这里只测 inner 链路
    let daily = compute_daily_activity_inner(&pool, ORG, &AuditFilterInput::default())
        .await
        .unwrap();
    let buckets = compute_bypass_duration_buckets_inner(&pool, ORG, &AuditFilterInput::default())
        .await
        .unwrap();
    let sil = compute_sil_change_timeline_inner(&pool, ORG, &AuditFilterInput::default())
        .await
        .unwrap();
    // 三段都返回且结构正确
    assert!(!daily.is_empty(), "SIF create 应有 1 天");
    assert_eq!(buckets.len(), 5);
    assert_eq!(sil.len(), 1);
    assert_eq!(sil[0].sif_code, "SIF-201");
}

// ===========================================================================
// Helper
// ===========================================================================

fn sif_input_with_project(project_id: i64, code: &str) -> SifInput {
    SifInput {
        project_id,
        code: code.into(),
        name: "测试 SIF".into(),
        description: "".into(),
        sil_design: "B".into(),
        sil_verified: "B".into(),
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
