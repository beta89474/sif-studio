//! m35 — λDD/MTTR + β 共因 + PFH（高需求）+ silAchieved/silVerified 双源打通
//!
//! 覆盖（IEC 61511-1/2）：
//!   - λDD × MTTR 项：λDU=0、λDD>0 时 PFDavg 仍可计算
//!   - β 共因因子抬高 1oo2 PFDavg / PFH
//!   - 高需求模式返回 PFH 并按 PFH 阈值表定级
//!   - silVerified 高于 silAchieved → update 拒绝（超标拒绝）
//!   - verifyStatus：pending/unverified/verified/downgraded/overclaimed
//!   - MTTR / β 参数域校验

use sif_studio_lib::commands::instruments::{create_instrument_inner, InstrumentInput};
use sif_studio_lib::commands::projects::{create_project_inner, ProjectInput};
use sif_studio_lib::commands::sifs::{
    compute_sif_metrics, create_sif_inner, link_instrument_to_sif_inner, list_sifs_inner,
    sil_verify_status, update_sif_inner, SifInput,
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
            code: "PRJ-M35".into(),
            name: "MTTR/β/PFH 测试项目".into(),
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

fn sif_input(
    proj_id: i64,
    code: &str,
    demand: &str,
    verified: &str,
    s_arch: &str,
) -> SifInput {
    SifInput {
        project_id: proj_id,
        code: code.into(),
        name: format!("SIF {code}"),
        description: String::new(),
        sil_design: "B".into(),
        sil_verified: verified.into(),
        demand_mode: demand.into(),
        pfdavg_target: Some(0.01),
        proof_interval: 12,
        sensor_arch: s_arch.into(),
        logic_arch: "1oo1".into(),
        final_arch: "1oo1".into(),
        mttr_hours: 8.0,
        beta_factor: 0.10,
        ..Default::default()
    }
}

fn inst_input(
    proj_id: i64,
    tag: &str,
    role: &str,
    ldu: f64,
    ldd: f64,
) -> InstrumentInput {
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
        lambda_dd: ldd,
        lambda_su: 0.0,
        lambda_sd: 0.0,
        sff: 0.96,
        pt_coverage: 1.0,
        hft: 0,
        equipment_type: "type_b".into(),
    }
}

/// λDD × MTTR：λDU=0 但 λDD=1e-3 → PFD = λDD × MTTR = 8e-3；PFH 仍为 None
#[tokio::test]
async fn mttr_dd_term_counts_without_lambda_du() {
    let (pool, pid) = setup().await;
    let sif = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-MTTR", "low", "NA", "1oo1"), ACTOR)
        .await
        .unwrap();
    let d = create_instrument_inner(&pool, ORG, ACTOR, inst_input(pid, "PT-1", "detector", 0.0, 1e-3))
        .await
        .unwrap();
    link_instrument_to_sif_inner(&pool, ORG, sif.id, d.id, "detector".into(), 0, None, String::new(), ACTOR)
        .await
        .unwrap();

    let calc = compute_sif_metrics(&pool, ORG, sif.id, 12, 8.0, 0.10, "low", "1oo1", "1oo1", "1oo1")
        .await
        .unwrap();
    let pfd = calc.pfd.expect("λDD 数据也应能算 PFDavg");
    assert!((pfd - 8.0e-3).abs() < 1e-12, "pfd={pfd}");
    // 8e-3 ∈ [1e-3,1e-2) → SIL 2 → B
    assert_eq!(calc.sil_achieved, "B");
    // PFH 只依赖 λDU → None
    assert!(calc.pfh.is_none(), "无 λDU 时 PFH 应为 None");

    // MTTR 翻倍 → DD 项翻倍
    let calc2 = compute_sif_metrics(&pool, ORG, sif.id, 12, 16.0, 0.10, "low", "1oo1", "1oo1", "1oo1")
        .await
        .unwrap();
    assert!((calc2.pfd.unwrap() - 1.6e-2).abs() < 1e-12);
}

