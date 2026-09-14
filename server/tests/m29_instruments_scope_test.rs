//! M2.9 集成测试 —— 仪表台账按项目隔离 + 强外键约束（B 阶段：org 作用域）
//!
//! 覆盖：
//!   - (org_id, project_id, tag) 作用域：
//!     * 同组织同项目内重复 tag → 409 Conflict
//!     * 跨项目同名 tag → 允许（罕见但合法：两台独立 PT-101）
//!   - create_instrument_inner 校验：project_id 必须 ≥1 + 项目存在且同 org
//!   - update_instrument_inner 允许改 project_id（用于把遗留库里的
//!     仪表重新指派到正式项目）
//!   - list_instruments_by_project_inner 按组织+项目过滤
//!   - count_instruments_by_project_inner 计数准确
//!   - 删除 project → 该 project 下所有 instrument CASCADE 清空
//!   - delete_instrument_inner 按 org+id 走（越权 id 返 NotFound）
//!
//! 测试隔离策略：与 m24/m25/m28 并行跑；每个用例自己开内存池（自带 org id=1）。

use sif_studio_lib::commands::instruments::{
    count_instruments_by_project_inner, create_instrument_inner, delete_instrument_inner,
    list_instruments_by_project_inner, update_instrument_inner, InstrumentInput,
};
use sif_studio_lib::commands::projects::{
    create_project_inner, delete_project_inner, ProjectInput,
};
use sif_studio_lib::db::open_in_memory;
use sqlx::{Pool, Sqlite};

const ORG: i64 = 1;
const ACTOR: &str = "tester";

// ===========================================================================
// helpers
// ===========================================================================

async fn fresh_pool() -> Pool<Sqlite> {
    open_in_memory().await.expect("open in-memory db")
}

async fn make_project(pool: &Pool<Sqlite>, code: &str, name: &str) -> i64 {
    create_project_inner(
        pool,
        ORG,
        &ProjectInput {
            code: code.into(),
            name: name.into(),
            client: "".into(),
            location: "".into(),
            phase: "design".into(),
            finished_at: "".into(),
            notes: "".into(),
        },
        ACTOR,
    )
    .await
    .expect("create project")
    .id
}

fn inst_input(tag: &str, project_id: i64) -> InstrumentInput {
    InstrumentInput {
        tag: tag.into(),
        service: format!("{tag} 服务"),
        kind: "PT".into(),
        role: "detector".into(),
        psv_id: "".into(),
        manufacturer: "Rosemount".into(),
        model: "3051CD".into(),
        range_min: Some(0.0),
        range_max: Some(100.0),
        unit: "kPa".into(),
        setpoint: Some(50.0),
        sil_target: "B".into(),
        proof_interval: 12,
        installed_at: "2026-09-13".into(),
        notes: "".into(),
        project_id,
    }
}

// ===========================================================================
// 1. 联合唯一：同项目重复 tag → 409
// ===========================================================================
#[tokio::test]
async fn dup_tag_in_same_project_rejected() {
    let pool = fresh_pool().await;
    let pid = make_project(&pool, "PRJ-A", "项目 A").await;

    let _ = create_instrument_inner(&pool, ORG, ACTOR, inst_input("PT-101", pid))
        .await
        .expect("first insert ok");

    let err = create_instrument_inner(&pool, ORG, ACTOR, inst_input("PT-101", pid))
        .await
        .expect_err("second insert should conflict");
    let msg = err.to_string();
    assert!(
        msg.contains("PT-101") && msg.contains("already"),
        "错误信息应含 tag 名 + already: {msg}"
    );
}

// ===========================================================================
// 2. 跨项目同名 tag → 允许（独立设备）
// ===========================================================================
#[tokio::test]
async fn same_tag_in_different_projects_allowed() {
    let pool = fresh_pool().await;
    let p_a = make_project(&pool, "PRJ-A", "项目 A").await;
    let p_b = make_project(&pool, "PRJ-B", "项目 B").await;

    let a = create_instrument_inner(&pool, ORG, ACTOR, inst_input("PT-101", p_a))
        .await
        .expect("A insert");
    let b = create_instrument_inner(&pool, ORG, ACTOR, inst_input("PT-101", p_b))
        .await
        .expect("B insert 同名 tag");

    assert_ne!(a.id, b.id, "两条独立记录");
    assert_eq!(a.project_id, Some(p_a));
    assert_eq!(b.project_id, Some(p_b));
}

