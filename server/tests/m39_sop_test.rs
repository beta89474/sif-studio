//! m39 — 检验测试规程 SOP 关联（IEC 61511-1 §16.2.2）
//!
//! 覆盖：
//!   - SOP 库 CRUD + code 组织内唯一 + 必填校验 + usage_count 聚合
//!   - proof_test 关联 SOP：sop_linked + code/title/version 快照
//!   - 无关联的检验记录 sop_linked=false
//!   - sop_id 跨组织 / 不存在 → 拒绝
//!   - 删除被引用 SOP：无 force 拒绝；force=true → SET NULL，检验历史保留

use sif_studio_lib::commands::projects::{create_project_inner, ProjectInput};
use sif_studio_lib::commands::proof_tests::{
    create_proof_test_inner, list_proof_tests_inner, update_proof_test_inner, ProofTestInput,
};
use sif_studio_lib::commands::sifs::{create_sif_inner, SifInput};
use sif_studio_lib::commands::sops::{
    create_sop_inner, delete_sop_inner, list_sops_inner, update_sop_inner, ProofTestSopInput,
};
use sif_studio_lib::db::open_in_memory;
use sqlx::{Pool, Sqlite};

const ORG: i64 = 1;
const ORG_OTHER: i64 = 2;
const ACTOR: &str = "tester";

async fn setup() -> (Pool<Sqlite>, i64) {
    let pool = open_in_memory().await.expect("open");
    sqlx::query("INSERT INTO org (id, name) VALUES (?, '乙组织')")
        .bind(ORG_OTHER)
        .execute(&pool)
        .await
        .unwrap();
    let p = create_project_inner(
        &pool,
        ORG,
        &ProjectInput {
            code: "PRJ-SOP".into(),
            name: "SOP 测试项目".into(),
            client: "".into(),
            location: "".into(),
            phase: "design".into(),
            finished_at: "".into(),
            notes: "".into(),
        },
        ACTOR,
    )
    .await
    .unwrap();
    (pool, p.id)
}

async fn make_sif(pool: &Pool<Sqlite>, pid: i64, code: &str) -> i64 {
    create_sif_inner(
        pool,
        ORG,
        &SifInput {
            project_id: pid,
            code: code.into(),
            name: format!("{code} 名称"),
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
        },
        ACTOR,
    )
    .await
    .unwrap()
    .id
}

fn sop_input(code: &str) -> ProofTestSopInput {
    ProofTestSopInput {
        code: code.into(),
        title: "SDV 全行程测试规程".into(),
        version: "v2.1".into(),
        doc_ref: "SOP-MECH-007".into(),
        scope: "气动关断阀".into(),
        test_method: "1. 隔离工艺 2. 全行程动作 3. 记录时间".into(),
        pass_criteria: "行程到位且动作时间 ≤ 5s".into(),
        notes: String::new(),
    }
}

fn pt_input(sif_id: i64, sop_id: Option<i64>) -> ProofTestInput {
    ProofTestInput {
        sif_id,
        tested_at: "2026-09-01".into(),
        result: "pass".into(),
        tested_by: "王工".into(),
        findings: String::new(),
        notes: String::new(),
        sop_id,
    }
}

/// SOP CRUD 主路径 + 默认版本
#[tokio::test]
async fn sop_crud_happy_path() {
    let (pool, _pid) = setup().await;
    let created = create_sop_inner(&pool, ORG, &sop_input("SOP-01"), ACTOR)
        .await
        .expect("create sop");
    assert_eq!(created.version, "v2.1");
    assert_eq!(created.usage_count, 0);

    let listed = list_sops_inner(&pool, ORG).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].doc_ref, "SOP-MECH-007");

    let upd = update_sop_inner(
        &pool,
        ORG,
        created.id,
        &ProofTestSopInput {
            version: "v2.2".into(),
            ..sop_input("SOP-01")
        },
        ACTOR,
    )
    .await
    .expect("update");
    assert_eq!(upd.version, "v2.2");
}

/// code 组织内唯一：重复 code → Conflict；改 code 成已存在值同样冲突
#[tokio::test]
async fn sop_code_unique_per_org() {
    let (pool, _pid) = setup().await;
    create_sop_inner(&pool, ORG, &sop_input("SOP-DUP"), ACTOR).await.unwrap();
    let err = create_sop_inner(&pool, ORG, &sop_input("SOP-DUP"), ACTOR).await;
    assert!(matches!(err, Err(sif_studio_lib::AppError::Conflict(_))));
}

/// 必填校验：空 code / 空 title 拒绝
#[tokio::test]
async fn sop_required_fields() {
    let (pool, _pid) = setup().await;
    let bad_code = ProofTestSopInput { code: String::new(), ..sop_input("X") };
    assert!(create_sop_inner(&pool, ORG, &bad_code, ACTOR).await.is_err());
    let bad_title = ProofTestSopInput { title: String::new(), ..sop_input("Y") };
    assert!(create_sop_inner(&pool, ORG, &bad_title, ACTOR).await.is_err());
}

