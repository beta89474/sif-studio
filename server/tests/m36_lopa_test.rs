//! m36 — LOPA（保护层分析）集成测试（IEC 61511-1 Annex E）
//!
//! 覆盖：
//!   - 场景 CRUD 主路径 + 默认 severity/sil_claim 生效
//!   - 保护层 CRUD + scenario_id 外键校验
//!   - layer_count 子查询聚合
//!   - 同项目重复 code → 409 Conflict；跨项目同 code → 允许
//!   - 多租户隔离：跨 org 不可见/不可写
//!   - SIF 软关联 + 删 SIF → sif_id SET NULL
//!   - 删 scenario → lopa_layer CASCADE 清空
//!   - 校验：code/title 必填、project/sif 同 org、layer_type 必填
//!   - NotFound：scenario/layer 不存在或跨 org
//!   - 审计历史含 create / update / delete

use sif_studio_lib::commands::audit::list_history_for_target_inner;
use sif_studio_lib::commands::lopa::{
    create_lopa_layer_inner, create_lopa_scenario_inner, delete_lopa_layer_inner,
    delete_lopa_scenario_inner, get_lopa_scenario_inner, list_lopa_layers_inner,
    list_lopa_scenarios_inner, update_lopa_layer_inner, update_lopa_scenario_inner,
    LopaLayerInput, LopaScenarioInput,
};
use sif_studio_lib::commands::projects::{create_project_inner, ProjectInput};
use sif_studio_lib::commands::sifs::{create_sif_inner, delete_sif_inner, SifInput};
use sif_studio_lib::db::open_in_memory;
use sif_studio_lib::AppError;
use sqlx::{Pool, Sqlite};

const ORG: i64 = 1;
const ORG_OTHER: i64 = 2; // 不同组织（手动插入 org 行，模拟多租户）
const ACTOR: &str = "tester";

// ===========================================================================
// helpers
// ===========================================================================

async fn fresh_pool() -> Pool<Sqlite> {
    let pool = open_in_memory().await.expect("open in-memory db");
    // 注入第二个组织（默认库只有 org_id=1）
    sqlx::query("INSERT INTO org (id, name) VALUES (?, '乙组织')")
        .bind(ORG_OTHER)
        .execute(&pool)
        .await
        .expect("insert second org");
    pool
}