// ===========================================================================
// 3. list_instruments_by_project_inner 按项目过滤
// ===========================================================================
#[tokio::test]
async fn list_instruments_scoped_to_project() {
    let pool = fresh_pool().await;
    let p_a = make_project(&pool, "PRJ-A", "A").await;
    let p_b = make_project(&pool, "PRJ-B", "B").await;

    // A 里 3 条，B 里 2 条
    for t in ["PT-101", "PT-102", "FT-201"] {
        create_instrument_inner(&pool, ORG, ACTOR, inst_input(t, p_a))
            .await
            .unwrap();
    }
    for t in ["XV-301", "XV-302"] {
        create_instrument_inner(&pool, ORG, ACTOR, inst_input(t, p_b))
            .await
            .unwrap();
    }

    let a_only = list_instruments_by_project_inner(&pool, ORG, p_a)
        .await
        .unwrap();
    let b_only = list_instruments_by_project_inner(&pool, ORG, p_b)
        .await
        .unwrap();

    assert_eq!(a_only.len(), 3, "A 项目 3 条");
    assert_eq!(b_only.len(), 2, "B 项目 2 条");
    for x in &a_only {
        assert_eq!(x.project_id, Some(p_a));
    }
    for x in &b_only {
        assert_eq!(x.project_id, Some(p_b));
    }
    let a_tags: Vec<&str> = a_only.iter().map(|i| i.tag.as_str()).collect();
    assert!(a_tags.contains(&"PT-101"));
    assert!(!a_tags.contains(&"XV-301"), "B 项目的仪表不应混入 A 列表");
}

// ===========================================================================
// 4. count_instruments_by_project_inner 计数
// ===========================================================================
#[tokio::test]
async fn count_instruments_scoped_to_project() {
    let pool = fresh_pool().await;
    let p_a = make_project(&pool, "PRJ-A", "A").await;
    let p_b = make_project(&pool, "PRJ-B", "B").await;

    for t in ["PT-1", "PT-2", "PT-3", "PT-4"] {
        create_instrument_inner(&pool, ORG, ACTOR, inst_input(t, p_a))
            .await
            .unwrap();
    }
    create_instrument_inner(&pool, ORG, ACTOR, inst_input("XV-1", p_b))
        .await
        .unwrap();

    assert_eq!(
        count_instruments_by_project_inner(&pool, ORG, p_a)
            .await
            .unwrap(),
        4
    );
    assert_eq!(
        count_instruments_by_project_inner(&pool, ORG, p_b)
            .await
            .unwrap(),
        1
    );
    // 不存在的项目返回 0（不是错）
    assert_eq!(
        count_instruments_by_project_inner(&pool, ORG, 99999)
            .await
            .unwrap(),
        0
    );
}

// ===========================================================================
// 5. project CASCADE → 自动清掉其下所有 instrument
// ===========================================================================
#[tokio::test]
async fn project_delete_cascades_instruments() {
    let pool = fresh_pool().await;
    let pid = make_project(&pool, "PRJ-X", "X").await;
    create_instrument_inner(&pool, ORG, ACTOR, inst_input("PT-1", pid))
        .await
        .unwrap();
    create_instrument_inner(&pool, ORG, ACTOR, inst_input("PT-2", pid))
        .await
        .unwrap();
    assert_eq!(
        count_instruments_by_project_inner(&pool, ORG, pid)
            .await
            .unwrap(),
        2
    );

    delete_project_inner(&pool, ORG, pid, ACTOR)
        .await
        .expect("delete project");
    assert_eq!(
        count_instruments_by_project_inner(&pool, ORG, pid)
            .await
            .unwrap(),
        0
    );
}

// ===========================================================================
// 6. update_instrument_inner 允许改 project_id（重指派遗留库仪表）
// ===========================================================================
#[tokio::test]
async fn update_instrument_can_relocate_project() {
    let pool = fresh_pool().await;
    let p_legacy = make_project(&pool, "PRJ-MIG-SRC", "迁移源项目").await;
    let p_real = make_project(&pool, "PRJ-MIG-DST", "正式项目").await;

    let inst = create_instrument_inner(&pool, ORG, ACTOR, inst_input("PT-OLD", p_legacy))
        .await
        .unwrap();
    assert_eq!(inst.project_id, Some(p_legacy));

    // 把 PT-OLD 重指派到正式项目
    let mut input = inst_input("PT-OLD", p_real);
    input.setpoint = Some(85.0);
    let updated = update_instrument_inner(&pool, ORG, ACTOR, inst.id, input)
        .await
        .unwrap();
    assert_eq!(updated.project_id, Some(p_real));
    assert_eq!(updated.setpoint, Some(85.0));

    // 源项目空了，目标项目拿到 1 条
    assert_eq!(
        count_instruments_by_project_inner(&pool, ORG, p_legacy)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        count_instruments_by_project_inner(&pool, ORG, p_real)
            .await
            .unwrap(),
        1
    );
}