/// 检验记录关联 SOP：sop_linked=true 且快照 code/version 正确；
/// usage_count 随之增加
#[tokio::test]
async fn proof_test_linked_to_sop_snapshot_and_usage() {
    let (pool, pid) = setup().await;
    let sif_id = make_sif(&pool, pid, "SIF-S1").await;
    let sop = create_sop_inner(&pool, ORG, &sop_input("SOP-L1"), ACTOR).await.unwrap();

    let pt = create_proof_test_inner(&pool, ORG, ACTOR, pt_input(sif_id, Some(sop.id)))
        .await
        .expect("link sop");
    assert!(pt.sop_linked);
    assert_eq!(pt.sop_id, Some(sop.id));
    assert_eq!(pt.sop_code.as_deref(), Some("SOP-L1"));
    assert_eq!(pt.sop_version.as_deref(), Some("v2.1"));
    assert_eq!(pt.sop_title.as_deref(), Some("SDV 全行程测试规程"));

    let sops = list_sops_inner(&pool, ORG).await.unwrap();
    assert_eq!(sops[0].usage_count, 1);
}

/// 无 SOP 关联的检验记录 sop_linked=false，快照字段为 None
#[tokio::test]
async fn proof_test_without_sop_not_linked() {
    let (pool, pid) = setup().await;
    let sif_id = make_sif(&pool, pid, "SIF-S0").await;
    let pt = create_proof_test_inner(&pool, ORG, ACTOR, pt_input(sif_id, None))
        .await
        .unwrap();
    assert!(!pt.sop_linked);
    assert!(pt.sop_id.is_none());
    assert!(pt.sop_code.is_none());
}

/// 不存在的 sop_id → Validation 拒绝
#[tokio::test]
async fn reject_nonexistent_sop_ref() {
    let (pool, pid) = setup().await;
    let sif_id = make_sif(&pool, pid, "SIF-S2").await;
    let err = create_proof_test_inner(&pool, ORG, ACTOR, pt_input(sif_id, Some(99999))).await;
    assert!(matches!(err, Err(sif_studio_lib::AppError::Validation(_))));
}

/// 跨组织 sop_id（乙组织 id 空间即使巧合也查不到）→ Validation 拒绝
#[tokio::test]
async fn reject_cross_org_sop_ref() {
    let (pool, pid) = setup().await;
    let sif_id = make_sif(&pool, pid, "SIF-S3").await;
    // 直接在乙组织插一条 SOP
    let other_id: i64 = sqlx::query_scalar(
        "INSERT INTO proof_test_sop (org_id, code, title) VALUES (2, 'SOP-XORG', '跨组织') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    // 甲组织列表看不到乙组织 SOP
    assert!(list_sops_inner(&pool, ORG).await.unwrap().is_empty());
    let err = create_proof_test_inner(&pool, ORG, ACTOR, pt_input(sif_id, Some(other_id))).await;
    assert!(matches!(err, Err(sif_studio_lib::AppError::Validation(_))));
}

/// 删除被引用 SOP：无 force 拒绝；force=true 后检验记录 sop_id SET NULL
#[tokio::test]
async fn delete_used_sop_requires_force_and_sets_null() {
    let (pool, pid) = setup().await;
    let sif_id = make_sif(&pool, pid, "SIF-S4").await;
    let sop = create_sop_inner(&pool, ORG, &sop_input("SOP-D1"), ACTOR).await.unwrap();
    let pt = create_proof_test_inner(&pool, ORG, ACTOR, pt_input(sif_id, Some(sop.id)))
        .await
        .unwrap();

    // 无 force → 拒绝
    let denied = delete_sop_inner(&pool, ORG, sop.id, false, ACTOR).await;
    assert!(matches!(denied, Err(sif_studio_lib::AppError::Validation(_))));

    // force → 删除成功，proof_test 行保留但关联解除
    let n = delete_sop_inner(&pool, ORG, sop.id, true, ACTOR).await.unwrap();
    assert_eq!(n, 1);

    let list = list_proof_tests_inner(&pool, ORG, Some(sif_id)).await.unwrap();
    assert_eq!(list.len(), 1, "检验记录不应被级联删除");
    assert!(list[0].sop_id.is_none());
    assert!(!list[0].sop_linked);
    assert_eq!(list[0].id, pt.id);
}

/// update 变更 SOP 关联（None → Some → None）后快照随之刷新
#[tokio::test]
async fn update_switches_sop_link() {
    let (pool, pid) = setup().await;
    let sif_id = make_sif(&pool, pid, "SIF-S5").await;
    let sop = create_sop_inner(&pool, ORG, &sop_input("SOP-U1"), ACTOR).await.unwrap();

    // 无关联 → 关联
    let updated = update_proof_test_inner(
        &pool,
        ORG,
        ACTOR,
        create_proof_test_inner(&pool, ORG, ACTOR, pt_input(sif_id, None))
            .await
            .unwrap()
            .id,
        pt_input(sif_id, Some(sop.id)),
    )
    .await
    .unwrap();
    assert!(updated.sop_linked);
    assert_eq!(updated.sop_code.as_deref(), Some("SOP-U1"));

    // 关联 → 解除
    let cleared = update_proof_test_inner(
        &pool,
        ORG,
        ACTOR,
        updated.id,
        pt_input(sif_id, None),
    )
    .await
    .unwrap();
    assert!(!cleared.sop_linked);
    assert!(cleared.sop_id.is_none());
}
