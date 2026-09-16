//! M30 集成测试 —— 报警台账（alarm_ledger）CRUD
//!
//! 覆盖：
//!   - create_alarm_inner 必填校验 / 枚举校验 / 项目内 tag 唯一
//!   - list_alarms_inner 跨项目总览 + list_alarms_by_project_inner 项目过滤
//!   - update / delete + 404
//!   - 可选仪表关联：同 org 同项目允许；跨项目挂接被拒
//!   - 删除仪表后报警保留（instrument_id SET NULL）
//!   - 删除项目后报警 CASCADE
//!   - 修改历史走统一 audit_log（alarm_create / update / delete）

use sif_studio_lib::commands::alarms::{
    create_alarm_inner, delete_alarm_inner, list_alarm_history_inner, list_alarms_by_project_inner,
    list_alarms_inner, update_alarm_inner, AlarmInput,
};
use sif_studio_lib::commands::instruments::{create_instrument_inner, delete_instrument_inner};
use sif_studio_lib::commands::projects::{
    create_project_inner, delete_project_inner, ProjectInput,
};
use sif_studio_lib::db::open_in_memory;
use sif_studio_lib::AppError;
use sqlx::{Pool, Sqlite};

const ORG: i64 = 1;
const ACTOR: &str = "tester";

async fn setup() -> (Pool<Sqlite>, i64) {
    let pool = open_in_memory().await.expect("open");
    let p = create_project_inner(
        &pool,
        ORG,
        &ProjectInput {
            code: "PRJ-A".into(),
            name: "报警测试项目".into(),
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
    (pool, p.id)
}

async fn second_project(pool: &Pool<Sqlite>, code: &str) -> i64 {
    create_project_inner(
        pool,
        ORG,
        &ProjectInput {
            code: code.into(),
            name: "第二项目".into(),
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
    .id
}

fn alarm_input(pid: i64, tag: &str) -> AlarmInput {
    AlarmInput {
        project_id: pid,
        tag: tag.into(),
        instrument_id: None,
        description: String::new(),
        alarm_type: "HH".into(),
        priority: "high".into(),
        category: "process".into(),
        setpoint: Some(1.5),
        unit: "MPa".into(),
        deadband: Some(0.02),
        delay_seconds: 3,
        status: "normal".into(),
        response_action: String::new(),
        notes: String::new(),
    }
}

fn instrument_input(pid: i64, tag: &str) -> sif_studio_lib::commands::instruments::InstrumentInput {
    sif_studio_lib::commands::instruments::InstrumentInput {
        tag: tag.into(),
        service: "测试仪表".into(),
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
    }
}

// ===========================================================================
// CRUD 基本路径
// ===========================================================================

#[tokio::test]
async fn create_list_update_delete_alarm() {
    let (pool, pid) = setup().await;

    let a = create_alarm_inner(&pool, ORG, ACTOR, alarm_input(pid, "PAHH-201"))
        .await
        .unwrap();
    assert_eq!(a.tag, "PAHH-201");
    assert_eq!(a.alarm_type, "HH");
    assert_eq!(a.priority, "high");
    assert_eq!(a.setpoint, Some(1.5));
    assert!(a.instrument_id.is_none());

    // 项目过滤
    let by_proj = list_alarms_by_project_inner(&pool, ORG, pid).await.unwrap();
    assert_eq!(by_proj.len(), 1);
    // 总览
    let all = list_alarms_inner(&pool, ORG).await.unwrap();
    assert_eq!(all.len(), 1);

    // update
    let mut input = alarm_input(pid, "PAHH-201");
    input.priority = "critical".into();
    input.status = "active".into();
    input.response_action = "切断进料".into();
    let upd = update_alarm_inner(&pool, ORG, ACTOR, a.id, input)
        .await
        .unwrap();
    assert_eq!(upd.priority, "critical");
    assert_eq!(upd.status, "active");
    assert_eq!(upd.response_action, "切断进料");

    // delete
    let n = delete_alarm_inner(&pool, ORG, ACTOR, a.id).await.unwrap();
    assert_eq!(n, 1);
    assert!(list_alarms_by_project_inner(&pool, ORG, pid)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn create_alarm_rejects_blank_tag_and_bad_enums() {
    let (pool, pid) = setup().await;

    assert!(create_alarm_inner(&pool, ORG, ACTOR, alarm_input(pid, "  "))
        .await
        .is_err());
    let mut input = alarm_input(pid, "P-1");
    input.alarm_type = "BOGUS".into();
    assert!(matches!(
        create_alarm_inner(&pool, ORG, ACTOR, input).await,
        Err(AppError::Validation(_))
    ));

    let mut input2 = alarm_input(pid, "P-2");
    input2.priority = "urgent".into();
    assert!(create_alarm_inner(&pool, ORG, ACTOR, input2)
        .await
        .is_err());

    let mut input3 = alarm_input(pid, "P-3");
    input3.delay_seconds = -5;
    assert!(create_alarm_inner(&pool, ORG, ACTOR, input3)
        .await
        .is_err());
}

#[tokio::test]
async fn duplicate_tag_per_project_conflicts_but_cross_project_ok() {
    let (pool, pid1) = setup().await;
    let pid2 = second_project(&pool, "PRJ-B").await;

    create_alarm_inner(&pool, ORG, ACTOR, alarm_input(pid1, "PAH-1"))
        .await
        .unwrap();
    let dup = create_alarm_inner(&pool, ORG, ACTOR, alarm_input(pid1, "PAH-1")).await;
    assert!(matches!(dup, Err(AppError::Conflict(_))));

    // 跨项目同名允许
    let ok = create_alarm_inner(&pool, ORG, ACTOR, alarm_input(pid2, "PAH-1")).await;
    assert!(ok.is_ok(), "跨项目同 tag 应允许：{ok:?}");

    let all = list_alarms_inner(&pool, ORG).await.unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(
        list_alarms_by_project_inner(&pool, ORG, pid2)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn update_delete_missing_returns_not_found() {
    let (pool, _) = setup().await;
    let r = update_alarm_inner(&pool, ORG, ACTOR, 99999, alarm_input(1, "X")).await;
    assert!(matches!(r, Err(AppError::NotFound(_))));
    let r2 = delete_alarm_inner(&pool, ORG, ACTOR, 99999).await;
    assert!(matches!(r2, Err(AppError::NotFound(_))));
}

#[tokio::test]
async fn create_alarm_rejects_foreign_project() {
    let (pool, _) = setup().await;
    // project_id 不存在
    assert!(create_alarm_inner(&pool, ORG, ACTOR, alarm_input(999_999, "PAH-1"))
        .await
        .is_err());
    // project_id <= 0
    let mut input = alarm_input(1, "PAH-1");
    input.project_id = 0;
    assert!(create_alarm_inner(&pool, ORG, ACTOR, input).await.is_err());
}

// ===========================================================================
// 仪表软关联
// ===========================================================================

#[tokio::test]
async fn alarm_links_to_instrument_in_same_project() {
    let (pool, pid) = setup().await;
    let inst = create_instrument_inner(&pool, ORG, ACTOR, instrument_input(pid, "PT-201"))
        .await
        .unwrap();

    let mut input = alarm_input(pid, "PAHH-201");
    input.instrument_id = Some(inst.id);
    let a = create_alarm_inner(&pool, ORG, ACTOR, input).await.unwrap();
    assert_eq!(a.instrument_id, Some(inst.id));
}

#[tokio::test]
async fn alarm_rejects_instrument_from_other_project() {
    let (pool, pid1) = setup().await;
    let pid2 = second_project(&pool, "PRJ-C").await;
    // 仪表属于 pid1，报警建在 pid2 → 拒绝
    let inst = create_instrument_inner(&pool, ORG, ACTOR, instrument_input(pid1, "PT-9"))
        .await
        .unwrap();

    let mut input = alarm_input(pid2, "PAHH-9");
    input.instrument_id = Some(inst.id);
    let r = create_alarm_inner(&pool, ORG, ACTOR, input).await;
    assert!(
        matches!(r, Err(AppError::Validation(_))),
        "跨项目挂接仪表应被拒：{r:?}"
    );
}

#[tokio::test]
async fn deleting_instrument_keeps_alarm_and_nulls_link() {
    let (pool, pid) = setup().await;
    let inst = create_instrument_inner(&pool, ORG, ACTOR, instrument_input(pid, "PT-301"))
        .await
        .unwrap();
    let mut input = alarm_input(pid, "PAHH-301");
    input.instrument_id = Some(inst.id);
    let a = create_alarm_inner(&pool, ORG, ACTOR, input).await.unwrap();

    // 删仪表：报警必须保留，instrument_id 置空
    delete_instrument_inner(&pool, ORG, ACTOR, inst.id)
        .await
        .unwrap();
    let remaining = list_alarms_by_project_inner(&pool, ORG, pid)
        .await
        .unwrap();
    assert_eq!(remaining.len(), 1, "删仪表不得级联删报警");
    assert_eq!(remaining[0].id, a.id);
    assert_eq!(remaining[0].instrument_id, None, "软关联应被 SET NULL");
}

// ===========================================================================
// 级联 + 审计历史
// ===========================================================================

#[tokio::test]
async fn deleting_project_cascades_alarms() {
    let (pool, pid) = setup().await;
    create_alarm_inner(&pool, ORG, ACTOR, alarm_input(pid, "PAH-X"))
        .await
        .unwrap();
    delete_project_inner(&pool, ORG, pid, ACTOR).await.unwrap();
    assert!(list_alarms_inner(&pool, ORG).await.unwrap().is_empty());
}

#[tokio::test]
async fn alarm_history_records_lifecycle() {
    let (pool, pid) = setup().await;
    let a = create_alarm_inner(&pool, ORG, ACTOR, alarm_input(pid, "PAH-H"))
        .await
        .unwrap();
    let mut input = alarm_input(pid, "PAH-H");
    input.priority = "low".into();
    update_alarm_inner(&pool, ORG, ACTOR, a.id, input)
        .await
        .unwrap();
    delete_alarm_inner(&pool, ORG, ACTOR, a.id).await.unwrap();

    let hist = list_alarm_history_inner(&pool, ORG, a.id, None)
        .await
        .unwrap();
    let actions: Vec<&str> = hist.iter().map(|h| h.action.as_str()).collect();
    assert!(actions.contains(&"alarm_create"), "{actions:?}");
    assert!(actions.contains(&"alarm_update"), "{actions:?}");
    assert!(actions.contains(&"alarm_delete"), "{actions:?}");
}
