//! m37 — Route 1H 架构与实际通道数一致性（IEC 61508-2 表 2/3 配套）
//!
//! 背景：计算引擎在通道数与表决架构不符时静默退化为串联公式（sifs.rs
//! subsystem_pfd 的 `_` 分支）。本组测试锁定 arch_channel_status 三态：
//!   matched  = 通道数 == 架构要求（表决公式成立）
//!   degraded = 通道数不足或超配（结果失真，前端红牌提示）
//!   empty    = 子系统无仪表
//! 并验证退化公式的数值（串联 PFD ≈ λDU·TI/2）与架构变更后的状态刷新。

use sif_studio_lib::commands::instruments::{create_instrument_inner, InstrumentInput};
use sif_studio_lib::commands::projects::{create_project_inner, ProjectInput};
use sif_studio_lib::commands::sifs::{
    create_sif_inner, link_instrument_to_sif_inner, list_sifs_inner, update_sif_inner, SifInput,
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
            code: "PRJ-1H".into(),
            name: "Route 1H 测试".into(),
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

fn sif_input(proj_id: i64, code: &str, sensor_arch: &str) -> SifInput {
    SifInput {
        project_id: proj_id,
        code: code.into(),
        name: format!("SIF {code}"),
        description: String::new(),
        sil_design: "A".into(),
        sil_verified: "A".into(),
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
        // λDU=1e-6/h；Type A + SFF 0.80（60~<90 档）→ HFT1 最高 SIL3，覆盖 SIL A 声明
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

async fn link_det(pool: &Pool<Sqlite>, sif_id: i64, inst_id: i64, role: &str) {
    link_instrument_to_sif_inner(
        pool,
        ORG,
        sif_id,
        inst_id,
        role.into(),
        0,
        None,
        String::new(),
        ACTOR,
    )
    .await
    .expect("link");
}

async fn summary_of(pool: &Pool<Sqlite>, pid: i64, code: &str) -> sif_studio_lib::commands::sifs::SifSummary {
    list_sifs_inner(pool, ORG, Some(pid))
        .await
        .expect("list")
        .into_iter()
        .find(|s| s.code == code)
        .unwrap_or_else(|| panic!("sif {code} not found"))
}

/// 新建 SIF 无任何仪表 → 三子系统均为 empty
#[tokio::test]
async fn fresh_sif_all_subsystems_empty() {
    let (pool, pid) = setup().await;
    create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-EMPTY", "1oo2"), ACTOR)
        .await
        .unwrap();
    let s = summary_of(&pool, pid, "SIF-EMPTY").await;
    assert_eq!(s.sensor_match, "empty");
    assert_eq!(s.logic_match, "empty");
    assert_eq!(s.final_match, "empty");
    assert!(s.pfdavg_calculated.is_none(), "无数据不计算 PFD");
}

/// 1oo2 + 2 台检测仪表 → matched，PFD 用表决公式（含 β 项），
/// 数量级远低于串联退化值
#[tokio::test]
async fn one_oo2_with_two_detectors_matched() {
    let (pool, pid) = setup().await;
    let sif = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-M2", "1oo2"), ACTOR)
        .await
        .unwrap();
    let d1 = create_instrument_inner(&pool, ORG, ACTOR, det_input(pid, "PT-1")).await.unwrap();
    let d2 = create_instrument_inner(&pool, ORG, ACTOR, det_input(pid, "PT-2")).await.unwrap();
    link_det(&pool, sif.id, d1.id, "detector").await;
    link_det(&pool, sif.id, d2.id, "detector").await;

    let s = summary_of(&pool, pid, "SIF-M2").await;
    assert_eq!(s.detector_count, 2);
    assert_eq!(s.sensor_match, "matched");
    assert_eq!(s.logic_match, "empty");
    assert_eq!(s.final_match, "empty");

    // 1oo2 表决公式（PTC=1.0 → λDU_eff = 0.5e-6，TI = 12×30×24 = 8640h）：
    //   (λDU_eff·TI)²/3 + β·λDU_eff·TI/2
    //   = (0.5e-6·8640)²/3 + 0.1·0.5e-6·4320 = 6.22e-6 + 2.16e-4 ≈ 2.222e-4
    let pfd = s.pfdavg_calculated.expect("pfd computed");
    assert!(
        (pfd - 2.222e-4).abs() / 2.222e-4 < 0.02,
        "1oo2 voting formula expected ~2.222e-4, got {pfd:.3e}"
    );
}

/// 1oo2 + 仅 1 台检测仪表 → degraded，PFD 静默退化为串联 λDU_eff·TI/2 = 2.16e-3
#[tokio::test]
async fn one_oo2_with_single_detector_degrades() {
    let (pool, pid) = setup().await;
    let sif = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-D1", "1oo2"), ACTOR)
        .await
        .unwrap();
    let d1 = create_instrument_inner(&pool, ORG, ACTOR, det_input(pid, "PT-D1")).await.unwrap();
    link_det(&pool, sif.id, d1.id, "detector").await;

    let s = summary_of(&pool, pid, "SIF-D1").await;
    assert_eq!(s.detector_count, 1);
    assert_eq!(s.sensor_match, "degraded");

    // 退化串联公式（PTC 折入 + TI=8640h）：λDU_eff·TI/2 = 0.5e-6·4320 = 2.16e-3
    let pfd = s.pfdavg_calculated.expect("pfd computed");
    assert!(
        (pfd - 2.16e-3).abs() / 2.16e-3 < 0.02,
        "degraded series formula expected ~2.16e-3, got {pfd:.3e}"
    );
}

/// 1oo1 + 2 台检测仪表（超配）→ degraded（架构声明与实际不符，同样走串联退化）
#[tokio::test]
async fn overprovision_is_degraded() {
    let (pool, pid) = setup().await;
    let sif = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-OP", "1oo1"), ACTOR)
        .await
        .unwrap();
    let d1 = create_instrument_inner(&pool, ORG, ACTOR, det_input(pid, "PT-O1")).await.unwrap();
    let d2 = create_instrument_inner(&pool, ORG, ACTOR, det_input(pid, "PT-O2")).await.unwrap();
    link_det(&pool, sif.id, d1.id, "detector").await;
    link_det(&pool, sif.id, d2.id, "detector").await;

    let s = summary_of(&pool, pid, "SIF-OP").await;
    assert_eq!(s.detector_count, 2);
    assert_eq!(s.sensor_match, "degraded", "1oo1 挂 2 台应标记超配退化");
}

/// 2oo3 + 3 台 → matched；update 改架构为 2oo4 → degraded（状态随架构刷新）
#[tokio::test]
async fn arch_change_refreshes_match_status() {
    let (pool, pid) = setup().await;
    let sif = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-23", "2oo3"), ACTOR)
        .await
        .unwrap();
    let d1 = create_instrument_inner(&pool, ORG, ACTOR, det_input(pid, "PT-A1")).await.unwrap();
    let d2 = create_instrument_inner(&pool, ORG, ACTOR, det_input(pid, "PT-A2")).await.unwrap();
    let d3 = create_instrument_inner(&pool, ORG, ACTOR, det_input(pid, "PT-A3")).await.unwrap();
    link_det(&pool, sif.id, d1.id, "detector").await;
    link_det(&pool, sif.id, d2.id, "detector").await;
    link_det(&pool, sif.id, d3.id, "detector").await;

    let s = summary_of(&pool, pid, "SIF-23").await;
    assert_eq!(s.sensor_match, "matched");

    // 改架构 2oo3 → 2oo4（SIL A + Type A SFF0.80 HFT2 最高 SIL4，可行）
    let upd = sif_input(pid, "SIF-23", "2oo4");
    update_sif_inner(&pool, ORG, sif.id, &upd, ACTOR)
        .await
        .expect("update arch");

    let s2 = summary_of(&pool, pid, "SIF-23").await;
    assert_eq!(s2.sensor_arch, "2oo4");
    assert_eq!(s2.sensor_match, "degraded", "2oo4 要求 4 通道，仅 3 台应退化");
}

/// aux 角色不参与表决通道计数：挂 2 detector + 1 aux → 仍 matched
#[tokio::test]
async fn aux_role_not_counted_as_channel() {
    let (pool, pid) = setup().await;
    let sif = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-AUX", "1oo2"), ACTOR)
        .await
        .unwrap();
    let d1 = create_instrument_inner(&pool, ORG, ACTOR, det_input(pid, "PT-X1")).await.unwrap();
    let d2 = create_instrument_inner(&pool, ORG, ACTOR, det_input(pid, "PT-X2")).await.unwrap();
    let aux = create_instrument_inner(&pool, ORG, ACTOR, det_input(pid, "PT-XA")).await.unwrap();
    link_det(&pool, sif.id, d1.id, "detector").await;
    link_det(&pool, sif.id, d2.id, "detector").await;
    link_det(&pool, sif.id, aux.id, "aux").await;

    let s = summary_of(&pool, pid, "SIF-AUX").await;
    assert_eq!(s.detector_count, 2);
    assert_eq!(s.aux_count, 1);
    assert_eq!(s.sensor_match, "matched", "aux 不得计入表决通道");
}

/// SIL=NA 的 SIF 不做 Route 1H 查表约束，但匹配状态仍然展示（挂 1 台 1oo2 → degraded）
#[tokio::test]
async fn sil_na_sif_still_reports_match_status() {
    let (pool, pid) = setup().await;
    let mut inp = sif_input(pid, "SIF-NAM", "1oo2");
    inp.sil_design = "NA".into();
    inp.sil_verified = "NA".into();
    let sif = create_sif_inner(&pool, ORG, &inp, ACTOR).await.unwrap();
    let d1 = create_instrument_inner(&pool, ORG, ACTOR, det_input(pid, "PT-N1")).await.unwrap();
    link_det(&pool, sif.id, d1.id, "detector").await;

    let s = summary_of(&pool, pid, "SIF-NAM").await;
    assert_eq!(s.sensor_match, "degraded");
}