async fn make_project(pool: &Pool<Sqlite>, org: i64, code: &str, name: &str) -> i64 {
    create_project_inner(
        pool,
        org,
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

fn scenario_input(project_id: i64, code: &str, title: &str) -> LopaScenarioInput {
    LopaScenarioInput {
        project_id,
        sif_id: None,
        code: code.into(),
        title: title.into(),
        hazard: "高压过压".into(),
        cause: "冷却失效".into(),
        consequence: "容器爆裂".into(),
        severity: "major".into(),
        init_freq: Some(1e-2),
        risk_tol: Some(1e-6),
        sil_claim: "B".into(),
        notes: "定级前场景".into(),
    }
}

fn layer_input(scenario_id: i64, seq: i64, layer_type: &str) -> LopaLayerInput {
    LopaLayerInput {
        scenario_id,
        seq,
        layer_type: layer_type.into(),
        description: format!("{layer_type} 保护层"),
        pfd: Some(1e-2),
        credit: 2.0,
    }
}

async fn make_sif(pool: &Pool<Sqlite>, project_id: i64, code: &str) -> i64 {
    create_sif_inner(
        pool,
        ORG,
        &SifInput {
            project_id,
            code: code.into(),
            name: format!("SIF {code}"),
            description: String::new(),
            sil_design: "B".into(),
            sil_verified: "B".into(),
            demand_mode: "low".into(),
            pfdavg_target: Some(0.01),
            proof_interval: 12,
            sensor_arch: "1oo1".into(),
            logic_arch: "1oo1".into(),
            final_arch: "1oo1".into(),
            mttr_hours: 8.0,
            beta_factor: 0.1,
            ..Default::default()
        },
        ACTOR,
    )
    .await
    .expect("create sif")
    .id
}

// ===========================================================================
// 1. 场景 CRUD 主路径 + 默认值
// ===========================================================================

#[tokio::test]
async fn scenario_crud_happy_path() {
    let pool = fresh_pool().await;
    let pid = make_project(&pool, ORG, "PRJ-L1", "LOPA 项目 A").await;

    // create（带完整字段）
    let s = create_lopa_scenario_inner(&pool, ORG, &scenario_input(pid, "SC-001", "高压过压"), ACTOR)
        .await
        .expect("create");
    assert_eq!(s.project_code, "PRJ-L1");
    assert_eq!(s.code, "SC-001");
    assert_eq!(s.title, "高压过压");
    assert_eq!(s.severity, "major");
    assert_eq!(s.sil_claim, "B");
    assert_eq!(s.layer_count, 0);
    assert!(s.sif_id.is_none() && s.sif_code.is_none());

    // get
    let got = get_lopa_scenario_inner(&pool, ORG, s.id).await.expect("get");
    assert_eq!(got.id, s.id);
    assert_eq!(got.hazard, "高压过压");

    // list（按 projectId 过滤）
    let list = list_lopa_scenarios_inner(&pool, ORG, Some(pid)).await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, s.id);

    // update
    let mut upd = scenario_input(pid, "SC-001", "高压过压（修订）");
    upd.severity = "catastrophic".into();
    upd.sil_claim = "C".into();
    let after = update_lopa_scenario_inner(&pool, ORG, s.id, &upd, ACTOR)
        .await
        .expect("update");
    assert_eq!(after.title, "高压过压（修订）");
    assert_eq!(after.severity, "catastrophic");
    assert_eq!(after.sil_claim, "C");

    // delete
    delete_lopa_scenario_inner(&pool, ORG, s.id, ACTOR)
        .await
        .expect("delete");
    let err = get_lopa_scenario_inner(&pool, ORG, s.id).await;
    assert!(matches!(err, Err(AppError::NotFound(_))));
}

// ===========================================================================
// 2. 默认 severity / sil_claim 生效
// ===========================================================================

#[tokio::test]
async fn scenario_default_severity_and_sil_claim() {
    let pool = fresh_pool().await;
    let pid = make_project(&pool, ORG, "PRJ-L2", "默认值项目").await;

    // 仅给 code + title + project_id，其余走 serde default
    let minimal = LopaScenarioInput {
        project_id: pid,
        code: "SC-MIN".into(),
        title: "最小输入场景".into(),
        ..Default::default()
    };
    let s = create_lopa_scenario_inner(&pool, ORG, &minimal, ACTOR)
        .await
        .expect("create minimal");
    assert_eq!(s.severity, "medium", "默认 severity=medium");
    assert_eq!(s.sil_claim, "NA", "默认 sil_claim=NA");
    assert!(s.init_freq.is_none());
    assert!(s.risk_tol.is_none());
    assert_eq!(s.hazard, "");
}

// ===========================================================================
// 3. 保护层 CRUD + layer_count 聚合
// ===========================================================================

#[tokio::test]
async fn layer_crud_and_count_aggregation() {
    let pool = fresh_pool().await;
    let pid = make_project(&pool, ORG, "PRJ-L3", "保护层项目").await;
    let sid = create_lopa_scenario_inner(&pool, ORG, &scenario_input(pid, "SC-1", "场景"), ACTOR)
        .await
        .unwrap()
        .id;

    // 加 3 个保护层
    for i in 1..=3 {
        let layer_type = match i {
            1 => "ipl",
            2 => "alarm",
            _ => "procedural",
        };
        let l = create_lopa_layer_inner(&pool, ORG, &layer_input(sid, i, layer_type), ACTOR)
            .await
            .expect("create layer");
        assert_eq!(l.scenario_id, sid);
        assert_eq!(l.seq, i);
    }

    // 重新查询 scenario，layer_count 应为 3
    let s = get_lopa_scenario_inner(&pool, ORG, sid).await.unwrap();
    assert_eq!(s.layer_count, 3, "layer_count 子查询聚合");

    // list layers（按 seq 排序）
    let layers = list_lopa_layers_inner(&pool, ORG, sid).await.unwrap();
    assert_eq!(layers.len(), 3);
    assert_eq!(layers[0].seq, 1);
    assert_eq!(layers[2].seq, 3);

    // update layer
    let mut upd = layer_input(sid, 5, "bypass");
    upd.description = "升级版旁路".into();
    let updated = update_lopa_layer_inner(&pool, ORG, layers[0].id, &upd, ACTOR)
        .await
        .expect("update layer");
    assert_eq!(updated.layer_type, "bypass");
    assert_eq!(updated.seq, 5);
    assert_eq!(updated.description, "升级版旁路");

    // delete layer
    delete_lopa_layer_inner(&pool, ORG, layers[2].id, ACTOR)
        .await
        .expect("delete layer");
    let after = list_lopa_layers_inner(&pool, ORG, sid).await.unwrap();
    assert_eq!(after.len(), 2);
}

// ===========================================================================
// 4. 同项目重复 code → Conflict；跨项目同 code → 允许
// ===========================================================================

#[tokio::test]
async fn duplicate_code_in_same_project_conflict() {
    let pool = fresh_pool().await;
    let pid = make_project(&pool, ORG, "PRJ-DUP", "重复 code 项目").await;

    let _ = create_lopa_scenario_inner(&pool, ORG, &scenario_input(pid, "SC-DUP", "a"), ACTOR)
        .await
        .unwrap();

    let err = create_lopa_scenario_inner(&pool, ORG, &scenario_input(pid, "SC-DUP", "b"), ACTOR)
        .await
        .expect_err("同项目同 code 应冲突");
    assert!(
        matches!(err, AppError::Conflict(_)),
        "应返回 Conflict，实际: {err:?}"
    );
}

#[tokio::test]
async fn same_code_in_different_projects_allowed() {
    let pool = fresh_pool().await;
    let p_a = make_project(&pool, ORG, "PRJ-A", "项目 A").await;
    let p_b = make_project(&pool, ORG, "PRJ-B", "项目 B").await;

    let a = create_lopa_scenario_inner(&pool, ORG, &scenario_input(p_a, "SC-X", "a"), ACTOR)
        .await
        .expect("A 创建");
    let b = create_lopa_scenario_inner(&pool, ORG, &scenario_input(p_b, "SC-X", "b"), ACTOR)
        .await
        .expect("B 创建（跨项目同 code 允许）");
    assert_ne!(a.id, b.id);
    assert_eq!(a.project_id, p_a);
    assert_eq!(b.project_id, p_b);
}

// ===========================================================================
// 5. 多租户隔离：跨 org 不可见、不可写
// ===========================================================================

#[tokio::test]
async fn cross_org_isolation() {
    let pool = fresh_pool().await;
    let pid_a = make_project(&pool, ORG, "PRJ-ISO", "甲组织项目").await;
    let pid_b = make_project(&pool, ORG_OTHER, "PRJ-ISO", "乙组织项目").await;

    let s = create_lopa_scenario_inner(&pool, ORG, &scenario_input(pid_a, "SC-1", "甲场景"), ACTOR)
        .await
        .unwrap();

    // list 跨 org → 看不到
    let list_other = list_lopa_scenarios_inner(&pool, ORG_OTHER, Some(pid_b))
        .await
        .unwrap();
    assert!(list_other.is_empty(), "乙组织不应看到甲组织的场景");

    // get 跨 org → NotFound
    let err = get_lopa_scenario_inner(&pool, ORG_OTHER, s.id).await;
    assert!(matches!(err, Err(AppError::NotFound(_))));

    // update 跨 org → NotFound
    let err = update_lopa_scenario_inner(&pool, ORG_OTHER, s.id, &scenario_input(pid_b, "SC-X", "y"), ACTOR).await;
    assert!(matches!(err, Err(AppError::NotFound(_))));

    // delete 跨 org → NotFound
    let err = delete_lopa_scenario_inner(&pool, ORG_OTHER, s.id, ACTOR).await;
    assert!(matches!(err, Err(AppError::NotFound(_))));

    // 校验 scope：用乙组织去创建指向甲项目 id 的场景 → 拒绝
    let err = create_lopa_scenario_inner(
        &pool,
        ORG_OTHER,
        &scenario_input(pid_a, "SC-EVIL", "越权"),
        ACTOR,
    )
    .await;
    assert!(
        matches!(err, Err(AppError::Validation(_))),
        "跨 org 引用别 org 的 project_id 应拒绝"
    );

    // layer 跨 org：在甲场景上乙组织加 layer → get 校验拒绝 NotFound
    let err = create_lopa_layer_inner(&pool, ORG_OTHER, &layer_input(s.id, 1, "ipl"), ACTOR).await;
    assert!(matches!(err, Err(AppError::NotFound(_))));
}

// ===========================================================================
// 6. SIF 软关联 + 删 SIF → sif_id SET NULL
// ===========================================================================

#[tokio::test]
async fn sif_soft_association_and_deletion_clears_sif_id() {
    let pool = fresh_pool().await;
    let pid = make_project(&pool, ORG, "PRJ-SIF", "SIF 关联项目").await;
    let sif_id = make_sif(&pool, pid, "SIF-001").await;

    let mut inp = scenario_input(pid, "SC-SIF", "关联 SIF 的场景");
    inp.sif_id = Some(sif_id);
    let s = create_lopa_scenario_inner(&pool, ORG, &inp, ACTOR)
        .await
        .expect("create scenario with sif");
    assert_eq!(s.sif_id, Some(sif_id));
    assert_eq!(s.sif_code.as_deref(), Some("SIF-001"));

    // 删 SIF（无图引用，可直接删）→ scenario.sif_id 应 SET NULL
    delete_sif_inner(&pool, ORG, sif_id, ACTOR)
        .await
        .expect("delete sif");

    let after = get_lopa_scenario_inner(&pool, ORG, s.id).await.unwrap();
    assert!(after.sif_id.is_none(), "删 SIF 后 sif_id 应为 NULL");
    assert!(after.sif_code.is_none());
    assert_eq!(after.title, "关联 SIF 的场景", "scenario 本身仍存在");
}

// ===========================================================================
// 7. 删 scenario → lopa_layer CASCADE 清空
// ===========================================================================

#[tokio::test]
async fn deleting_scenario_cascades_layers() {
    let pool = fresh_pool().await;
    let pid = make_project(&pool, ORG, "PRJ-CASC", "级联测试").await;
    let sid = create_lopa_scenario_inner(&pool, ORG, &scenario_input(pid, "SC-C", "场景"), ACTOR)
        .await
        .unwrap()
        .id;

    for i in 1..=2 {
        create_lopa_layer_inner(&pool, ORG, &layer_input(sid, i, "ipl"), ACTOR)
            .await
            .unwrap();
    }
    assert_eq!(
        list_lopa_layers_inner(&pool, ORG, sid).await.unwrap().len(),
        2
    );

    delete_lopa_scenario_inner(&pool, ORG, sid, ACTOR)
        .await
        .expect("delete scenario");

    // scenario 已删 → list layers 校验 NotFound
    let err = list_lopa_layers_inner(&pool, ORG, sid).await;
    assert!(matches!(err, Err(AppError::NotFound(_))));

    // 直接查表确认层已被 CASCADE 删除
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM lopa_layer WHERE scenario_id = ?")
        .bind(sid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0, "lopa_layer 应被 CASCADE 清空");
}

// ===========================================================================
// 8. 校验：code/title 必填 + project_id / sif_id 同 org
// ===========================================================================

#[tokio::test]
async fn validation_required_fields_and_scope() {
    let pool = fresh_pool().await;
    let pid = make_project(&pool, ORG, "PRJ-V", "校验项目").await;

    // code 空 → 拒绝
    let mut bad_code = scenario_input(pid, "", "缺 code");
    bad_code.code = "  ".into();
    let err = create_lopa_scenario_inner(&pool, ORG, &bad_code, ACTOR).await;
    assert!(matches!(err, Err(AppError::Validation(_))));

    // title 空 → 拒绝
    let mut bad_title = scenario_input(pid, "SC-X", "");
    bad_title.title = "  ".into();
    let err = create_lopa_scenario_inner(&pool, ORG, &bad_title, ACTOR).await;
    assert!(matches!(err, Err(AppError::Validation(_))));

    // project_id 不存在于本 org → 拒绝
    let err = create_lopa_scenario_inner(
        &pool,
        ORG,
        &scenario_input(999_999, "SC-GHOST", "幽灵项目"),
        ACTOR,
    )
    .await;
    assert!(
        matches!(err, Err(AppError::Validation(_))),
        "不存在的 project_id 应拒绝"
    );

    // sif_id 不存在于本 org → 拒绝
    let mut bad_sif = scenario_input(pid, "SC-SIF-BAD", "bad sif");
    bad_sif.sif_id = Some(999_999);
    let err = create_lopa_scenario_inner(&pool, ORG, &bad_sif, ACTOR).await;
    assert!(
        matches!(err, Err(AppError::Validation(_))),
        "不存在的 sif_id 应拒绝"
    );

    // layer_type 空 → 拒绝
    let sid = create_lopa_scenario_inner(&pool, ORG, &scenario_input(pid, "SC-LT", "layer 校验"), ACTOR)
        .await
        .unwrap()
        .id;
    let mut bad_layer = layer_input(sid, 1, "ipl");
    bad_layer.layer_type = "  ".into();
    let err = create_lopa_layer_inner(&pool, ORG, &bad_layer, ACTOR).await;
    assert!(matches!(err, Err(AppError::Validation(_))));
}

// ===========================================================================
// 9. NotFound：跨 org / 不存在的 id
// ===========================================================================

#[tokio::test]
async fn not_found_for_missing_or_cross_org() {
    let pool = fresh_pool().await;

    // scenario 不存在
    let err = get_lopa_scenario_inner(&pool, ORG, 99999).await;
    assert!(matches!(err, Err(AppError::NotFound(_))));

    // update 不存在
    let pid = make_project(&pool, ORG, "PRJ-NF", "项目").await;
    let err = update_lopa_scenario_inner(
        &pool,
        ORG,
        99999,
        &scenario_input(pid, "SC-NF", "x"),
        ACTOR,
    )
    .await;
    assert!(matches!(err, Err(AppError::NotFound(_))));

    // delete 不存在
    let err = delete_lopa_scenario_inner(&pool, ORG, 99999, ACTOR).await;
    assert!(matches!(err, Err(AppError::NotFound(_))));

    // layer update/delete 不存在
    let err = update_lopa_layer_inner(&pool, ORG, 99999, &layer_input(1, 1, "ipl"), ACTOR).await;
    assert!(matches!(err, Err(AppError::NotFound(_))));
    let err = delete_lopa_layer_inner(&pool, ORG, 99999, ACTOR).await;
    assert!(matches!(err, Err(AppError::NotFound(_))));
}

// ===========================================================================
// 10. 审计历史：scenario create / update / delete 全落
// ===========================================================================

#[tokio::test]
async fn audit_history_records_all_writes() {
    let pool = fresh_pool().await;
    let pid = make_project(&pool, ORG, "PRJ-AUD", "审计项目").await;

    let s = create_lopa_scenario_inner(&pool, ORG, &scenario_input(pid, "SC-AUD", "审计场景"), ACTOR)
        .await
        .unwrap();
    let _ = update_lopa_scenario_inner(
        &pool,
        ORG,
        s.id,
        &scenario_input(pid, "SC-AUD", "审计场景（改）"),
        ACTOR,
    )
    .await
    .unwrap();
    let layer = create_lopa_layer_inner(&pool, ORG, &layer_input(s.id, 1, "ipl"), ACTOR)
        .await
        .unwrap();
    // 注：删 scenario 会 CASCADE 删除 layer，但 audit_log 不受影响（target_id 已知）
    delete_lopa_scenario_inner(&pool, ORG, s.id, ACTOR).await.unwrap();

    let hist = list_history_for_target_inner(&pool, ORG, "lopa_scenario", s.id, None)
        .await
        .unwrap();
    let actions: Vec<&str> = hist.iter().map(|a| a.action.as_str()).collect();
    assert!(actions.contains(&"lopa_scenario_create"), "应有 create 记录");
    assert!(actions.contains(&"lopa_scenario_update"), "应有 update 记录");
    assert!(actions.contains(&"lopa_scenario_delete"), "应有 delete 记录");

    // layer create 的审计记录在 lopa_layer 表（target_id = layer.id）
    let layer_hist = list_history_for_target_inner(&pool, ORG, "lopa_layer", layer.id, None)
        .await
        .unwrap();
    let layer_actions: Vec<&str> = layer_hist.iter().map(|a| a.action.as_str()).collect();
    assert!(
        layer_actions.contains(&"lopa_layer_create"),
        "layer create 也应入审计"
    );

    // 跨 org 看不到审计（org_id 作用域隔离）
    let hist_other = list_history_for_target_inner(&pool, ORG_OTHER, "lopa_scenario", s.id, None)
        .await
        .unwrap();
    assert!(hist_other.is_empty(), "跨 org 不应看到审计历史");
}
