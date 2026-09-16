//! m38 — LOPA ↔ SIF gap analysis（IEC 61511-1 Annex E 定级追溯）
//!
//! 验证三方对照状态机：
//!   unlinked     未关联 SIF（定级前场景）
//!   na           LOPA claim=NA（无需 SIS）
//!   pending_data 关联 SIF 但无失效数据
//!   covered      SIF achieved ≥ claim 且 design == claim
//!   gap          SIF achieved < claim（残余风险缺口）
//!   drift        design != claim 但 achieved 当前可覆盖
//! 并验证 RRF 反推 SIL 与手填 claim 不一致时的数据质量告警。

use sif_studio_lib::commands::instruments::{create_instrument_inner, InstrumentInput};
use sif_studio_lib::commands::lopa::{
    create_lopa_scenario_inner, list_lopa_scenarios_inner, LopaScenarioInput,
};
use sif_studio_lib::commands::projects::{create_project_inner, ProjectInput};
use sif_studio_lib::commands::sifs::{
    create_sif_inner, link_instrument_to_sif_inner, SifInput,
};
use sif_studio_lib::db::open_in_memory;
use sqlx::{Pool, Sqlite};

const ORG: i64 = 1;
const ACTOR: &str = "tester";

async fn setup() -> (Pool<Sqlite>, i64) {
    let pool = open_in_memory().await.expect("open");
    let p = create_project_inner(
        &pool,
        ORG,
        &ProjectInput {
            code: "PRJ-GAP".into(),
            name: "LOPA Gap 测试".into(),
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

fn sif_input(proj_id: i64, code: &str, sil: &str, sensor_arch: &str) -> SifInput {
    SifInput {
        project_id: proj_id,
        code: code.into(),
        name: format!("SIF {code}"),
        description: String::new(),
        sil_design: sil.into(),
        sil_verified: sil.into(),
        demand_mode: "low".into(),
        pfdavg_target: Some(0.01),
        proof_interval: 12,
        sensor_arch: sensor_arch.into(),
        logic_arch: "1oo1".into(),
        final_arch: "1oo1".into(),
        mttr_hours: 8.0,
        beta_factor: 0.10,
        ..Default::default()
    }
}

/// Type A SFF=0.80 检测仪表，λDU=1e-6, PTC=1.0
/// → 单台 1oo1 PFD = 0.5e-6×4320 = 2.16e-3（SIL B 区间）
fn det_input(proj_id: i64, tag: &str) -> InstrumentInput {
    InstrumentInput {
        tag: tag.into(),
        service: String::new(),
        kind: "PT".into(),
        role: "detector".into(),
        psv_id: String::new(),
        manufacturer: String::new(),
        model: String::new(),
        range_min: None,
        range_max: None,
        unit: String::new(),
        setpoint: None,
        sil_target: "B".into(),
        proof_interval: 12,
        installed_at: String::new(),
        notes: String::new(),
        project_id: proj_id,
        lambda_du: 1e-6,
        lambda_dd: 0.0,
        lambda_su: 0.0,
        lambda_sd: 0.0,
        sff: 0.80,
        pt_coverage: 1.0,
        hft: 0,
        equipment_type: "type_a".into(),
    }
}

async fn link_det(pool: &Pool<Sqlite>, sif_id: i64, inst_id: i64) {
    link_instrument_to_sif_inner(
        pool, ORG, sif_id, inst_id, "detector".into(), 0, None, String::new(), ACTOR,
    )
    .await
    .expect("link detector");
}

fn scenario(
    proj_id: i64,
    code: &str,
    sif_id: Option<i64>,
    claim: &str,
    init: Option<f64>,
    tol: Option<f64>,
) -> LopaScenarioInput {
    LopaScenarioInput {
        project_id: proj_id,
        sif_id,
        code: code.into(),
        title: format!("场景 {code}"),
        hazard: "h".into(),
        cause: "c".into(),
        consequence: "x".into(),
        severity: "major".into(),
        init_freq: init,
        risk_tol: tol,
        sil_claim: claim.into(),
        notes: String::new(),
    }
}

fn find<'a>(rows: &'a [sif_studio_lib::commands::lopa::LopaScenario], code: &str)
    -> &'a sif_studio_lib::commands::lopa::LopaScenario {
    rows.iter().find(|s| s.code == code).unwrap_or_else(|| panic!("{code} not found"))
}

/// RRF 反推区间映射（不依赖 SIF）
#[tokio::test]
async fn rrf_band_mapping() {
    let (pool, pid) = setup().await;
    // 1e-2 / 1e-3 = 10 → SIL1 下界（A）
    create_lopa_scenario_inner(&pool, ORG, &scenario(pid, "SC-A", None, "A", Some(1e-2), Some(1e-3)), ACTOR).await.unwrap();
    // 1e-2 / 2e-6 = 5000 → SIL3 区间（C）
    create_lopa_scenario_inner(&pool, ORG, &scenario(pid, "SC-C", None, "C", Some(1e-2), Some(2e-6)), ACTOR).await.unwrap();
    // 1e-2 / 1e-2 = 1 → NA
    create_lopa_scenario_inner(&pool, ORG, &scenario(pid, "SC-N", None, "NA", Some(1e-2), Some(1e-2)), ACTOR).await.unwrap();

    let rows = list_lopa_scenarios_inner(&pool, ORG, Some(pid)).await.unwrap();
    assert_eq!(find(&rows, "SC-A").sil_from_rrf.as_deref(), Some("A"));
    assert_eq!(find(&rows, "SC-A").rrf_required, Some(10.0));
    assert_eq!(find(&rows, "SC-C").sil_from_rrf.as_deref(), Some("C"));
    assert_eq!(find(&rows, "SC-C").rrf_required, Some(5000.0));
    assert_eq!(find(&rows, "SC-N").sil_from_rrf.as_deref(), Some("NA"));
}

/// 无 sif 关联 → unlinked
#[tokio::test]
async fn unlinked_status() {
    let (pool, pid) = setup().await;
    create_lopa_scenario_inner(&pool, ORG, &scenario(pid, "SC-U", None, "B", None, None), ACTOR).await.unwrap();
    let rows = list_lopa_scenarios_inner(&pool, ORG, Some(pid)).await.unwrap();
    let s = find(&rows, "SC-U");
    assert_eq!(s.gap_status, "unlinked");
    assert!(s.sif_sil_achieved.is_none());
}

/// claim=NA 且关联了 SIF → na（LOPA 判定无需 SIS）
#[tokio::test]
async fn na_status() {
    let (pool, pid) = setup().await;
    let sif = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-NA", "A", "1oo1"), ACTOR).await.unwrap();
    create_lopa_scenario_inner(&pool, ORG, &scenario(pid, "SC-NA", Some(sif.id), "NA", None, None), ACTOR).await.unwrap();
    let rows = list_lopa_scenarios_inner(&pool, ORG, Some(pid)).await.unwrap();
    assert_eq!(find(&rows, "SC-NA").gap_status, "na");
}

/// 关联 SIF 但无失效数据 → pending_data
#[tokio::test]
async fn pending_data_status() {
    let (pool, pid) = setup().await;
    let sif = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-PD", "B", "1oo1"), ACTOR).await.unwrap();
    create_lopa_scenario_inner(&pool, ORG, &scenario(pid, "SC-PD", Some(sif.id), "B", None, None), ACTOR).await.unwrap();
    let rows = list_lopa_scenarios_inner(&pool, ORG, Some(pid)).await.unwrap();
    let s = find(&rows, "SC-PD");
    assert_eq!(s.gap_status, "pending_data");
    assert!(s.sif_sil_achieved.is_none());
    assert!(s.gap_message.contains("缺失效数据"));
}

/// 1oo1 单台 PFD=2.16e-3 → achieved=B；claim=B + design=B → covered
#[tokio::test]
async fn covered_status() {
    let (pool, pid) = setup().await;
    let sif = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-OK", "B", "1oo1"), ACTOR).await.unwrap();
    let d = create_instrument_inner(&pool, ORG, ACTOR, det_input(pid, "PT-OK")).await.unwrap();
    link_det(&pool, sif.id, d.id).await;

    create_lopa_scenario_inner(&pool, ORG, &scenario(pid, "SC-OK", Some(sif.id), "B", Some(1e-2), Some(1e-4)), ACTOR).await.unwrap();
    let rows = list_lopa_scenarios_inner(&pool, ORG, Some(pid)).await.unwrap();
    let s = find(&rows, "SC-OK");
    assert_eq!(s.gap_status, "covered");
    assert_eq!(s.sif_sil_achieved.as_deref(), Some("B"));
    assert!(s.gap_message.contains("覆盖"));
}

/// SIF design=C / 1oo2 声明（Route 1H HFT1 允许 link），但仅挂 1 台 →
/// 计算退化为串联 PFD=2.16e-3 → achieved=B < claim=C → gap
#[tokio::test]
async fn gap_status_when_achieved_below_claim() {
    let (pool, pid) = setup().await;
    let sif = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-GAP", "C", "1oo2"), ACTOR).await.unwrap();
    let d = create_instrument_inner(&pool, ORG, ACTOR, det_input(pid, "PT-GAP")).await.unwrap();
    link_det(&pool, sif.id, d.id).await;

    create_lopa_scenario_inner(&pool, ORG, &scenario(pid, "SC-GAP", Some(sif.id), "C", Some(1e-2), Some(1e-5)), ACTOR).await.unwrap();
    let rows = list_lopa_scenarios_inner(&pool, ORG, Some(pid)).await.unwrap();
    let s = find(&rows, "SC-GAP");
    assert_eq!(s.gap_status, "gap", "achieved=B 低于 claim=C 必须判 gap");
    assert_eq!(s.sif_sil_achieved.as_deref(), Some("B"));
    assert!(s.gap_message.contains("残余风险缺口"));
}

/// achieved=B 覆盖 claim=A，但 SIF design=B 与 claim=A 不一致 → drift
#[tokio::test]
async fn drift_status_when_design_differs_from_claim() {
    let (pool, pid) = setup().await;
    let sif = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-DR", "B", "1oo1"), ACTOR).await.unwrap();
    let d = create_instrument_inner(&pool, ORG, ACTOR, det_input(pid, "PT-DR")).await.unwrap();
    link_det(&pool, sif.id, d.id).await;

    create_lopa_scenario_inner(&pool, ORG, &scenario(pid, "SC-DR", Some(sif.id), "A", None, None), ACTOR).await.unwrap();
    let rows = list_lopa_scenarios_inner(&pool, ORG, Some(pid)).await.unwrap();
    let s = find(&rows, "SC-DR");
    assert_eq!(s.gap_status, "drift");
    assert_eq!(s.sif_sil_achieved.as_deref(), Some("B"));
    assert!(s.gap_message.contains("不一致"));
}

/// RRF 反推 D 但手填 claim=B → 主状态仍可 covered，message 追加数据质量告警
#[tokio::test]
async fn rrf_claim_mismatch_warns_in_message() {
    let (pool, pid) = setup().await;
    let sif = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-W", "B", "1oo1"), ACTOR).await.unwrap();
    let d = create_instrument_inner(&pool, ORG, ACTOR, det_input(pid, "PT-W")).await.unwrap();
    link_det(&pool, sif.id, d.id).await;

    // 1e-1 / 1e-5 = 10000 → RRF 反推 D
    create_lopa_scenario_inner(&pool, ORG, &scenario(pid, "SC-W", Some(sif.id), "B", Some(1e-1), Some(1e-5)), ACTOR).await.unwrap();
    let rows = list_lopa_scenarios_inner(&pool, ORG, Some(pid)).await.unwrap();
    let s = find(&rows, "SC-W");
    assert_eq!(s.gap_status, "covered", "主状态按 achieved/design 判定");
    assert_eq!(s.sil_from_rrf.as_deref(), Some("D"));
    assert!(s.gap_message.contains("反推"), "message 应包含 RRF 告警：{}", s.gap_message);
}
