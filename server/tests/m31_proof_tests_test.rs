//! m31 — 检验测试管理集成测试（IEC 61511-1 §16.3）
//!
//! 覆盖：
//!   - create_proof_test_inner CRUD 主路径 + next_due_at 自动计算
//!   - 枚举校验（result / tested_at 未来日期 / 格式）
//!   - SIF 不存在返 NotFound
//!   - 删 SIF CASCADE 清检验记录
//!   - count_overdue 只计最新一条
//!   - 审计历史含 create / update / delete

use sif_studio_lib::commands::instruments::{
    create_instrument_inner, InstrumentInput,
};
use sif_studio_lib::commands::projects::{create_project_inner, ProjectInput};
use sif_studio_lib::commands::proof_tests::{
    count_overdue_proof_tests_inner, create_proof_test_inner, delete_proof_test_inner,
    list_proof_test_history_inner, list_proof_tests_inner, update_proof_test_inner,
    ProofTestInput,
};
use sif_studio_lib::commands::sifs::{create_sif_inner, delete_sif_inner, SifInput};
use sif_studio_lib::db::open_in_memory;
use sif_studio_lib::AppError;
use sqlx::{Pool, Sqlite};

const ORG: i64 = 1;
const ACTOR: &str = "tester";

async fn setup() -> (Pool<Sqlite>, i64, i64) {
    let pool = open_in_memory().await.expect("open");
    let p = create_project_inner(
        &pool,
        ORG,
        &ProjectInput {
            code: "PRJ-PT".into(),
            name: "检验测试项目".into(),
            client: "".into(),
            location: "".into(),
            phase: "design".into(),
            finished_at: "".into(),
            notes: "".into(),
        },
        ACTOR,
    )
    .await
    .expect("create project");

    // 建 SIF（proof_interval = 6 月）
    let sif = create_sif_inner(
        &pool,
        ORG,
        &SifInput {
            project_id: p.id,
            code: "SIF-001".into(),
            name: "反应釜高压 SIF".into(),
            description: String::new(),
            sil_design: "B".into(),
            sil_verified: "B".into(),
            demand_mode: "low".into(),
            pfdavg_target: Some(0.01),
            proof_interval: 6,
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
    .expect("create sif");

    (pool, p.id, sif.id)
}

fn pt_input(sif_id: i64) -> ProofTestInput {
    ProofTestInput {
        sif_id,
        tested_at: "2026-09-01".into(),
        result: "pass".into(),
        tested_by: "王工".into(),
        findings: String::new(),
        notes: "常规检验".into(),
        ..Default::default()
    }
}

// ===========================================================================
// CRUD 基本路径
// ===========================================================================

#[tokio::test]
async fn create_list_update_delete_proof_test() {
    let (pool, _pid, sif_id) = setup().await;

    let pt = create_proof_test_inner(&pool, ORG, ACTOR, pt_input(sif_id))
        .await
        .expect("create");
    assert_eq!(pt.sif_code, "SIF-001");
    assert_eq!(pt.result, "pass");
    // next_due_at = 2026-09-01 + 6 月 = 2027-03-01
    assert_eq!(pt.next_due_at, "2027-03-01");
    assert_eq!(pt.status, "current");

    // list
    let list = list_proof_tests_inner(&pool, ORG, Some(sif_id))
        .await
        .unwrap();
    assert_eq!(list.len(), 1);

    // update
    let updated = update_proof_test_inner(
        &pool,
        ORG,
        ACTOR,
        pt.id,
        ProofTestInput {
            tested_at: "2026-08-01".into(),
            result: "conditional".into(),
            tested_by: "李工".into(),
            findings: "发现漂移".into(),
            notes: "需复检".into(),
            sif_id,
            ..Default::default()
        },
    )
    .await
    .expect("update");
    assert_eq!(updated.result, "conditional");
    assert_eq!(updated.next_due_at, "2027-02-01"); // 2026-08-01 + 6 月
    assert_eq!(updated.tested_by, "李工");

    // delete
    delete_proof_test_inner(&pool, ORG, ACTOR, pt.id)
        .await
        .unwrap();
    assert!(list_proof_tests_inner(&pool, ORG, Some(sif_id))
        .await
        .unwrap()
        .is_empty());
}

// ===========================================================================
// 枚举校验
// ===========================================================================

#[tokio::test]
async fn create_rejects_bad_result_and_future_tested_at() {
    let (pool, _pid, sif_id) = setup().await;

    // result='BOGUS'
    let mut input = pt_input(sif_id);
    input.result = "BOGUS".into();
    assert!(matches!(
        create_proof_test_inner(&pool, ORG, ACTOR, input).await,
        Err(AppError::Validation(_))
    ));

    // tested_at 未来日期
    let mut input = pt_input(sif_id);
    input.tested_at = "2099-01-01".into();
    assert!(matches!(
        create_proof_test_inner(&pool, ORG, ACTOR, input).await,
        Err(AppError::Validation(_))
    ));

    // tested_at 格式错误
    let mut input = pt_input(sif_id);
    input.tested_at = "2026/01/01".into();
    assert!(matches!(
        create_proof_test_inner(&pool, ORG, ACTOR, input).await,
        Err(AppError::Validation(_))
    ));

    // tested_by 空
    let mut input = pt_input(sif_id);
    input.tested_by = "  ".into();
    assert!(matches!(
        create_proof_test_inner(&pool, ORG, ACTOR, input).await,
        Err(AppError::Validation(_))
    ));
}

// ===========================================================================
// SIF 不存在返 NotFound
// ===========================================================================

#[tokio::test]
async fn create_rejects_missing_sif() {
    let (pool, _pid, _sif_id) = setup().await;
    let mut input = pt_input(99999);
    input.sif_id = 99999;
    assert!(matches!(
        create_proof_test_inner(&pool, ORG, ACTOR, input).await,
        Err(AppError::NotFound(_))
    ));
}

// ===========================================================================
// 删 SIF CASCADE 清检验记录
// ===========================================================================

#[tokio::test]
async fn deleting_sif_cascades_proof_tests() {
    let (pool, _pid, sif_id) = setup().await;
    create_proof_test_inner(&pool, ORG, ACTOR, pt_input(sif_id))
        .await
        .unwrap();
    assert_eq!(
        list_proof_tests_inner(&pool, ORG, Some(sif_id))
            .await
            .unwrap()
            .len(),
        1
    );

    // SIF 无图引用，可以删除
    delete_sif_inner(&pool, ORG, sif_id, ACTOR)
        .await
        .expect("delete sif");

    assert!(list_proof_tests_inner(&pool, ORG, Some(sif_id))
        .await
        .unwrap()
        .is_empty());
}

// ===========================================================================
// overdue 只计最新一条
// ===========================================================================

#[tokio::test]
async fn count_overdue_counts_latest_per_sif_only() {
    let (pool, _pid, sif_id) = setup().await;

    // 建第一条：tested_at=2026-01-01, next_due=2026-07-01（过去 → overdue）
    let pt1 = create_proof_test_inner(&pool, ORG, ACTOR, pt_input(sif_id))
        .await
        .unwrap();
    // 手动把 next_due_at 改到过去（模拟历史记录）
    sqlx::query("UPDATE proof_test SET next_due_at = '2020-01-01' WHERE id = ?")
        .bind(pt1.id)
        .execute(&pool)
        .await
        .unwrap();

    // 此时 overdue count = 1
    let s = count_overdue_proof_tests_inner(&pool, ORG).await.unwrap();
    assert_eq!(s.count, 1, "第一条过期 → count=1");

    // 建第二条（更新检验，next_due 在未来）
    let _pt2 = create_proof_test_inner(
        &pool,
        ORG,
        ACTOR,
        ProofTestInput {
            sif_id,
            tested_at: "2026-09-01".into(),
            result: "pass".into(),
            tested_by: "王工".into(),
            findings: String::new(),
            notes: String::new(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    // next_due = 2026-09-01 + 6 月 = 2027-03-01（未来 → current）

    // 现在 pt1 是旧条（id 小），pt2 是最新条（id 大）
    // pt2 未过期 → 该 SIF 不计 overdue
    let s = count_overdue_proof_tests_inner(&pool, ORG).await.unwrap();
    assert_eq!(s.count, 0, "最新条未过期 → count=0");
}

// ===========================================================================
// 审计历史
// ===========================================================================

#[tokio::test]
async fn proof_test_history_records_lifecycle() {
    let (pool, _pid, sif_id) = setup().await;

    let pt = create_proof_test_inner(&pool, ORG, ACTOR, pt_input(sif_id))
        .await
        .unwrap();

    update_proof_test_inner(
        &pool,
        ORG,
        ACTOR,
        pt.id,
        ProofTestInput {
            tested_at: "2026-08-01".into(),
            result: "fail".into(),
            tested_by: "张工".into(),
            findings: "阀卡涩".into(),
            notes: String::new(),
            sif_id,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    delete_proof_test_inner(&pool, ORG, ACTOR, pt.id)
        .await
        .unwrap();

    let hist = list_proof_test_history_inner(&pool, ORG, pt.id, None)
        .await
        .unwrap();
    let actions: Vec<&str> = hist.iter().map(|h| h.action.as_str()).collect();
    assert!(actions.contains(&"proof_test_create"));
    assert!(actions.contains(&"proof_test_update"));
    assert!(actions.contains(&"proof_test_delete"));
}

// ===========================================================================
// Issue 1 回归：仪表跨项目迁移守卫
// ===========================================================================

#[tokio::test]
async fn instrument_project_id_change_blocked_when_linked_to_sif() {
    let (pool, pid, sif_id) = setup().await;

    // 建项目 B
    let pid_b = create_project_inner(
        &pool,
        ORG,
        &ProjectInput {
            code: "PRJ-B".into(),
            name: "项目B".into(),
            client: "".into(),
            location: "".into(),
            phase: "design".into(),
            finished_at: "".into(),
            notes: "".into(),
        },
        ACTOR,
    )
    .await
    .unwrap()
    .id;

    // 建仪表（项目 A）
    let inst = create_instrument_inner(
        &pool,
        ORG,
        ACTOR,
        InstrumentInput {
            tag: "PT-301".into(),
            service: "压力".into(),
            kind: "PT".into(),
            role: "detector".into(),
            psv_id: String::new(),
            manufacturer: String::new(),
            model: String::new(),
            range_min: Some(0.0),
            range_max: Some(2.0),
            unit: "MPa".into(),
            setpoint: None,
            sil_target: "NA".into(),
            proof_interval: 12,
            lambda_du: 0.0,
            lambda_dd: 0.0,
            lambda_su: 0.0,
            lambda_sd: 0.0,
            sff: 0.0,
            pt_coverage: 1.0,
            hft: 0,
            equipment_type: "type_b".into(),
            installed_at: String::new(),
            notes: String::new(),
            project_id: pid,
        },
    )
    .await
    .unwrap();

    // 关联仪表到 SIF（通过 sif_instrument 表直接插）
    sqlx::query("INSERT INTO sif_instrument (org_id, sif_id, instrument_id, role, port_index, note) VALUES (?, ?, ?, 'detector', 0, '')")
        .bind(ORG)
        .bind(sif_id)
        .bind(inst.id)
        .execute(&pool)
        .await
        .unwrap();

    // 尝试改 project_id → 应被拒
    let input_b = sif_studio_lib::commands::instruments::InstrumentInput {
        tag: "PT-301".into(),
        service: "压力".into(),
        kind: "PT".into(),
        role: "detector".into(),
        psv_id: String::new(),
        manufacturer: String::new(),
        model: String::new(),
        range_min: Some(0.0),
        range_max: Some(2.0),
        unit: "MPa".into(),
        setpoint: None,
        sil_target: "NA".into(),
        proof_interval: 12,
        lambda_du: 0.0,
        lambda_dd: 0.0,
        lambda_su: 0.0,
        lambda_sd: 0.0,
        sff: 0.0,
        pt_coverage: 1.0,
        hft: 0,
        equipment_type: "type_b".into(),
        installed_at: String::new(),
        notes: String::new(),
        project_id: pid_b, // 改到项目 B
    };
    let err = sif_studio_lib::commands::instruments::update_instrument_inner(
        &pool,
        ORG,
        ACTOR,
        inst.id,
        input_b,
    )
    .await;
    assert!(matches!(err, Err(AppError::Validation(_))));

    // 解除关联后迁移成功
    sqlx::query("DELETE FROM sif_instrument WHERE instrument_id = ?")
        .bind(inst.id)
        .execute(&pool)
        .await
        .unwrap();
    let input_b2 = sif_studio_lib::commands::instruments::InstrumentInput {
        tag: "PT-301".into(),
        service: "压力".into(),
        kind: "PT".into(),
        role: "detector".into(),
        psv_id: String::new(),
        manufacturer: String::new(),
        model: String::new(),
        range_min: Some(0.0),
        range_max: Some(2.0),
        unit: "MPa".into(),
        setpoint: None,
        sil_target: "NA".into(),
        proof_interval: 12,
        lambda_du: 0.0,
        lambda_dd: 0.0,
        lambda_su: 0.0,
        lambda_sd: 0.0,
        sff: 0.0,
        pt_coverage: 1.0,
        hft: 0,
        equipment_type: "type_b".into(),
        installed_at: String::new(),
        notes: String::new(),
        project_id: pid_b,
    };
    let updated = sif_studio_lib::commands::instruments::update_instrument_inner(
        &pool,
        ORG,
        ACTOR,
        inst.id,
        input_b2,
    )
    .await
    .unwrap();
    assert_eq!(updated.project_id, Some(pid_b));
}

// ===========================================================================
// Issue 2 回归：SIF 删除 RESTRICT（有图引用时拒绝）
// ===========================================================================

#[tokio::test]
async fn sif_delete_rejected_when_diagram_references_it() {
    let (pool, _pid, sif_id) = setup().await;

    // 手工插一张图引用此 SIF
    sqlx::query("INSERT INTO diagram (org_id, project_id, code, name, sif_id, sheet_size, revision, data, version) VALUES (?, ?, 'DWG-1', '测试图', ?, 'A3', 'A', '{}', 0)")
        .bind(ORG)
        .bind(_pid)
        .bind(sif_id)
        .execute(&pool)
        .await
        .unwrap();

    // 尝试删 SIF → 应被拒
    let err = delete_sif_inner(&pool, ORG, sif_id, ACTOR).await;
    assert!(matches!(err, Err(AppError::Validation(_))));

    // 无图引用的 SIF 可以删除
    let sif2 = create_sif_inner(
        &pool,
        ORG,
        &SifInput {
            project_id: _pid,
            code: "SIF-002".into(),
            name: "独立 SIF".into(),
            description: String::new(),
            sil_design: "NA".into(),
            sil_verified: "NA".into(),
            demand_mode: "low".into(),
            pfdavg_target: None,
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
    .unwrap();
    delete_sif_inner(&pool, ORG, sif2.id, ACTOR)
        .await
        .expect("无图的 SIF 可删");
}
