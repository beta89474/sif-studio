//! M2.4 集成测试 —— SIF / Project 修改历史（B 阶段：全部内联 org_id=1）
//!
//! 单独一个 test target（与 integration_test.rs 并行跑，互不干扰）。
//!
//! 覆盖：
//!   - sif_create / sif_update / sif_delete 都写 audit（payload 协议同 M2.3）
//!   - sif_link / sif_unlink 写 audit（fieldsChanged=["link"/"unlink"] + description）
//!   - project_create / project_update / project_delete 都写 audit
//!   - list_sif_history / list_project_history 复用通用 helper（按 id DESC + limit）

use sif_studio_lib::commands::audit::list_history_for_target_inner;
use sif_studio_lib::commands::instruments::{create_instrument_inner, InstrumentInput};
use sif_studio_lib::commands::projects::{
    create_project_inner, delete_project_inner, list_project_history_inner, update_project_inner,
    ProjectInput,
};
use sif_studio_lib::commands::sifs::{
    create_sif_inner, delete_sif_inner, link_instrument_to_sif_inner, list_sif_history_inner,
    unlink_instrument_from_sif_inner, update_sif_inner, SifInput,
};
use sif_studio_lib::db::open_in_memory;
use sqlx::{Pool, Sqlite};

/// B 阶段统一组织夹具（004 迁移后内存库自带 org id=1）
const ORG: i64 = 1;
const ACTOR: &str = "tester";

// ===========================================================================
// helpers
// ===========================================================================

/// 构造一个完整 InstrumentInput
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
        installed_at: "2025-06-12".into(),
        notes: String::new(),
        // M2.9 — 测试用项目 ID 1（setup 中建的项目）
        project_id: 1,
    }
}

/// 构造一条 SifInput（Rust 结构体需给全字段）
fn sif_input(project_id: i64, code: &str, name: &str) -> SifInput {
    SifInput {
        project_id,
        code: code.into(),
        name: name.into(),
        description: "初始说明".into(),
        sil_design: "B".into(),
        sil_verified: "B".into(),
        demand_mode: "low".into(),
        pfdavg_target: Some(0.005),
        proof_interval: 12,
    }
}

/// 构造一条 ProjectInput
fn project_input(code: &str, name: &str) -> ProjectInput {
    ProjectInput {
        code: code.into(),
        name: name.into(),
        client: "中石化某炼化".into(),
        location: "江苏南京".into(),
        phase: "design".into(),
        finished_at: String::new(),
        notes: "测试用".into(),
    }
}

/// 建一个 project + 一个 detector 仪表，供 SIF 测试用
async fn setup_sif_scenario() -> (Pool<Sqlite>, i64, i64) {
    let pool = open_in_memory().await.unwrap();
    let p = create_project_inner(&pool, ORG, &project_input("PRJ-T", "测试项目"), ACTOR)
        .await
        .unwrap();
    let i = create_instrument_inner(
        &pool,
        ORG,
        ACTOR,
        inst_input("PT-401", Some(50.0), "测试服务", "kPa"),
    )
    .await
    .unwrap();
    (pool, p.id, i.id)
}

// ===========================================================================
// SIF 侧
// ===========================================================================

// ---------- 1. sif_create 写 audit，完整快照 ----------
#[tokio::test]
async fn test_sif_create_writes_audit_with_snapshot() {
    let (pool, pid, _iid) = setup_sif_scenario().await;
    let s = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-401", "测试联锁"), ACTOR)
        .await
        .unwrap();
    assert!(s.id > 0);

    let hist = list_sif_history_inner(&pool, ORG, s.id, None)
        .await
        .unwrap();
    assert_eq!(hist.len(), 1);
    assert_eq!(hist[0].action, "sif_create");
    assert_eq!(hist[0].target_table, "sif");
    assert!(hist[0].before.is_none(), "create 的 before 应为 None");
    assert_eq!(
        hist[0].after.as_ref().unwrap()["code"],
        "SIF-401",
        "after 应含完整快照"
    );
    assert_eq!(hist[0].fields_changed, vec!["*".to_string()]);
}

