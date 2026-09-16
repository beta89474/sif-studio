//! m33 — PTC 折入 + SFF/HFT 可行性校验（IEC 61511-2 §6 / 表 3）
//!
//! 覆盖：
//!   - PTC < 1 时 PFDavg 比 PTC=1 更大（uncovered 失效按全 TI 计入）
//!   - PTC=1 时退化为原公式
//!   - SFF 不足 → link 被拒绝
//!   - SFF 充足 → link 成功
//!   - SIL=NA 时不校验 SFF
//!   - update SIF 提升 SIL 后 SFF 不足 → 拒绝

use sif_studio_lib::commands::instruments::{create_instrument_inner, InstrumentInput};
use sif_studio_lib::commands::projects::{create_project_inner, ProjectInput};
use sif_studio_lib::commands::sifs::{
    create_sif_inner, link_instrument_to_sif_inner, list_sifs_inner,
    update_sif_inner, compute_sif_metrics, SifInput,
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
            code: "PRJ-PTC".into(),
            name: "PTC 测试".into(),
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

fn sif_input(proj_id: i64, code: &str, sil: &str, ti: i64) -> SifInput {
    SifInput {
        project_id: proj_id,
        code: code.into(),
        name: format!("SIF {code}"),
        description: String::new(),
        sil_design: sil.into(),
        sil_verified: sil.into(),
        demand_mode: "low".into(),
        pfdavg_target: Some(0.01),
        proof_interval: ti,
        sensor_arch: "1oo1".into(),
        logic_arch: "1oo1".into(),
        final_arch: "1oo1".into(),
        mttr_hours: 8.0,
        beta_factor: 0.10,
        ..Default::default()
    }
}

fn inst_input(proj_id: i64, tag: &str, role: &str, ldu: f64, ptc: f64, sff: f64) -> InstrumentInput {
    InstrumentInput {
        tag: tag.into(),
        service: String::new(),
        kind: "PT".into(),
        role: role.into(),
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
        lambda_du: ldu,
        lambda_dd: 0.0,
        lambda_su: 0.0,
        lambda_sd: 0.0,
        sff,
        pt_coverage: ptc,
        hft: 0,
        equipment_type: "type_b".into(),
    }
}

/// PTC=1 vs PTC=0.5：PTC 越低 PFDavg 越大
#[tokio::test]
async fn ptc_lower_increases_pfdavg() {
    let (pool, pid) = setup().await;
    let sif = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-PTC", "B", 12), ACTOR)
        .await
        .unwrap();

    // PTC=1.0 → λDU_eff = λDU × (1-0.5) = λDU × 0.5
    let d_full = create_instrument_inner(&pool, ORG, ACTOR, inst_input(pid, "PT-1", "detector", 1e-6, 1.0, 0.95)).await.unwrap();
    link_instrument_to_sif_inner(&pool, ORG, sif.id, d_full.id, "detector".into(), 0, None, String::new(), ACTOR).await.unwrap();

    let calc_full = compute_sif_metrics(&pool, ORG, sif.id, 12, 8.0, 0.10, "low", "1oo1", "1oo1", "1oo1").await.unwrap();
    let pfd_full = calc_full.pfd.unwrap();

    // PTC=0.5 → λDU_eff = λDU × (1-0.25) = λDU × 0.75
    let d_half = create_instrument_inner(&pool, ORG, ACTOR, inst_input(pid, "PT-2", "detector", 1e-6, 0.5, 0.95)).await.unwrap();
    link_instrument_to_sif_inner(&pool, ORG, sif.id, d_half.id, "detector".into(), 0, None, String::new(), ACTOR).await.unwrap();

    let calc_half = compute_sif_metrics(&pool, ORG, sif.id, 12, 8.0, 0.10, "low", "1oo1", "1oo1", "1oo1").await.unwrap();
    let pfd_half = calc_half.pfd.unwrap();

    // PTC=0.5 的等效 λDU 更大 → PFD 更大
    assert!(pfd_half > pfd_full, "pfd_half={pfd_half} should be > pfd_full={pfd_full}");

    // PTC=1.0: PFD = 1e-6 × 0.5 × 8640 / 2 = 2.16e-3
    let expected_full = 1e-6 * 0.5 * 8640.0 / 2.0;
    assert!((pfd_full - expected_full).abs() < 1e-12, "pfd_full={pfd_full} expected={expected_full}");
}

/// PTC=0 → uncovered 全部失效按全 TI 计入，PFD = λDU × TI
#[tokio::test]
async fn ptc_zero_uses_full_ti() {
    let (pool, pid) = setup().await;
    let sif = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-PTC0", "B", 12), ACTOR)
        .await
        .unwrap();

    let d = create_instrument_inner(&pool, ORG, ACTOR, inst_input(pid, "PT-1", "detector", 1e-6, 0.0, 0.95)).await.unwrap();
    link_instrument_to_sif_inner(&pool, ORG, sif.id, d.id, "detector".into(), 0, None, String::new(), ACTOR).await.unwrap();

    let calc = compute_sif_metrics(&pool, ORG, sif.id, 12, 8.0, 0.10, "low", "1oo1", "1oo1", "1oo1").await.unwrap();
    let pfd = calc.pfd.unwrap();
    // PTC=0 → λDU_eff = λDU × 1.0 → PFD = 1e-6 × 8640 / 2 = 4.32e-3
    let expected = 1e-6 * 8640.0 / 2.0;
    assert!((pfd - expected).abs() < 1e-12, "pfd={pfd} expected={expected}");
}

/// SFF 不足 → link 被拒绝（SIL B + 1oo1 → min SFF = 0.95）
#[tokio::test]
async fn sff_below_minimum_blocks_link() {
    let (pool, pid) = setup().await;
    // SIL B (SIL 2) + 1oo1 (HFT=0)，Type B 仪表 SFF 60~<90 档最高只能 SIL 1
    let sif = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-SFF", "B", 12), ACTOR)
        .await
        .unwrap();

    // 仪表 SFF=0.80（60~<90 档），HFT=0 最高 SIL 1 < SIL 2 → 应拒绝
    let d = create_instrument_inner(&pool, ORG, ACTOR, inst_input(pid, "PT-LOW", "detector", 1e-6, 1.0, 0.80)).await.unwrap();
    let res = link_instrument_to_sif_inner(&pool, ORG, sif.id, d.id, "detector".into(), 0, None, String::new(), ACTOR).await;
    assert!(res.is_err(), "Type B SFF 0.80 HFT=0 max SIL1, target SIL2 should be rejected");
    let err = res.unwrap_err();
    assert!(err.to_string().contains("SFF"), "error should mention SFF, got: {err}");
}

/// SFF 充足 → link 成功
#[tokio::test]
async fn sff_above_minimum_allows_link() {
    let (pool, pid) = setup().await;
    let sif = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-SFF2", "B", 12), ACTOR)
        .await
        .unwrap();

    // SFF=0.96（90~<99 档），Type B HFT=0 最高 SIL 2 ≥ SIL 2 → 成功
    let d = create_instrument_inner(&pool, ORG, ACTOR, inst_input(pid, "PT-OK", "detector", 1e-6, 1.0, 0.96)).await.unwrap();
    let res = link_instrument_to_sif_inner(&pool, ORG, sif.id, d.id, "detector".into(), 0, None, String::new(), ACTOR).await;
    assert!(res.is_ok(), "Type B SFF 0.96 band 90-99 HFT0 max SIL2 should be allowed");
}

/// SIL=NA → 不校验 SFF（SFF=0 也可关联）
#[tokio::test]
async fn sil_na_skips_sff_check() {
    let (pool, pid) = setup().await;
    let sif = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-NA", "NA", 12), ACTOR)
        .await
        .unwrap();

    let d = create_instrument_inner(&pool, ORG, ACTOR, inst_input(pid, "PT-NA", "detector", 1e-6, 1.0, 0.0)).await.unwrap();
    let res = link_instrument_to_sif_inner(&pool, ORG, sif.id, d.id, "detector".into(), 0, None, String::new(), ACTOR).await;
    assert!(res.is_ok(), "SIL=NA should skip SFF check");
}

/// update SIF 提升 SIL 后，现有仪表 SFF 分档不够 → 拒绝
#[tokio::test]
async fn update_sif_raising_sil_blocked_by_sff() {
    let (pool, pid) = setup().await;
    // 先建 SIL A (SIL 1) + 1oo1，Type B SFF 60~<90 档 HFT=0 最高 SIL 1
    let sif = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-UP", "A", 12), ACTOR)
        .await
        .unwrap();

    // 仪表 SFF=0.80（60~<90 档），最高 SIL 1 → 关联 SIL A 成功
    let d = create_instrument_inner(&pool, ORG, ACTOR, inst_input(pid, "PT-UP", "detector", 1e-6, 1.0, 0.80)).await.unwrap();
    link_instrument_to_sif_inner(&pool, ORG, sif.id, d.id, "detector".into(), 0, None, String::new(), ACTOR).await.unwrap();

    // 尝试提升到 SIL B（需最高 ≥ SIL 2）→ 该档 HFT=0 仅 SIL 1 → 拒绝
    let mut up = sif_input(pid, "SIF-UP", "B", 12);
    up.code = sif.code.clone();
    up.name = sif.name.clone();
    let res = update_sif_inner(&pool, ORG, sif.id, &up, ACTOR).await;
    assert!(res.is_err(), "raising SIL from A to B with Type B SFF=0.80 HFT0 should be rejected");
}

/// 冗余架构降低 SFF 要求：SIL B + 1oo2 (HFT=1)，Type B 60~<90 档最高 SIL 2
#[tokio::test]
async fn redundant_arch_lowers_sff_requirement() {
    let (pool, pid) = setup().await;
    let mut input = sif_input(pid, "SIF-RED", "B", 12);
    input.sensor_arch = "1oo2".into(); // HFT=1，60~<90 档最高 SIL 2
    let sif = create_sif_inner(&pool, ORG, &input, ACTOR).await.unwrap();

    // SFF=0.80 在 HFT=0 仅 SIL 1（见 sff_below_minimum 用例），HFT=1 可达 SIL 2
    let d1 = create_instrument_inner(&pool, ORG, ACTOR, inst_input(pid, "PT-R1", "detector", 1e-6, 1.0, 0.80)).await.unwrap();
    let d2 = create_instrument_inner(&pool, ORG, ACTOR, inst_input(pid, "PT-R2", "detector", 1e-6, 1.0, 0.80)).await.unwrap();

    let r1 = link_instrument_to_sif_inner(&pool, ORG, sif.id, d1.id, "detector".into(), 0, None, String::new(), ACTOR).await;
    let r2 = link_instrument_to_sif_inner(&pool, ORG, sif.id, d2.id, "detector".into(), 0, None, String::new(), ACTOR).await;
    assert!(r1.is_ok() && r2.is_ok(), "Type B SFF 0.80 band 60-90 HFT1 max SIL2 should pass");
}

/// list_sifs_inner 仍正常返回 pfdavg 字段（PTC 折入后）
#[tokio::test]
async fn list_sifs_returns_pfdavg_with_ptc() {
    let (pool, pid) = setup().await;
    let sif = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-LIST", "B", 12), ACTOR).await.unwrap();
    let d = create_instrument_inner(&pool, ORG, ACTOR, inst_input(pid, "PT-L", "detector", 1e-6, 1.0, 0.96)).await.unwrap();
    link_instrument_to_sif_inner(&pool, ORG, sif.id, d.id, "detector".into(), 0, None, String::new(), ACTOR).await.unwrap();

    let sifs = list_sifs_inner(&pool, ORG, None).await.unwrap();
    assert_eq!(sifs.len(), 1);
    assert!(sifs[0].pfdavg_calculated.is_some());
    assert!(!sifs[0].sil_achieved.is_empty());
}
