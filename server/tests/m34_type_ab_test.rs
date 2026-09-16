//! m34 — Type A/B 架构约束（IEC 61508-2 表 2/3）+ 仪表编辑后 SFF 再校验
//!
//! 第 5 项：Type A（简单元件）/ Type B（复杂元件）SFF 分档表不同
//!   Type A, SFF<60% : HFT0=SIL1
//!   Type B, SFF<60% : HFT0=不允许
//!   60~<90 档       : Type A HFT0=SIL2 / Type B HFT0=SIL1
//! 第 6 项：update_instrument 改低 SFF / 改设备类型时，
//!   已关联 SIF 若不再满足架构约束 → 拒绝修改

use sif_studio_lib::commands::instruments::{create_instrument_inner, update_instrument_inner, InstrumentInput};
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
            code: "PRJ-AB".into(),
            name: "Type A/B 测试".into(),
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

fn sif_input(proj_id: i64, code: &str, sil: &str) -> SifInput {
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
        sensor_arch: "1oo1".into(),
        logic_arch: "1oo1".into(),
        final_arch: "1oo1".into(),
        mttr_hours: 8.0,
        beta_factor: 0.10,
        ..Default::default()
    }
}

fn inst_input(proj_id: i64, tag: &str, sff: f64, etype: &str) -> InstrumentInput {
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
        sff,
        pt_coverage: 1.0,
        hft: 0,
        equipment_type: etype.into(),
    }
}

/// SIL B（SIL2）+ 1oo1 + SFF=0.80（60~<90 档）：
/// Type A HFT=0 最高 SIL 2 → 允许；Type B HFT=0 最高 SIL 1 → 拒绝
#[tokio::test]
async fn type_a_vs_type_b_same_sff_band_60_90() {
    let (pool, pid) = setup().await;
    let sif = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-AB", "B"), ACTOR).await.unwrap();

    // Type A：60~<90 档 HFT=0 → SIL 2 → 允许
    let a = create_instrument_inner(&pool, ORG, ACTOR, inst_input(pid, "PT-A", 0.80, "type_a")).await.unwrap();
    let r_a = link_instrument_to_sif_inner(&pool, ORG, sif.id, a.id, "detector".into(), 0, None, String::new(), ACTOR).await;
    assert!(r_a.is_ok(), "Type A SFF 0.80 HFT0 can claim SIL2 (table 2)");

    // 同 SIF 已关联 Type A；Type B 仪表 0.80 → 最高 SIL 1 → 拒绝
    let b = create_instrument_inner(&pool, ORG, ACTOR, inst_input(pid, "PT-B", 0.80, "type_b")).await.unwrap();
    let r_b = link_instrument_to_sif_inner(&pool, ORG, sif.id, b.id, "detector".into(), 0, None, String::new(), ACTOR).await;
    assert!(r_b.is_err(), "Type B SFF 0.80 HFT0 max SIL1 < SIL2 (table 3)");
    assert!(r_b.unwrap_err().to_string().contains("SIL"));
}

/// Type B SFF<60% + HFT=0：连 SIL 1 都不允许
#[tokio::test]
async fn type_b_low_sff_not_allowed_even_sil1() {
    let (pool, pid) = setup().await;
    let sif = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-LOW", "A"), ACTOR).await.unwrap();

    let d = create_instrument_inner(&pool, ORG, ACTOR, inst_input(pid, "PT-LOW", 0.50, "type_b")).await.unwrap();
    let r = link_instrument_to_sif_inner(&pool, ORG, sif.id, d.id, "detector".into(), 0, None, String::new(), ACTOR).await;
    assert!(r.is_err(), "Type B SFF<60% HFT0 must not claim any SIL");
    assert!(r.unwrap_err().to_string().contains("不允许声明任何 SIL"));
}

/// Type A SFF<60% + HFT=0：可声明 SIL 1（与 Type B 的关键区别）
#[tokio::test]
async fn type_a_low_sff_allows_sil1() {
    let (pool, pid) = setup().await;
    let sif = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-ALOW", "A"), ACTOR).await.unwrap();

    let d = create_instrument_inner(&pool, ORG, ACTOR, inst_input(pid, "PT-ALOW", 0.50, "type_a")).await.unwrap();
    let r = link_instrument_to_sif_inner(&pool, ORG, sif.id, d.id, "detector".into(), 0, None, String::new(), ACTOR).await;
    assert!(r.is_ok(), "Type A SFF<60% HFT0 can claim SIL1 (table 2)");
}