// ---------- 2. sif_update 产生字段级 diff ----------
#[tokio::test]
async fn test_sif_update_field_diff() {
    let (pool, pid, _iid) = setup_sif_scenario().await;
    let s = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-402", "原名称"), ACTOR)
        .await
        .unwrap();

    // 只改 name + proof_interval（12 → 24），其余字段完全一致
    let mut input = sif_input(pid, "SIF-402", "改后名称");
    input.proof_interval = 24;
    let updated = update_sif_inner(&pool, ORG, s.id, &input, ACTOR)
        .await
        .unwrap();
    assert_eq!(updated.name, "改后名称");
    assert_eq!(updated.proof_interval, 24);

    let hist = list_sif_history_inner(&pool, ORG, s.id, None)
        .await
        .unwrap();
    assert_eq!(hist.len(), 2, "应有 create + update 两条");
    let upd = &hist[0]; // 倒序，最新在前
    assert_eq!(upd.action, "sif_update");
    let fields: &Vec<String> = &upd.fields_changed;
    assert!(fields.contains(&"name".to_string()), "name 应被标为改动");
    assert!(
        fields.contains(&"proofInterval".to_string()),
        "proofInterval 应被标为改动"
    );
    assert!(!fields.contains(&"code".to_string()), "code 未变不应列入");
    // before / after 快照都应有 name
    assert_eq!(upd.before.as_ref().unwrap()["name"], "原名称");
    assert_eq!(upd.after.as_ref().unwrap()["name"], "改后名称");
}

// ---------- 3. sif_delete 写 audit，before 快照 ----------
#[tokio::test]
async fn test_sif_delete_writes_audit_with_before_snapshot() {
    let (pool, pid, _iid) = setup_sif_scenario().await;
    let s = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-403", "待删除"), ACTOR)
        .await
        .unwrap();
    let n = delete_sif_inner(&pool, ORG, s.id, ACTOR).await.unwrap();
    assert_eq!(n, 1);

    // SIF 行已删，但 audit 仍在
    let hist = list_sif_history_inner(&pool, ORG, s.id, None)
        .await
        .unwrap();
    assert_eq!(hist.len(), 2, "create + delete");
    let del = &hist[0];
    assert_eq!(del.action, "sif_delete");
    assert!(del.after.is_none(), "delete 的 after 应为 None");
    assert_eq!(del.before.as_ref().unwrap()["code"], "SIF-403");
}

// ---------- 4. sif_link 写 audit + description ----------
#[tokio::test]
async fn test_sif_link_writes_audit_with_description() {
    let (pool, pid, iid) = setup_sif_scenario().await;
    let s = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-404", "关联测试"), ACTOR)
        .await
        .unwrap();

    let link_id = link_instrument_to_sif_inner(
        &pool,
        ORG,
        s.id,
        iid,
        "detector".into(),
        1,
        None,
        "第一条检测回路".into(),
        ACTOR,
    )
    .await
    .unwrap();
    assert!(link_id > 0);

    let hist = list_sif_history_inner(&pool, ORG, s.id, None)
        .await
        .unwrap();
    assert_eq!(hist.len(), 2, "create + link");
    let link = &hist[0];
    assert_eq!(link.action, "sif_link");
    assert_eq!(link.fields_changed, vec!["link".to_string()]);
    // description 应解析到 note
    assert!(
        link.note.contains("PT-401"),
        "description 应含位号；实际: {}",
        link.note
    );
    assert!(link.note.contains("detector"), "应含 role");
    // after 快照含关键字段
    let after = link.after.as_ref().unwrap();
    assert_eq!(after["instrument_tag"], "PT-401");
    assert_eq!(after["role"], "detector");
    assert_eq!(after["port_index"], 1);
}

// ---------- 5. sif_unlink 写 audit + description ----------
#[tokio::test]
async fn test_sif_unlink_writes_audit() {
    let (pool, pid, iid) = setup_sif_scenario().await;
    let s = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-405", "解除测试"), ACTOR)
        .await
        .unwrap();
    let link_id = link_instrument_to_sif_inner(
        &pool,
        ORG,
        s.id,
        iid,
        "final".into(),
        2,
        None,
        String::new(),
        ACTOR,
    )
    .await
    .unwrap();

    let n = unlink_instrument_from_sif_inner(&pool, ORG, link_id, ACTOR)
        .await
        .unwrap();
    assert_eq!(n, 1);

    let hist = list_sif_history_inner(&pool, ORG, s.id, None)
        .await
        .unwrap();
    assert_eq!(hist.len(), 3, "create + link + unlink");
    let unl = &hist[0]; // 最新的
    assert_eq!(unl.action, "sif_unlink");
    assert_eq!(unl.fields_changed, vec!["unlink".to_string()]);
    // 解除时 before 有（link 详情），after 为 None
    assert!(unl.before.is_some());
    assert!(unl.after.is_none());
    assert!(unl.note.contains("PT-401"), "description 应含位号");
}