/// β 共因：1oo2 PFDavg 随 β 增大而增大，β=0 时无共因项
#[tokio::test]
async fn beta_factor_raises_1oo2_pfdavg() {
    let (pool, pid) = setup().await;
    let sif = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-B", "low", "NA", "1oo2"), ACTOR)
        .await
        .unwrap();
    let d1 = create_instrument_inner(&pool, ORG, ACTOR, inst_input(pid, "PT-1", "detector", 1e-6, 0.0))
        .await
        .unwrap();
    let d2 = create_instrument_inner(&pool, ORG, ACTOR, inst_input(pid, "PT-2", "detector", 1e-6, 0.0))
        .await
        .unwrap();
    link_instrument_to_sif_inner(&pool, ORG, sif.id, d1.id, "detector".into(), 0, None, String::new(), ACTOR)
        .await
        .unwrap();
    link_instrument_to_sif_inner(&pool, ORG, sif.id, d2.id, "detector".into(), 1, None, String::new(), ACTOR)
        .await
        .unwrap();

    let avg_du: f64 = 1e-6 * 0.5; // PTC=1.0
    let ti: f64 = 12.0 * 30.0 * 24.0;
    let vote = (avg_du * ti).powi(2) / 3.0;

    let no_beta = compute_sif_metrics(&pool, ORG, sif.id, 12, 8.0, 0.0, "low", "1oo2", "1oo1", "1oo1")
        .await
        .unwrap();
    assert!((no_beta.pfd.unwrap() - vote).abs() < 1e-18, "β=0 应只剩表决项");

    let with_beta = compute_sif_metrics(&pool, ORG, sif.id, 12, 8.0, 0.10, "low", "1oo2", "1oo1", "1oo1")
        .await
        .unwrap();
    let expected = vote + 0.10 * avg_du * ti / 2.0;
    assert!(
        (with_beta.pfd.unwrap() - expected).abs() < 1e-18,
        "β 项应叠加到 PFDavg"
    );
}

/// 高需求模式：1oo1 PFH = λDU；按 PFH 阈值表定级
#[tokio::test]
async fn pfh_high_demand_classification() {
    let (pool, pid) = setup().await;
    // λDU = 5e-6/h ∈ [1e-6,1e-5) → SIL 1（A）
    let sif = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-PFH", "high", "NA", "1oo1"), ACTOR)
        .await
        .unwrap();
    let d = create_instrument_inner(&pool, ORG, ACTOR, inst_input(pid, "PT-1", "detector", 5e-6, 0.0))
        .await
        .unwrap();
    link_instrument_to_sif_inner(&pool, ORG, sif.id, d.id, "detector".into(), 0, None, String::new(), ACTOR)
        .await
        .unwrap();

    let calc = compute_sif_metrics(&pool, ORG, sif.id, 12, 8.0, 0.10, "high", "1oo1", "1oo1", "1oo1")
        .await
        .unwrap();
    let pfh = calc.pfh.expect("高需求模式应返回 PFH");
    assert!((pfh - 5e-6).abs() < 1e-15, "pfh={pfh}");
    assert_eq!(calc.sil_achieved, "A");

    // 1oo2：PFH 由共因 β 项主导
    let sif2 = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-PFH2", "high", "NA", "1oo2"), ACTOR)
        .await
        .unwrap();
    let d1 = create_instrument_inner(&pool, ORG, ACTOR, inst_input(pid, "PT-2", "detector", 1e-6, 0.0))
        .await
        .unwrap();
    let d2 = create_instrument_inner(&pool, ORG, ACTOR, inst_input(pid, "PT-3", "detector", 1e-6, 0.0))
        .await
        .unwrap();
    link_instrument_to_sif_inner(&pool, ORG, sif2.id, d1.id, "detector".into(), 0, None, String::new(), ACTOR)
        .await
        .unwrap();
    link_instrument_to_sif_inner(&pool, ORG, sif2.id, d2.id, "detector".into(), 1, None, String::new(), ACTOR)
        .await
        .unwrap();

    let ti: f64 = 12.0 * 30.0 * 24.0;
    let window = ti / 2.0 + 8.0;
    let expected_pfh = 2.0 * (1.0 - 0.10) * 1e-6f64.powi(2) * window + 0.10 * 1e-6;
    let calc2 = compute_sif_metrics(&pool, ORG, sif2.id, 12, 8.0, 0.10, "high", "1oo2", "1oo1", "1oo1")
        .await
        .unwrap();
    assert!((calc2.pfh.unwrap() - expected_pfh).abs() < 1e-15);
    // 1.08e-7/h ∈ [1e-7,1e-6) → SIL 2（B）
    assert_eq!(calc2.sil_achieved, "B");
}

/// 超标拒绝：silVerified 高于 silAchieved 时 update 必须被拒绝
#[tokio::test]
async fn overclaimed_sil_verified_is_rejected() {
    let (pool, pid) = setup().await;
    // 建 SIF 时无仪表（设计阶段），即使 silVerified=NA 也允许
    let sif = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-OC", "low", "NA", "1oo1"), ACTOR)
        .await
        .unwrap();
    // λDU=1e-6、PTC=1.0 → PFD=2.16e-3 → silAchieved = B（SIL 2）
    let d = create_instrument_inner(&pool, ORG, ACTOR, inst_input(pid, "PT-1", "detector", 1e-6, 0.0))
        .await
        .unwrap();
    link_instrument_to_sif_inner(&pool, ORG, sif.id, d.id, "detector".into(), 0, None, String::new(), ACTOR)
        .await
        .unwrap();

    // 声明验证 SIL 3（C）> 可达 SIL 2（B）→ 拒绝
    let mut over = sif_input(pid, "SIF-OC", "low", "C", "1oo1");
    over.code = sif.code.clone();
    over.name = sif.name.clone();
    let err = update_sif_inner(&pool, ORG, sif.id, &over, ACTOR).await.unwrap_err();
    assert!(
        err.to_string().contains("silVerified") || err.to_string().contains("验证 SIL"),
        "错误信息应说明超标，实际: {err}"
    );

    // 声明等于可达值 → 通过
    let mut ok = sif_input(pid, "SIF-OC", "low", "B", "1oo1");
    ok.code = sif.code.clone();
    ok.name = sif.name.clone();
    assert!(update_sif_inner(&pool, ORG, sif.id, &ok, ACTOR).await.is_ok());
}