/// 第 6 项：已关联 SIL B 的 Type B 仪表（SFF=0.96），改低到 0.80 → 拒绝
#[tokio::test]
async fn lowering_sff_of_linked_instrument_blocked() {
    let (pool, pid) = setup().await;
    let sif = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-ED", "B"), ACTOR).await.unwrap();
    let d = create_instrument_inner(&pool, ORG, ACTOR, inst_input(pid, "PT-ED", 0.96, "type_b")).await.unwrap();
    link_instrument_to_sif_inner(&pool, ORG, sif.id, d.id, "detector".into(), 0, None, String::new(), ACTOR).await.unwrap();

    let mut bad = inst_input(pid, "PT-ED", 0.80, "type_b");
    bad.tag = "PT-ED".into();
    let r = update_instrument_inner(&pool, ORG, ACTOR, d.id, bad).await;
    assert!(r.is_err(), "lowering SFF to 0.80 must be blocked for linked SIL2 SIF");
    let msg = r.unwrap_err().to_string();
    assert!(msg.contains("SIF-ED"), "error should identify affected SIF, got: {msg}");
}

/// 第 6 项：SFF 提升（0.96→0.99）允许
#[tokio::test]
async fn raising_sff_of_linked_instrument_allowed() {
    let (pool, pid) = setup().await;
    let sif = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-UP", "B"), ACTOR).await.unwrap();
    let d = create_instrument_inner(&pool, ORG, ACTOR, inst_input(pid, "PT-UP", 0.96, "type_b")).await.unwrap();
    link_instrument_to_sif_inner(&pool, ORG, sif.id, d.id, "detector".into(), 0, None, String::new(), ACTOR).await.unwrap();

    let good = inst_input(pid, "PT-UP", 0.99, "type_b");
    let r = update_instrument_inner(&pool, ORG, ACTOR, d.id, good).await;
    assert!(r.is_ok(), "raising SFF should be allowed");
}

/// 第 6 项：Type A（SFF=0.80，可声明 SIL2）改成 Type B → 分档掉级 → 拒绝
#[tokio::test]
async fn switching_type_a_to_type_b_blocked_when_linked() {
    let (pool, pid) = setup().await;
    let sif = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-TC", "B"), ACTOR).await.unwrap();
    let d = create_instrument_inner(&pool, ORG, ACTOR, inst_input(pid, "PT-TC", 0.80, "type_a")).await.unwrap();
    link_instrument_to_sif_inner(&pool, ORG, sif.id, d.id, "detector".into(), 0, None, String::new(), ACTOR).await.unwrap();

    let bad = inst_input(pid, "PT-TC", 0.80, "type_b");
    let r = update_instrument_inner(&pool, ORG, ACTOR, d.id, bad).await;
    assert!(r.is_err(), "Type A→B at SFF 0.80 drops max SIL from 2 to 1, must be blocked");
}

/// 第 6 项：SFF 不变（只改 notes）不触发再校验，编辑成功
#[tokio::test]
async fn non_sff_edit_not_affected() {
    let (pool, pid) = setup().await;
    let sif = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-NE", "B"), ACTOR).await.unwrap();
    let d = create_instrument_inner(&pool, ORG, ACTOR, inst_input(pid, "PT-NE", 0.96, "type_b")).await.unwrap();
    link_instrument_to_sif_inner(&pool, ORG, sif.id, d.id, "detector".into(), 0, None, String::new(), ACTOR).await.unwrap();

    let mut edited = inst_input(pid, "PT-NE", 0.96, "type_b");
    edited.notes = "仅改备注".into();
    let r = update_instrument_inner(&pool, ORG, ACTOR, d.id, edited).await;
    assert!(r.is_ok(), "notes-only edit should pass");
}

/// 第 6 项：SIL=NA 的已关联 SIF 不受 SFF 编辑限制
#[tokio::test]
async fn sff_edit_unrestricted_for_sil_na_sif() {
    let (pool, pid) = setup().await;
    let sif = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-NAN", "NA"), ACTOR).await.unwrap();
    let d = create_instrument_inner(&pool, ORG, ACTOR, inst_input(pid, "PT-NAN", 0.96, "type_b")).await.unwrap();
    link_instrument_to_sif_inner(&pool, ORG, sif.id, d.id, "detector".into(), 0, None, String::new(), ACTOR).await.unwrap();

    let edited = inst_input(pid, "PT-NAN", 0.30, "type_b");
    let r = update_instrument_inner(&pool, ORG, ACTOR, d.id, edited).await;
    assert!(r.is_ok(), "SFF edit must be unrestricted when linked SIF is SIL=NA");
}