// ===========================================================================
// 7. create_instrument_inner 校验：project_id 必须 ≥1 且存在
// ===========================================================================
#[tokio::test]
async fn create_instrument_rejects_invalid_project_id() {
    let pool = fresh_pool().await;

    let mut bad = inst_input("PT-Z", 1);
    bad.project_id = 0;
    let err = create_instrument_inner(&pool, ORG, ACTOR, bad)
        .await
        .expect_err("project_id=0 应拒绝");
    assert!(err.to_string().contains("project_id"));

    let mut bad2 = inst_input("PT-Z2", 1);
    bad2.project_id = -5;
    let err2 = create_instrument_inner(&pool, ORG, ACTOR, bad2)
        .await
        .expect_err("project_id=-5 应拒绝");
    assert!(err2.to_string().contains("project_id"));

    let mut bad3 = inst_input("PT-Z3", 1);
    bad3.project_id = 99999;
    let err3 = create_instrument_inner(&pool, ORG, ACTOR, bad3)
        .await
        .expect_err("不存在的 project_id 应拒绝");
    assert!(err3.to_string().contains("not found"));
}

// ===========================================================================
// 8. 存量回填兼容：无 org/project 归属的旧 instrument 经补救 UPDATE 归位
// ===========================================================================
#[tokio::test]
async fn legacy_instruments_backfill_to_default_scope() {
    let pool = fresh_pool().await;

    // 004 迁移已把存量 project 全部回填 org_id=1；这里取迁移建的
    // PRJ-LEGACY 兜底项目（003 迁移产物）。
    let legacy_id: i64 = sqlx::query_scalar("SELECT id FROM project WHERE code='PRJ-LEGACY'")
        .fetch_one(&pool)
        .await
        .expect("迁移应建 PRJ-LEGACY");

    // 模拟一条「迁移前」的遗留仪表：org_id / project_id 均 NULL
    sqlx::query(
        "INSERT INTO instrument (tag, kind, role, org_id, project_id)
         VALUES ('OLD-1', 'PT', 'detector', NULL, NULL)",
    )
    .execute(&pool)
    .await
    .unwrap();

    // 模拟 004 迁移的补救 UPDATE（org 与 project 同时回填）
    sqlx::query("UPDATE instrument SET org_id=1, project_id=? WHERE org_id IS NULL")
        .bind(legacy_id)
        .execute(&pool)
        .await
        .unwrap();

    // 作用域查询可见
    let list = list_instruments_by_project_inner(&pool, ORG, legacy_id)
        .await
        .unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].tag, "OLD-1");
    assert_eq!(list[0].project_id, Some(legacy_id));
}

// ===========================================================================
// 9. delete_instrument_inner 按 org+id 删除
// ===========================================================================
#[tokio::test]
async fn delete_instrument_by_id_works() {
    let pool = fresh_pool().await;
    let pid = make_project(&pool, "PRJ-D", "D").await;
    let inst = create_instrument_inner(&pool, ORG, ACTOR, inst_input("PT-DEL", pid))
        .await
        .unwrap();
    assert_eq!(
        count_instruments_by_project_inner(&pool, ORG, pid)
            .await
            .unwrap(),
        1
    );
    let n = delete_instrument_inner(&pool, ORG, ACTOR, inst.id)
        .await
        .unwrap();
    assert_eq!(n, 1);
    assert_eq!(
        count_instruments_by_project_inner(&pool, ORG, pid)
            .await
            .unwrap(),
        0
    );
}

// ===========================================================================
// 10. 跨项目同名 + 同项目不同 tag 共存
// ===========================================================================
#[tokio::test]
async fn cross_project_overlap_with_local_unique() {
    let pool = fresh_pool().await;
    let p1 = make_project(&pool, "P1", "1").await;
    let p2 = make_project(&pool, "P2", "2").await;

    // P1 有 PT-A, PT-B
    create_instrument_inner(&pool, ORG, ACTOR, inst_input("PT-A", p1))
        .await
        .unwrap();
    create_instrument_inner(&pool, ORG, ACTOR, inst_input("PT-B", p1))
        .await
        .unwrap();
    // P2 也有 PT-A（独立设备），加 PT-C
    create_instrument_inner(&pool, ORG, ACTOR, inst_input("PT-A", p2))
        .await
        .unwrap();
    create_instrument_inner(&pool, ORG, ACTOR, inst_input("PT-C", p2))
        .await
        .unwrap();

    let p1_list: Vec<String> = list_instruments_by_project_inner(&pool, ORG, p1)
        .await
        .unwrap()
        .into_iter()
        .map(|i| i.tag)
        .collect();
    let p2_list: Vec<String> = list_instruments_by_project_inner(&pool, ORG, p2)
        .await
        .unwrap()
        .into_iter()
        .map(|i| i.tag)
        .collect();

    assert_eq!(p1_list.len(), 2);
    assert!(p1_list.contains(&"PT-A".to_string()));
    assert!(p1_list.contains(&"PT-B".to_string()));
    assert!(!p1_list.contains(&"PT-C".to_string()));

    assert_eq!(p2_list.len(), 2);
    assert!(p2_list.contains(&"PT-A".to_string()));
    assert!(p2_list.contains(&"PT-C".to_string()));
    assert!(!p2_list.contains(&"PT-B".to_string()));
}