// ---------- 6. list_sif_history 倒序 + limit + 隔离 ----------
#[tokio::test]
async fn test_list_sif_history_order_limit_isolation() {
    let (pool, pid, _iid) = setup_sif_scenario().await;
    let a = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-406", "A"), ACTOR)
        .await
        .unwrap();
    // 两次 update
    let mut i2 = sif_input(pid, "SIF-406", "A-v2");
    i2.proof_interval = 18;
    update_sif_inner(&pool, ORG, a.id, &i2, ACTOR)
        .await
        .unwrap();
    let mut i3 = sif_input(pid, "SIF-406", "A-v3");
    i3.proof_interval = 24;
    update_sif_inner(&pool, ORG, a.id, &i3, ACTOR)
        .await
        .unwrap();

    // B：不相关的 SIF
    let _b = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-407", "B"), ACTOR)
        .await
        .unwrap();

    let hist = list_sif_history_inner(&pool, ORG, a.id, None)
        .await
        .unwrap();
    assert_eq!(hist.len(), 3, "A 应有 3 条（create + 2 update）");
    assert_eq!(hist[0].action, "sif_update");
    assert_eq!(hist[0].after.as_ref().unwrap()["name"], "A-v3");
    assert_eq!(hist[2].action, "sif_create");

    // limit 生效
    let limited = list_sif_history_inner(&pool, ORG, a.id, Some(2))
        .await
        .unwrap();
    assert_eq!(limited.len(), 2);

    // 不存在的 SIF → 空列表
    let none = list_sif_history_inner(&pool, ORG, 99999, None)
        .await
        .unwrap();
    assert!(none.is_empty());

    // 隔离：B 的历史不应混入 A
    assert!(hist.iter().all(|h| h.target_id == Some(a.id)));
}

// ===========================================================================
// Project 侧
// ===========================================================================

// ---------- 7. project create / update / delete 三处 audit ----------
#[tokio::test]
async fn test_project_create_update_delete_audit() {
    let pool = open_in_memory().await.unwrap();
    let p = create_project_inner(&pool, ORG, &project_input("PRJ-501", "新项目"), ACTOR)
        .await
        .unwrap();

    // update：改 client + phase
    let mut up = project_input("PRJ-501", "新项目");
    up.client = "新业主公司".into();
    up.phase = "construction".into();
    let updated = update_project_inner(&pool, ORG, p.id, &up, ACTOR)
        .await
        .unwrap();
    assert_eq!(updated.phase, "construction");

    // delete
    delete_project_inner(&pool, ORG, p.id, ACTOR).await.unwrap();

    let hist = list_project_history_inner(&pool, ORG, p.id, None)
        .await
        .unwrap();
    assert_eq!(hist.len(), 3, "create + update + delete");
    assert_eq!(hist[0].action, "project_delete");
    assert_eq!(hist[1].action, "project_update");
    assert_eq!(hist[2].action, "project_create");
    // update 的字段 diff
    let fields = &hist[1].fields_changed;
    assert!(fields.contains(&"client".to_string()));
    assert!(fields.contains(&"phase".to_string()));
    // create/delete 用 ["*"]
    assert_eq!(hist[2].fields_changed, vec!["*".to_string()]);
    assert_eq!(hist[0].fields_changed, vec!["*".to_string()]);
}

// ---------- 8. project_history 隔离 + 通用 helper 复用 ----------
#[tokio::test]
async fn test_list_project_history_isolation_and_generic_helper() {
    let pool = open_in_memory().await.unwrap();
    let a = create_project_inner(&pool, ORG, &project_input("PRJ-601", "A 项目"), ACTOR)
        .await
        .unwrap();
    let b = create_project_inner(&pool, ORG, &project_input("PRJ-602", "B 项目"), ACTOR)
        .await
        .unwrap();

    let ha = list_project_history_inner(&pool, ORG, a.id, None)
        .await
        .unwrap();
    let hb = list_project_history_inner(&pool, ORG, b.id, None)
        .await
        .unwrap();
    assert_eq!(ha.len(), 1);
    assert_eq!(hb.len(), 1);
    assert_eq!(ha[0].target_id, Some(a.id));
    assert_eq!(hb[0].target_id, Some(b.id));
    assert_eq!(ha[0].after.as_ref().unwrap()["code"], "PRJ-601");
    assert_eq!(hb[0].after.as_ref().unwrap()["code"], "PRJ-602");

    // 通用 helper 直接调用（同 audit 表的三种 target_table 都能查）
    let inst_hist = list_history_for_target_inner(&pool, ORG, "instrument", 99999, None)
        .await
        .unwrap();
    assert!(inst_hist.is_empty(), "不存在的 target 应返回空");
}