/// verifyStatus 各状态（list_sifs_inner 输出 + 纯函数）
#[tokio::test]
async fn sil_verify_status_lifecycle() {
    let (pool, pid) = setup().await;

    // 1) 无仪表 → pending
    let sif = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-VS", "low", "NA", "1oo1"), ACTOR)
        .await
        .unwrap();
    let rows = list_sifs_inner(&pool, ORG, None).await.unwrap();
    assert_eq!(
        rows.iter().find(|r| r.id == sif.id).unwrap().sil_verify_status,
        "pending"
    );

    // 挂仪表后可达 SIL B，silVerified=NA → unverified
    let d = create_instrument_inner(&pool, ORG, ACTOR, inst_input(pid, "PT-1", "detector", 1e-6, 0.0))
        .await
        .unwrap();
    link_instrument_to_sif_inner(&pool, ORG, sif.id, d.id, "detector".into(), 0, None, String::new(), ACTOR)
        .await
        .unwrap();
    let rows = list_sifs_inner(&pool, ORG, None).await.unwrap();
    let row = rows.iter().find(|r| r.id == sif.id).unwrap();
    assert_eq!(row.sil_verify_status, "unverified");
    // 引擎始终同时返回 PFD 与 PFH（只要有 λDU 数据）；定级按 demandMode 选择
    assert!(row.pfdavg_calculated.is_some());
    assert!(row.pfh_calculated.is_some());

    // 更新 silVerified=B（=achieved）→ verified
    let mut eq = sif_input(pid, "SIF-VS", "low", "B", "1oo1");
    eq.code = sif.code.clone();
    eq.name = sif.name.clone();
    update_sif_inner(&pool, ORG, sif.id, &eq, ACTOR).await.unwrap();
    let rows = list_sifs_inner(&pool, ORG, None).await.unwrap();
    assert_eq!(
        rows.iter().find(|r| r.id == sif.id).unwrap().sil_verify_status,
        "verified"
    );

    // 更新 silVerified=A（< achieved B）→ downgraded
    let mut down = sif_input(pid, "SIF-VS", "low", "A", "1oo1");
    down.code = sif.code.clone();
    down.name = sif.name.clone();
    update_sif_inner(&pool, ORG, sif.id, &down, ACTOR).await.unwrap();
    let rows = list_sifs_inner(&pool, ORG, None).await.unwrap();
    assert_eq!(
        rows.iter().find(|r| r.id == sif.id).unwrap().sil_verify_status,
        "downgraded"
    );

    // 纯函数：overclaimed / 边界
    assert_eq!(sil_verify_status("C", "B", true), "overclaimed");
    assert_eq!(sil_verify_status("C", "NA", true), "overclaimed");
    assert_eq!(sil_verify_status("B", "B", false), "pending");
    assert_eq!(sil_verify_status("NA", "B", true), "unverified");
}

/// MTTR / β 参数域校验
#[tokio::test]
async fn mttr_beta_domain_validation() {
    let (pool, pid) = setup().await;
    let sif = create_sif_inner(&pool, ORG, &sif_input(pid, "SIF-DOM", "low", "NA", "1oo1"), ACTOR)
        .await
        .unwrap();

    let mut bad_beta = sif_input(pid, "SIF-DOM", "low", "NA", "1oo1");
    bad_beta.beta_factor = 1.5;
    bad_beta.code = sif.code.clone();
    bad_beta.name = sif.name.clone();
    assert!(update_sif_inner(&pool, ORG, sif.id, &bad_beta, ACTOR).await.is_err());

    let mut bad_mttr = sif_input(pid, "SIF-DOM", "low", "NA", "1oo1");
    bad_mttr.mttr_hours = -1.0;
    bad_mttr.code = sif.code.clone();
    bad_mttr.name = sif.name.clone();
    assert!(update_sif_inner(&pool, ORG, sif.id, &bad_mttr, ACTOR).await.is_err());

    let mut bad_mode = sif_input(pid, "SIF-DOM", "low", "NA", "1oo1");
    bad_mode.demand_mode = "seasonal".into();
    bad_mode.code = sif.code.clone();
    bad_mode.name = sif.name.clone();
    assert!(update_sif_inner(&pool, ORG, sif.id, &bad_mode, ACTOR).await.is_err());
}
