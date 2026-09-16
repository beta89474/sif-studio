//! 安全仪表功能（SIF）commands —— 业务核心
//!
//! 设计目标：
//! 1. `list_sifs` 单条 SQL 聚合，返回每条 SIF 的：
//!      - 位号清单（去重，role-tag 索引）、跨图清单
//!      - detector/final/aux 的 COUNT
//!      - 每图一个 diagram_count + 跨图数 SIF 关联图数
//! 2. link/unlink 走 sif_instrument 表（角色 + 端口 + 所在图）
//! 3. 删除 SIF 走 CASCADE，自动清 link/旁路记录/工单
//!
//! 阶段 B：sif / instrument / diagram 三方关联均按 org 作用域校验，
//! 跨组织关联一律 404（不暴露存在性）。
//!
//! 审计（M2.4）：create/update/delete/link/unlink 均写 audit_log。

use crate::commands::audit::{compute_field_diff, entity_audit_payload, snapshot, write_audit};
use crate::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::FromRow;

const SIF_COLS: &str = "id, project_id, code, name, description, sil_design, sil_verified,
     demand_mode, pfdavg_target, proof_interval, sensor_arch, logic_arch, final_arch,
     mttr_hours, beta_factor,
     plant, unit, equip, response_time, safe_state, reset_req, bypass_req, design_standard,
     lifecycle_phase";

/// 跨图汇总视图（一行 = 一个 SIF）
#[derive(Debug, Serialize, Deserialize, Clone, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SifSummary {
    pub id: i64,
    pub project_id: i64,
    pub project_code: String,
    pub code: String,
    pub name: String,
    pub description: String,
    pub sil_design: String,
    pub sil_verified: String,
    pub demand_mode: String,
    pub pfdavg_target: Option<f64>,
    pub proof_interval: i64,
    /// IEC 61511-2 — 检测元件表决架构
    pub sensor_arch: String,
    /// 逻辑解算器表决架构
    pub logic_arch: String,
    /// 最终元件表决架构
    pub final_arch: String,
    /// 平均修复时间 MTTR（小时）
    pub mttr_hours: f64,
    /// 共因失效因子 β（0~1）
    pub beta_factor: f64,
    /// SRS — 项目/装置
    pub plant: String,
    /// SRS — 工艺单元
    pub unit: String,
    /// SRS — 关联设备
    pub equip: String,
    /// SRS — 响应时间要求
    pub response_time: String,
    /// SRS — 安全状态定义
    pub safe_state: String,
    /// SRS — 复位要求
    pub reset_req: String,
    /// SRS — 旁路管理
    pub bypass_req: String,
    /// SRS — 设计依据标准
    pub design_standard: String,
    /// SIF 生命周期阶段（design/construction/commissioning/operation/closed）
    pub lifecycle_phase: String,
    pub detector_count: i64,
    pub final_count: i64,
    pub logic_count: i64,
    pub aux_count: i64,
    pub diagram_count: i64,
    pub detectors_csv: String, // 去重后的检测位号
    pub finals_csv: String,
    pub diagrams_csv: String, // 涉及图号
    /// 运行时计算：PFDavg（低需求模式，null = 数据不足无法计算）
    #[sqlx(default)]
    pub pfdavg_calculated: Option<f64>,
    /// 运行时计算：PFH（高需求/连续模式，1/h，null = 数据不足）
    #[sqlx(default)]
    pub pfh_calculated: Option<f64>,
    /// 运行时计算：达到的 SIL（NA/A/B/C/D），按 demandMode 取 PFD 或 PFH 判定
    #[sqlx(default)]
    pub sil_achieved: String,
    /// 运行时计算：子系统指标组件（sensor/logic/final）
    #[sqlx(default)]
    pub pfd_components: String,
    /// 运行时计算：silVerified 与 silAchieved 一致性
    /// pending=无失效数据 / verified=一致 / overclaimed=验证值高于可达 /
    /// downgraded=验证值低于可达 / unverified=有数据但 silVerified=NA
    #[sqlx(default)]
    pub sil_verify_status: String,
    /// Route 1H — 检测子系统架构与实际通道数匹配状态（matched/degraded/empty）
    #[sqlx(default)]
    pub sensor_match: String,
    /// Route 1H — 逻辑子系统匹配状态
    #[sqlx(default)]
    pub logic_match: String,
    /// Route 1H — 最终子系统匹配状态
    #[sqlx(default)]
    pub final_match: String,
}

#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SifBasic {
    pub id: i64,
    pub project_id: i64,
    pub code: String,
    pub name: String,
    pub description: String,
    pub sil_design: String,
    pub sil_verified: String,
    pub demand_mode: String,
    pub pfdavg_target: Option<f64>,
    pub proof_interval: i64,
    pub sensor_arch: String,
    pub logic_arch: String,
    pub final_arch: String,
    pub mttr_hours: f64,
    pub beta_factor: f64,
    pub plant: String,
    pub unit: String,
    pub equip: String,
    pub response_time: String,
    pub safe_state: String,
    pub reset_req: String,
    pub bypass_req: String,
    pub design_standard: String,
    pub lifecycle_phase: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SifInput {
    pub project_id: i64,
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_sil")]
    pub sil_design: String,
    #[serde(default = "default_sil")]
    pub sil_verified: String,
    #[serde(default = "default_demand")]
    pub demand_mode: String,
    pub pfdavg_target: Option<f64>,
    #[serde(default = "default_proof")]
    pub proof_interval: i64,
    #[serde(default = "default_arch")]
    pub sensor_arch: String,
    #[serde(default = "default_arch")]
    pub logic_arch: String,
    #[serde(default = "default_arch")]
    pub final_arch: String,
    /// 平均修复时间（小时），默认 8
    #[serde(default = "default_mttr")]
    pub mttr_hours: f64,
    /// 共因失效因子 β（0~1），默认 0.10
    #[serde(default = "default_beta")]
    pub beta_factor: f64,
    /// SRS — 项目/装置
    #[serde(default)]
    pub plant: String,
    /// SRS — 工艺单元
    #[serde(default)]
    pub unit: String,
    /// SRS — 关联设备
    #[serde(default)]
    pub equip: String,
    /// SRS — 响应时间要求
    #[serde(default)]
    pub response_time: String,
    /// SRS — 安全状态定义
    #[serde(default)]
    pub safe_state: String,
    /// SRS — 复位要求
    #[serde(default)]
    pub reset_req: String,
    /// SRS — 旁路管理
    #[serde(default)]
    pub bypass_req: String,
    /// SRS — 设计依据标准
    #[serde(default)]
    pub design_standard: String,
    /// SIF 生命周期阶段（design/construction/commissioning/operation/closed），默认 design
    #[serde(default = "default_lifecycle")]
    pub lifecycle_phase: String,
}

fn default_arch() -> String {
    "1oo1".into()
}

fn default_mttr() -> f64 {
    8.0
}

fn default_beta() -> f64 {
    0.10
}

fn default_lifecycle() -> String {
    "design".into()
}

/// SifInput 的语义默认值（与 serde default 对齐）；
/// 测试构造 SifInput 字面量时可用 `..Default::default()` 兜底新增可选项。
impl Default for SifInput {
    fn default() -> Self {
        SifInput {
            project_id: 0,
            code: String::new(),
            name: String::new(),
            description: String::new(),
            sil_design: default_sil(),
            sil_verified: default_sil(),
            demand_mode: default_demand(),
            pfdavg_target: None,
            proof_interval: default_proof(),
            sensor_arch: default_arch(),
            logic_arch: default_arch(),
            final_arch: default_arch(),
            mttr_hours: default_mttr(),
            beta_factor: default_beta(),
            plant: String::new(),
            unit: String::new(),
            equip: String::new(),
            response_time: String::new(),
            safe_state: String::new(),
            reset_req: String::new(),
            bypass_req: String::new(),
            design_standard: String::new(),
            lifecycle_phase: default_lifecycle(),
        }
    }
}

fn default_sil() -> String {
    "NA".into()
}
fn default_demand() -> String {
    "low".into()
}
fn default_proof() -> i64 {
    12
}

/// 跨图汇总：一次 SQL 拿全部位号 + 图号，减少 N+1；
/// 随后对每条 SIF 运行 PFDavg 计算引擎（IEC 61511-2 §6）。
pub async fn list_sifs_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    project_id: Option<i64>,
) -> AppResult<Vec<SifSummary>> {
    let base = "SELECT s.id, s.project_id, p.code AS project_code, s.code, s.name, s.description,
                      s.sil_design, s.sil_verified, s.demand_mode, s.pfdavg_target,
                      s.proof_interval, s.sensor_arch, s.logic_arch, s.final_arch,
                      s.mttr_hours, s.beta_factor,
                      s.plant, s.unit, s.equip, s.response_time,
                      s.safe_state, s.reset_req, s.bypass_req, s.design_standard,
                      s.lifecycle_phase,
                      COUNT(DISTINCT CASE WHEN si.role='detector' THEN si.instrument_id END) AS detector_count,
                      COUNT(DISTINCT CASE WHEN si.role='final'    THEN si.instrument_id END) AS final_count,
                      COUNT(DISTINCT CASE WHEN si.role='logic'    THEN si.instrument_id END) AS logic_count,
                      COUNT(DISTINCT CASE WHEN si.role='aux'      THEN si.instrument_id END) AS aux_count,
                      COUNT(DISTINCT si.diagram_id) AS diagram_count,
                      GROUP_CONCAT(DISTINCT CASE WHEN si.role='detector' THEN i.tag END) AS detectors_csv,
                      GROUP_CONCAT(DISTINCT CASE WHEN si.role='final'    THEN i.tag END) AS finals_csv,
                      GROUP_CONCAT(DISTINCT d.code)                            AS diagrams_csv
               FROM sif s
               JOIN project p ON p.id = s.project_id AND p.org_id = s.org_id
               LEFT JOIN sif_instrument si ON si.sif_id = s.id
               LEFT JOIN instrument i      ON i.id = si.instrument_id AND i.org_id = s.org_id
               LEFT JOIN diagram d         ON d.id = si.diagram_id AND d.org_id = s.org_id
              WHERE s.org_id = ?";

    let mut rows = if let Some(pid) = project_id {
        sqlx::query_as::<_, SifSummary>(&format!(
            "{base} AND s.project_id = ? GROUP BY s.id ORDER BY s.code"
        ))
        .bind(org_id)
        .bind(pid)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, SifSummary>(&format!("{base} GROUP BY s.id ORDER BY s.code"))
            .bind(org_id)
            .fetch_all(pool)
            .await?
    };

    // 指标引擎：逐条 SIF 计算 PFDavg/PFH，按 demandMode 判定 silAchieved
    for s in rows.iter_mut() {
        // Route 1H — 架构与通道数匹配状态（degraded = 计算引擎退化为串联公式）
        s.sensor_match = arch_channel_status(&s.sensor_arch, s.detector_count);
        s.logic_match = arch_channel_status(&s.logic_arch, s.logic_count);
        s.final_match = arch_channel_status(&s.final_arch, s.final_count);
        let calc = compute_sif_metrics(
            pool, org_id, s.id, s.proof_interval,
            s.mttr_hours, s.beta_factor, &s.demand_mode,
            &s.sensor_arch, &s.logic_arch, &s.final_arch,
        ).await;
        match calc {
            Ok(c) => {
                let has_data = c.pfd.is_some();
                s.pfdavg_calculated = c.pfd;
                s.pfh_calculated = c.pfh;
                s.sil_achieved = c.sil_achieved.clone();
                s.pfd_components = c.components_json;
                s.sil_verify_status =
                    sil_verify_status(&s.sil_verified, &c.sil_achieved, has_data);
            }
            Err(_) => {
                s.pfdavg_calculated = None;
                s.pfh_calculated = None;
                s.sil_achieved = "NA".into();
                s.pfd_components = "{}".into();
                s.sil_verify_status = "pending".into();
            }
        }
    }

    Ok(rows)
}

/// SIF 可靠性指标计算结果（PFDavg + PFH）
pub struct MetricsResult {
    /// 低需求模式 PFDavg（无 λDU/λDD 数据时为 None）
    pub pfd: Option<f64>,
    /// 高需求/连续模式 PFH（1/h，无 λDU 数据时为 None）
    pub pfh: Option<f64>,
    /// 按 demandMode 选择 PFD 或 PFH 判定出的可达 SIL（NA/A/B/C/D）
    pub sil_achieved: String,
    pub components_json: String,
}

/// 仪表失效参数（用于指标计算与 SFF/HFT 校验）
#[derive(Debug, Clone)]
pub struct InstFailure {
    pub role: String,
    pub lambda_du: f64,
    pub lambda_dd: f64,
    pub pt_coverage: f64,
    pub sff: f64,
    pub hft: i64,
    pub equipment_type: String,
    pub tag: String,
}

/// 拉取 SIF 关联仪表的失效参数（指标计算 + SFF/HFT 校验共用）
async fn fetch_sif_instruments(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    sif_id: i64,
) -> AppResult<Vec<InstFailure>> {
    let rows = sqlx::query_as::<_, (String, f64, f64, f64, f64, i64, String, String)>(
        "SELECT si.role, i.lambda_du, i.lambda_dd, i.pt_coverage, i.sff, i.hft, i.equipment_type, i.tag
         FROM sif_instrument si
         JOIN instrument i ON i.id = si.instrument_id AND i.org_id = ?
         WHERE si.sif_id = ?",
    )
    .bind(org_id)
    .bind(sif_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(role, ldu, ldd, ptc, sff, hft, etype, tag)| InstFailure {
            role,
            lambda_du: ldu,
            lambda_dd: ldd,
            pt_coverage: ptc,
            sff,
            hft,
            equipment_type: etype,
            tag,
        })
        .collect())
}

/// 单 SIF 可靠性指标计算（IEC 61511-2 §6 简化公式）
///
/// 同时计算 PFDavg（低需求）与 PFH（高需求/连续），silAchieved 按 demandMode 取其一。
///
/// PTC 折入（§6.2.6）：等效 λDU_eff = λDU × (1 − PTC/2)
/// λDD/MTTR（§6.2.4）：危险已检测失效在修复期暴露，PFD 补 λDD × MTTR
/// β 共因（§6.2.6）：仅作用于 1oo2/2oo3/2oo4 表决架构，
///   PFD 补 β·λDU_eff·TI/2，PFH 补 β·λDU
///
/// TI = proof_interval（月）→ 小时：TI × 30 × 24
pub async fn compute_sif_metrics(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    sif_id: i64,
    proof_interval_months: i64,
    mttr_hours: f64,
    beta: f64,
    demand_mode: &str,
    sensor_arch: &str,
    logic_arch: &str,
    final_arch: &str,
) -> AppResult<MetricsResult> {
    let insts = fetch_sif_instruments(pool, org_id, sif_id).await?;

    let det: Vec<InstFailure> = insts.iter().filter(|i| i.role == "detector").cloned().collect();
    let logic: Vec<InstFailure> = insts.iter().filter(|i| i.role == "logic").cloned().collect();
    let fin: Vec<InstFailure> = insts.iter().filter(|i| i.role == "final").cloned().collect();

    let ti_hours = (proof_interval_months.max(0) as f64) * 30.0 * 24.0;

    let pfd_sensor = subsystem_pfd(&det, sensor_arch, ti_hours, mttr_hours, beta);
    let pfd_logic = subsystem_pfd(&logic, logic_arch, ti_hours, mttr_hours, beta);
    let pfd_final = subsystem_pfd(&fin, final_arch, ti_hours, mttr_hours, beta);
    let pfh_sensor = subsystem_pfh(&det, sensor_arch, ti_hours, mttr_hours, beta);
    let pfh_logic = subsystem_pfh(&logic, logic_arch, ti_hours, mttr_hours, beta);
    let pfh_final = subsystem_pfh(&fin, final_arch, ti_hours, mttr_hours, beta);

    // 三子系统独立 → 串联求和
    let pfd_total = pfd_sensor + pfd_logic + pfd_final;
    let pfh_total = pfh_sensor + pfh_logic + pfh_final;

    // PFD 需要 λDU 或 λDD 数据；PFH 只依赖 λDU
    let any_pfd_data = insts.iter().any(|i| i.lambda_du > 0.0 || i.lambda_dd > 0.0);
    let any_pfh_data = insts.iter().any(|i| i.lambda_du > 0.0);
    let pfd_opt = if any_pfd_data { Some(pfd_total) } else { None };
    let pfh_opt = if any_pfh_data { Some(pfh_total) } else { None };

    let sil = if demand_mode == "high" {
        match pfh_opt {
            Some(p) => sil_from_pfh(p),
            None => "NA".into(),
        }
    } else {
        match pfd_opt {
            Some(p) => sil_from_pfd(p),
            None => "NA".into(),
        }
    };

    let components = serde_json::json!({
        "sensor": { "pfd": pfd_sensor, "pfh": pfh_sensor },
        "logic":  { "pfd": pfd_logic,  "pfh": pfh_logic },
        "final":  { "pfd": pfd_final,  "pfh": pfh_final },
    });

    Ok(MetricsResult {
        pfd: pfd_opt,
        pfh: pfh_opt,
        sil_achieved: sil,
        components_json: components.to_string(),
    })
}

/// 单子系统 PFDavg 计算（PTC 折入 + λDD/MTTR + β 共因）
fn subsystem_pfd(insts: &[InstFailure], arch: &str, ti_hours: f64, mttr: f64, beta: f64) -> f64 {
    if insts.is_empty() {
        return 0.0;
    }
    let n = insts.len();
    // PTC 加权后的等效 λDU；λDD 为已检测失效率
    let eff: Vec<f64> = insts
        .iter()
        .map(|i| i.lambda_du * (1.0 - i.pt_coverage / 2.0))
        .collect();
    let sum_du: f64 = eff.iter().sum();
    let avg_du = sum_du / (n as f64);
    let avg_dd: f64 = insts.iter().map(|i| i.lambda_dd).sum::<f64>() / (n as f64);
    // 已检测失效：检出后立即维修，平均不可用度 λDD × MTTR
    let dd_term = avg_dd * mttr;
    // 共因项（仅冗余表决架构）
    let beta_term = beta * avg_du * ti_hours / 2.0;

    // 架构与通道数匹配才用表决公式，否则退化为串联
    let pfd = match arch {
        "1oo1" if n == 1 => sum_du * ti_hours / 2.0 + avg_dd * mttr,
        "1oo2" if n == 2 => (avg_du * ti_hours).powi(2) / 3.0 + beta_term + dd_term,
        "2oo2" if n == 2 => 2.0 * avg_du * ti_hours + 2.0 * dd_term,
        "2oo3" if n == 3 => (avg_du * ti_hours).powi(2) + beta_term + dd_term,
        "2oo4" if n == 4 => 2.0 * (avg_du * ti_hours).powi(2) + beta_term + dd_term,
        _ => sum_du * ti_hours / 2.0 + avg_dd * mttr * (n as f64),
    };
    pfd.max(0.0)
}

/// 单子系统 PFH 计算（高需求/连续模式，IEC 61511-2 简化公式，1/h）
///
///   1oo1: PFH = λDU
///   1oo2: PFH = 2(1-β)·λDU²·(TI/2 + MTTR) + β·λDU
///   2oo2: PFH = 2·λDU（任一通道危险失效即丧失功能）
///   2oo3: PFH = 6(1-β)·λDU²·(TI/2 + MTTR) + β·λDU
///   2oo4: PFH = 12(1-β)·λDU²·(TI/2 + MTTR) + β·λDU
fn subsystem_pfh(insts: &[InstFailure], arch: &str, ti_hours: f64, mttr: f64, beta: f64) -> f64 {
    if insts.is_empty() {
        return 0.0;
    }
    let n = insts.len();
    let avg_du: f64 = insts.iter().map(|i| i.lambda_du).sum::<f64>() / (n as f64);
    let test_window = ti_hours / 2.0 + mttr;

    let pfh = match arch {
        "1oo1" if n == 1 => avg_du,
        "1oo2" if n == 2 => 2.0 * (1.0 - beta) * avg_du.powi(2) * test_window + beta * avg_du,
        "2oo2" if n == 2 => 2.0 * avg_du,
        "2oo3" if n == 3 => 6.0 * (1.0 - beta) * avg_du.powi(2) * test_window + beta * avg_du,
        "2oo4" if n == 4 => 12.0 * (1.0 - beta) * avg_du.powi(2) * test_window + beta * avg_du,
        _ => avg_du * (n as f64),
    };
    pfh.max(0.0)
}

/// PFDavg → SIL（低需求模式，IEC 61511-1 表 4）
fn sil_from_pfd(pfd: f64) -> String {
    if pfd <= 0.0 {
        return "NA".into();
    }
    if (1e-5..1e-4).contains(&pfd) { "D".into() }       // SIL 4
    else if (1e-4..1e-3).contains(&pfd) { "C".into() }   // SIL 3
    else if (1e-3..1e-2).contains(&pfd) { "B".into() }   // SIL 2
    else if (1e-2..1e-1).contains(&pfd) { "A".into() }   // SIL 1
    else if pfd >= 1e-1 { "NA".into() }                  // 不满足任何 SIL
    else { "D".into() }                                   // < 1e-5 优于 SIL 4
}

/// PFH → SIL（高需求/连续模式，IEC 61511-1 表 5，单位 1/h）
fn sil_from_pfh(pfh: f64) -> String {
    if pfh <= 0.0 {
        return "NA".into();
    }
    if (1e-9..1e-8).contains(&pfh) { "D".into() }
    else if (1e-8..1e-7).contains(&pfh) { "C".into() }
    else if (1e-7..1e-6).contains(&pfh) { "B".into() }
    else if (1e-6..1e-5).contains(&pfh) { "A".into() }
    else if pfh >= 1e-5 { "NA".into() }
    else { "D".into() }
}

/// silVerified（人工确认）与 silAchieved（计算）的一致性状态
///
/// 单一事实源原则：silAchieved 由失效数据自动计算，silVerified 仅为人工确认值。
///   pending     = 无失效数据，无法比对
///   unverified  = 有可达 SIL 但 silVerified=NA（尚未确认）
///   verified    = 两者一致
///   overclaimed = 验证值高于可达 SIL（保存时拒绝，历史数据可能残留）
///   downgraded  = 验证值低于可达 SIL（按更低 SIL 运行）
pub fn sil_verify_status(sil_verified: &str, sil_achieved: &str, has_data: bool) -> String {
    if !has_data {
        return "pending".into();
    }
    match sil_idx(sil_verified) {
        None => "unverified".into(),
        Some(v) => match sil_idx(sil_achieved) {
            Some(a) if v == a => "verified".into(),
            Some(a) if v > a => "overclaimed".into(),
            Some(_) => "downgraded".into(),
            None => "overclaimed".into(),
        },
    }
}

/// 校验表决架构取值
fn validate_arch(arch: &str) -> AppResult<()> {
    if !["1oo1", "1oo2", "2oo2", "2oo3", "2oo4"].contains(&arch) {
        return Err(AppError::Validation(format!(
            "架构 '{arch}' 非法，仅支持 1oo1/1oo2/2oo2/2oo3/2oo4"
        )));
    }
    Ok(())
}

/// 校验 SIF 可靠性参数（需求模式 / MTTR / β）
fn validate_reliability_params(input: &SifInput) -> AppResult<()> {
    if !["low", "high"].contains(&input.demand_mode.as_str()) {
        return Err(AppError::Validation(
            "demand_mode 只能是 low（低需求）/ high（高需求或连续）".into(),
        ));
    }
    if input.mttr_hours < 0.0 {
        return Err(AppError::Validation("MTTR 不能为负数".into()));
    }
    if !(0.0..=1.0).contains(&input.beta_factor) {
        return Err(AppError::Validation("β 共因因子必须在 0~1 之间".into()));
    }
    Ok(())
}

/// IEC 61508-2 Route 1H 架构约束表（SFF 分档 × HFT → 最高可达 SIL）
///
/// 行: SFF 分档 [0]=<60% [1]=60~<90 [2]=90~<99 [3]=>=99
/// 列: HFT 0 / 1 / 2；值: 1..4=最高可达 SIL，0=该组合不允许声明任何 SIL
///
/// 表 2 — Type A（故障模式明确的简单元件，如机械继电器）
const MAX_SIL_TYPE_A: [[u8; 3]; 4] = [
    [1, 2, 3], // SFF < 60%
    [2, 3, 4], // 60% ≤ SFF < 90%
    [3, 4, 4], // 90% ≤ SFF < 99%
    [3, 4, 4], // SFF ≥ 99%
];
/// 表 3 — Type B（复杂元件/含软件，如 PLC、智能变送器）
const MAX_SIL_TYPE_B: [[u8; 3]; 4] = [
    [0, 1, 2], // SFF < 60%（HFT=0 不允许）
    [1, 2, 3], // 60% ≤ SFF < 90%
    [2, 3, 4], // 90% ≤ SFF < 99%
    [3, 4, 4], // SFF ≥ 99%
];

/// 架构 → 有效 HFT（IEC 61511-2）
fn hft_from_arch(arch: &str) -> i64 {
    match arch {
        "1oo1" | "2oo2" => 0,
        "1oo2" | "2oo3" => 1,
        "2oo4" => 2,
        _ => 0,
    }
}

/// 架构 → 表决要求的通道数
fn arch_required_channels(arch: &str) -> i64 {
    match arch {
        "1oo1" => 1,
        "1oo2" | "2oo2" => 2,
        "2oo3" => 3,
        "2oo4" => 4,
        _ => 1,
    }
}

/// Route 1H — 架构与实际通道数匹配状态：
///   matched  = 通道数与表决架构一致（PFD/PFH 用对应表决公式）
///   degraded = 通道数不足或超配（计算引擎静默退化为串联公式，结果失真）
///   empty    = 子系统无任何仪表
fn arch_channel_status(arch: &str, actual: i64) -> String {
    if actual <= 0 {
        return "empty".into();
    }
    if actual == arch_required_channels(arch) {
        "matched".into()
    } else {
        "degraded".into()
    }
}

/// SIL 字母 → 序号 (A=1, B=2, C=3, D=4)
fn sil_idx(sil: &str) -> Option<usize> {
    match sil {
        "A" => Some(1),
        "B" => Some(2),
        "C" => Some(3),
        "D" => Some(4),
        _ => None,
    }
}

/// SFF 落入 IEC 61508-2 的哪个分档
fn sff_band(sff: f64) -> usize {
    if sff < 0.60 {
        0
    } else if sff < 0.90 {
        1
    } else if sff < 0.99 {
        2
    } else {
        3
    }
}

/// 设备类型 + SFF + HFT → 最高可达 SIL；None 表示该组合不允许声明任何 SIL
fn max_sil_achievable(equipment_type: &str, sff: f64, hft: i64) -> Option<u8> {
    let table = if equipment_type == "type_a" {
        &MAX_SIL_TYPE_A
    } else {
        &MAX_SIL_TYPE_B
    };
    let v = table[sff_band(sff)][hft.clamp(0, 2) as usize];
    if v == 0 { None } else { Some(v) }
}

/// 校验单台仪表能否支撑目标 SIL（按 Type A/B 分档表）
fn ensure_inst_supports_sil(
    tag: &str,
    equipment_type: &str,
    sff: f64,
    arch: &str,
    sil_design: &str,
) -> AppResult<()> {
    let Some(sil) = sil_idx(sil_design) else {
        return Ok(()); // SIL=NA 不约束
    };
    let hft = hft_from_arch(arch);
    match max_sil_achievable(equipment_type, sff, hft) {
        Some(max_sil) if max_sil as usize >= sil => Ok(()),
        Some(max_sil) => Err(AppError::Validation(format!(
            "仪表 {tag}（{}，SFF={:.3}，HFT={hft}）最高只能声明 SIL {}，\
             低于目标 SIL {sil_design}（IEC 61508-2 表 {tbl}），\
             请提升 SFF、改用冗余架构或确认设备类型",
            if equipment_type == "type_a" { "Type A" } else { "Type B" },
            sff,
            max_sil,
            tbl = if equipment_type == "type_a" { 2 } else { 3 },
        ))),
        None => Err(AppError::Validation(format!(
            "仪表 {tag}（Type B，SFF={:.3}，HFT={hft}）按 IEC 61508-2 表 3 不允许声明任何 SIL，\
             请提升 SFF 至 60% 以上或改用冗余架构",
            sff
        ))),
    }
}

/// SFF/HFT 可行性校验（IEC 61508-2 表 2 / 表 3，按设备 Type A/B 分档）
///
/// 对每个子系统（检测/逻辑/最终），根据架构确定有效 HFT，
/// 逐台仪表按其设备类型查分档表，校验最高可达 SIL ≥ 目标 SIL。
pub async fn check_sff_hft_feasibility(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    sif_id: i64,
    sil_design: &str,
    sensor_arch: &str,
    logic_arch: &str,
    final_arch: &str,
) -> AppResult<()> {
    if sil_idx(sil_design).is_none() {
        return Ok(()); // SIL=NA 不约束
    }

    let insts = fetch_sif_instruments(pool, org_id, sif_id).await?;
    for inst in &insts {
        if inst.role == "aux" {
            continue; // AUX（旁路关联等）不参与 SIF 表决
        }
        let arch = match inst.role.as_str() {
            "detector" => sensor_arch,
            "logic" => logic_arch,
            "final" => final_arch,
            _ => sensor_arch,
        };
        ensure_inst_supports_sil(&inst.tag, &inst.equipment_type, inst.sff, arch, sil_design)?;
    }
    Ok(())
}

/// 第 6 项防线：仪表 SFF / 设备类型编辑前，对其所有已关联 SIF 做再校验。
///
/// 用"拟修改后"的 SFF/设备类型，逐 SIF 按子系统架构查 Type A/B 分档表；
/// 任一 SIF 不再满足目标 SIL → 拒绝修改，避免已关联 SIF 静默违规。
pub async fn check_instrument_sff_change(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    instrument_id: i64,
    new_sff: f64,
    new_equipment_type: &str,
) -> AppResult<()> {
    let rows = sqlx::query_as::<_, (i64, String, String, String, String, String, String)>(
        "SELECT si.sif_id, si.role, s.code, s.sil_design,
                s.sensor_arch, s.logic_arch, s.final_arch
         FROM sif_instrument si
         JOIN sif s ON s.id = si.sif_id AND s.org_id = ?
         WHERE si.instrument_id = ?",
    )
    .bind(org_id)
    .bind(instrument_id)
    .fetch_all(pool)
    .await?;

    for (_sif_id, role, sif_code, sil_design, s_arch, l_arch, f_arch) in rows {
        if role == "aux" {
            continue;
        }
        let Some(sil) = sil_idx(&sil_design) else {
            continue;
        };
        let arch = match role.as_str() {
            "detector" => &s_arch,
            "logic" => &l_arch,
            "final" => &f_arch,
            _ => &s_arch,
        };
        let hft = hft_from_arch(arch);
        let type_label = if new_equipment_type == "type_a" { "Type A" } else { "Type B" };
        match max_sil_achievable(new_equipment_type, new_sff, hft) {
            Some(max_sil) if max_sil as usize >= sil => {}
            Some(max_sil) => {
                return Err(AppError::Validation(format!(
                    "修改后（{type_label}，SFF={new_sff:.3}，{arch} HFT={hft}）最高只能声明 SIL {max_sil}，\
                     已关联的 SIF {sif_code} 目标为 SIL {sil_design}（IEC 61508-2 表 {tbl}）；\
                     请先解除关联或走变更管理",
                    tbl = if new_equipment_type == "type_a" { 2 } else { 3 },
                )));
            }
            None => {
                return Err(AppError::Validation(format!(
                    "修改后（Type B，SFF={new_sff:.3}，{arch} HFT={hft}）按 IEC 61508-2 表 3 不允许声明任何 SIL，\
                     已关联的 SIF {sif_code} 目标为 SIL {sil_design}；请先解除关联或走变更管理"
                )));
            }
        }
    }
    Ok(())
}


/// 双源打通（IEC 61511-1 §10 验证）：silVerified 为人工确认值，不得高于
/// 由失效数据计算出的 silAchieved。无失效数据时不约束（设计阶段）。
async fn ensure_verified_not_overclaimed(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    sif_id: i64,
    input: &SifInput,
) -> AppResult<()> {
    let Some(verified) = sil_idx(&input.sil_verified) else {
        return Ok(()); // silVerified=NA 不约束
    };
    let m = compute_sif_metrics(
        pool, org_id, sif_id, input.proof_interval,
        input.mttr_hours, input.beta_factor, &input.demand_mode,
        &input.sensor_arch, &input.logic_arch, &input.final_arch,
    )
    .await?;
    let has_data = if input.demand_mode == "high" {
        m.pfh.is_some()
    } else {
        m.pfd.is_some()
    };
    if !has_data {
        return Ok(()); // 设计阶段，尚无仪表失效数据
    }
    if let Some(achieved) = sil_idx(&m.sil_achieved) {
        if verified > achieved {
            let metric = if input.demand_mode == "high" {
                format!("PFH={:.2e}/h", m.pfh.unwrap_or(0.0))
            } else {
                format!("PFDavg={:.2e}", m.pfd.unwrap_or(0.0))
            };
            return Err(AppError::Validation(format!(
                "验证 SIL {} 高于按当前失效数据计算的可达 SIL {}（{}）；\
                 silVerified 不能高于 silAchieved，请补全/修正仪表失效参数或调整验证值",
                input.sil_verified, m.sil_achieved, metric
            )));
        }
    } else {
        return Err(AppError::Validation(format!(
            "验证 SIL {} 但当前设计按 {} 计算不满足任何 SIL 等级",
            input.sil_verified,
            if input.demand_mode == "high" { "PFH" } else { "PFDavg" }
        )));
    }
    Ok(())
}

pub async fn get_sif_inner(pool: &sqlx::SqlitePool, org_id: i64, id: i64) -> AppResult<SifBasic> {
    sqlx::query_as::<_, SifBasic>(&format!(
        "SELECT {SIF_COLS} FROM sif WHERE org_id = ? AND id = ?"
    ))
    .bind(org_id)
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("sif id={id}")))
}

pub async fn create_sif_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    input: &SifInput,
    actor: &str,
) -> AppResult<SifBasic> {
    if input.code.trim().is_empty() || input.name.trim().is_empty() {
        return Err(AppError::Validation("code and name are required".into()));
    }
    validate_arch(&input.sensor_arch)?;
    validate_arch(&input.logic_arch)?;
    validate_arch(&input.final_arch)?;
    validate_reliability_params(input)?;
    let proj_exists: Option<i64> =
        sqlx::query_scalar("SELECT id FROM project WHERE org_id = ? AND id = ?")
            .bind(org_id)
            .bind(input.project_id)
            .fetch_optional(pool)
            .await?;
    if proj_exists.is_none() {
        return Err(AppError::Validation(format!(
            "project_id {} not found in current organization",
            input.project_id
        )));
    }

    let res = sqlx::query(
        "INSERT INTO sif (org_id, project_id, code, name, description, sil_design, sil_verified,
                          demand_mode, pfdavg_target, proof_interval,
                          sensor_arch, logic_arch, final_arch, mttr_hours, beta_factor,
                          plant, unit, equip, response_time,
                          safe_state, reset_req, bypass_req, design_standard,
                          lifecycle_phase)
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(org_id)
    .bind(input.project_id)
    .bind(&input.code)
    .bind(&input.name)
    .bind(&input.description)
    .bind(&input.sil_design)
    .bind(&input.sil_verified)
    .bind(&input.demand_mode)
    .bind(input.pfdavg_target)
    .bind(input.proof_interval)
    .bind(&input.sensor_arch)
    .bind(&input.logic_arch)
    .bind(&input.final_arch)
    .bind(input.mttr_hours)
    .bind(input.beta_factor)
    .bind(&input.plant)
    .bind(&input.unit)
    .bind(&input.equip)
    .bind(&input.response_time)
    .bind(&input.safe_state)
    .bind(&input.reset_req)
    .bind(&input.bypass_req)
    .bind(&input.design_standard)
    .bind(&input.lifecycle_phase)
    .execute(pool)
    .await;

    let id = match res {
        Ok(r) => r.last_insert_rowid(),
        Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
            return Err(AppError::Conflict(format!(
                "sif code '{}' already exists in project {}",
                input.code, input.project_id
            )));
        }
        Err(e) => return Err(AppError::from(e)),
    };

    let created = get_sif_inner(pool, org_id, id).await?;

    let payload = entity_audit_payload(None, Some(snapshot(&created)), vec!["*".to_string()]);
    let _ = write_audit(pool, org_id, actor, "sif_create", "sif", Some(id), payload).await;

    Ok(created)
}

pub async fn update_sif_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    id: i64,
    input: &SifInput,
    actor: &str,
) -> AppResult<SifBasic> {
    let before = get_sif_inner(pool, org_id, id).await?;

    validate_arch(&input.sensor_arch)?;
    validate_arch(&input.logic_arch)?;
    validate_arch(&input.final_arch)?;
    validate_reliability_params(input)?;

    // 目标项目必须同 org（防止把 SIF 挪到别组织项目）
    let proj_exists: Option<i64> =
        sqlx::query_scalar("SELECT id FROM project WHERE org_id = ? AND id = ?")
            .bind(org_id)
            .bind(input.project_id)
            .fetch_optional(pool)
            .await?;
    if proj_exists.is_none() {
        return Err(AppError::Validation(format!(
            "project_id {} not found in current organization",
            input.project_id
        )));
    }

    // 双源打通：UPDATE 前先按拟修改参数计算指标，
    // silVerified 高于 silAchieved 时拒绝落库（避免产生超标确认值）
    ensure_verified_not_overclaimed(pool, org_id, id, input).await?;

    let n = sqlx::query(
        "UPDATE sif SET
            project_id=?, code=?, name=?, description=?, sil_design=?, sil_verified=?,
            demand_mode=?, pfdavg_target=?, proof_interval=?,
            sensor_arch=?, logic_arch=?, final_arch=?, mttr_hours=?, beta_factor=?,
            plant=?, unit=?, equip=?, response_time=?,
            safe_state=?, reset_req=?, bypass_req=?, design_standard=?,
            lifecycle_phase=?,
            updated_at=datetime('now')
         WHERE org_id = ? AND id = ?",
    )
    .bind(input.project_id)
    .bind(&input.code)
    .bind(&input.name)
    .bind(&input.description)
    .bind(&input.sil_design)
    .bind(&input.sil_verified)
    .bind(&input.demand_mode)
    .bind(input.pfdavg_target)
    .bind(input.proof_interval)
    .bind(&input.sensor_arch)
    .bind(&input.logic_arch)
    .bind(&input.final_arch)
    .bind(input.mttr_hours)
    .bind(input.beta_factor)
    .bind(&input.plant)
    .bind(&input.unit)
    .bind(&input.equip)
    .bind(&input.response_time)
    .bind(&input.safe_state)
    .bind(&input.reset_req)
    .bind(&input.bypass_req)
    .bind(&input.design_standard)
    .bind(&input.lifecycle_phase)
    .bind(org_id)
    .bind(id)
    .execute(pool)
    .await?
    .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound(format!("sif id={id}")));
    }

    let after = get_sif_inner(pool, org_id, id).await?;

    // IEC 61511-2 SFF/HFT 可行性校验：SIL/架构变更后现有仪表必须仍满足表 3
    check_sff_hft_feasibility(
        pool, org_id, id, &input.sil_design,
        &input.sensor_arch, &input.logic_arch, &input.final_arch,
    )
    .await?;

    let fields_changed = compute_field_diff(&snapshot(&before), &snapshot(&after));
    let payload = entity_audit_payload(
        Some(snapshot(&before)),
        Some(snapshot(&after)),
        fields_changed,
    );
    let _ = write_audit(pool, org_id, actor, "sif_update", "sif", Some(id), payload).await;

    Ok(after)
}

pub async fn delete_sif_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    id: i64,
    actor: &str,
) -> AppResult<u64> {
    let before = get_sif_inner(pool, org_id, id).await?;

    // RESTRICT 守卫：若有联锁图引用此 SIF（diagram.sif_id），禁止删除。
    // schema 是 ON DELETE SET NULL，会破坏 migration 007 建立的"一图一 SIF"不变量。
    // 用户必须先删图（若未来提供 delete_diagram）或删整个项目（CASCADE）。
    let diag_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM diagram WHERE sif_id = ? AND org_id = ?",
    )
    .bind(id)
    .bind(org_id)
    .fetch_one(pool)
    .await?;
    if diag_count > 0 {
        return Err(AppError::Validation(
            "请先删除关联的联锁图再删除 SIF".into(),
        ));
    }

    let n = sqlx::query("DELETE FROM sif WHERE org_id = ? AND id = ?")
        .bind(org_id)
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound(format!("sif id={id}")));
    }

    let payload = entity_audit_payload(Some(snapshot(&before)), None, vec!["*".to_string()]);
    let _ = write_audit(pool, org_id, actor, "sif_delete", "sif", Some(id), payload).await;

    Ok(n)
}

#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SifLink {
    pub id: i64,
    pub sif_id: i64,
    pub instrument_id: i64,
    pub role: String,
    pub port_index: i64,
    pub diagram_id: Option<i64>,
    pub note: String,
    pub tag: String,
    pub kind: String,
    pub service: String,
    pub sil_target: String,
}

pub async fn list_sif_links_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    sif_id: i64,
) -> AppResult<Vec<SifLink>> {
    let rows = sqlx::query_as::<_, SifLink>(
        "SELECT sl.id, sl.sif_id, sl.instrument_id, sl.role, sl.port_index,
                sl.diagram_id, sl.note,
                i.tag, i.kind, i.service, i.sil_target
         FROM sif_instrument sl
         JOIN sif s ON s.id = sl.sif_id AND s.org_id = ?
         JOIN instrument i ON i.id = sl.instrument_id AND i.org_id = ?
         WHERE sl.sif_id = ?
         ORDER BY sl.role, sl.port_index, i.tag",
    )
    .bind(org_id)
    .bind(org_id)
    .bind(sif_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[allow(clippy::too_many_arguments)]
pub async fn link_instrument_to_sif_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    sif_id: i64,
    instrument_id: i64,
    role: String,
    port_index: i64,
    diagram_id: Option<i64>,
    note: String,
    actor: &str,
) -> AppResult<i64> {
    if !["detector", "final", "logic", "aux"].contains(&role.as_str()) {
        return Err(AppError::Validation(format!("role '{role}' invalid")));
    }
    // 三方归属校验（均须同 org；不存在统一 404）
    let sif_exists: Option<i64> =
        sqlx::query_scalar("SELECT id FROM sif WHERE org_id = ? AND id = ?")
            .bind(org_id)
            .bind(sif_id)
            .fetch_optional(pool)
            .await?;
    if sif_exists.is_none() {
        return Err(AppError::NotFound(format!("sif id={sif_id}")));
    }
    let inst_exists: Option<i64> =
        sqlx::query_scalar("SELECT id FROM instrument WHERE org_id = ? AND id = ?")
            .bind(org_id)
            .bind(instrument_id)
            .fetch_optional(pool)
            .await?;
    if inst_exists.is_none() {
        return Err(AppError::NotFound(format!("instrument id={instrument_id}")));
    }

    // IEC 61511-2 Route 1H 架构约束预校验（INSERT 前，避免产生无效关联）：
    // 取 SIF 的 SIL+架构 与 仪表的 SFF/设备类型，按表 2（Type A）/表 3（Type B）校验
    if role != "aux" {
        let (sil_design, s_arch, l_arch, f_arch): (String, String, String, String) = sqlx::query_as(
            "SELECT sil_design, sensor_arch, logic_arch, final_arch FROM sif WHERE org_id = ? AND id = ?",
        )
        .bind(org_id)
        .bind(sif_id)
        .fetch_one(pool)
        .await?;
        if sil_idx(&sil_design).is_some() {
            let arch_for_role = match role.as_str() {
                "detector" => &s_arch,
                "logic" => &l_arch,
                "final" => &f_arch,
                _ => &s_arch,
            };
            let (inst_sff, inst_etype, inst_tag): (f64, String, String) = sqlx::query_as(
                "SELECT sff, equipment_type, tag FROM instrument WHERE org_id = ? AND id = ?",
            )
            .bind(org_id)
            .bind(instrument_id)
            .fetch_one(pool)
            .await?;
            ensure_inst_supports_sil(
                &inst_tag,
                &inst_etype,
                inst_sff,
                arch_for_role,
                &sil_design,
            )?;
        }
    }

    if let Some(did) = diagram_id {
        let diag_exists: Option<i64> =
            sqlx::query_scalar("SELECT id FROM diagram WHERE org_id = ? AND id = ?")
                .bind(org_id)
                .bind(did)
                .fetch_optional(pool)
                .await?;
        if diag_exists.is_none() {
            return Err(AppError::NotFound(format!("diagram id={did}")));
        }
    }

    let res = sqlx::query(
        "INSERT INTO sif_instrument (sif_id, instrument_id, role, port_index, diagram_id, note)
         VALUES (?,?,?,?,?,?)",
    )
    .bind(sif_id)
    .bind(instrument_id)
    .bind(&role)
    .bind(port_index)
    .bind(diagram_id)
    .bind(&note)
    .execute(pool)
    .await;
    let id = match res {
        Ok(r) => r.last_insert_rowid(),
        Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
            return Err(AppError::Conflict(format!(
                "instrument {instrument_id} already linked to sif {sif_id} role={role}"
            )));
        }
        Err(e) => return Err(AppError::from(e)),
    };

    // M2.4 审计：sif_link payload 含 description 自然语言
    let instrument_tag: Option<String> =
        sqlx::query_scalar("SELECT tag FROM instrument WHERE org_id = ? AND id = ?")
            .bind(org_id)
            .bind(instrument_id)
            .fetch_optional(pool)
            .await?;
    let diagram_code: Option<String> = if let Some(did) = diagram_id {
        sqlx::query_scalar("SELECT code FROM diagram WHERE org_id = ? AND id = ?")
            .bind(org_id)
            .bind(did)
            .fetch_optional(pool)
            .await?
    } else {
        None
    };
    let instrument_tag_str = instrument_tag
        .clone()
        .unwrap_or_else(|| format!("inst#{instrument_id}"));
    let desc = format!(
        "关联 {} ({}) 作为 sif_id={} 的 {} · 端口 {} · 图 {}",
        instrument_tag_str,
        role,
        sif_id,
        role_cn(&role),
        port_index,
        diagram_code.as_deref().unwrap_or("（未挂图）"),
    );
    let mut payload = entity_audit_payload(
        None,
        Some(json!({
            "sif_id": sif_id,
            "instrument_id": instrument_id,
            "role": role,
            "port_index": port_index,
            "diagram_id": diagram_id,
            "note": note,
            "instrument_tag": instrument_tag,
            "diagram_code": diagram_code,
        })),
        vec!["link".to_string()],
    );
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("description".into(), serde_json::Value::String(desc));
    }
    let _ = write_audit(
        pool,
        org_id,
        actor,
        "sif_link",
        "sif",
        Some(sif_id),
        payload,
    )
    .await;

    Ok(id)
}

fn role_cn(r: &str) -> String {
    match r {
        "detector" => "检测".into(),
        "final" => "最终".into(),
        "logic" => "逻辑".into(),
        "aux" => "旁路".into(),
        _ => r.to_string(),
    }
}

pub async fn unlink_instrument_from_sif_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    link_id: i64,
    actor: &str,
) -> AppResult<u64> {
    // 先查要删的 link 详情（用于审计 payload），JOIN sif 限定 org
    #[derive(sqlx::FromRow)]
    struct LinkRow {
        sif_id: i64,
        instrument_id: i64,
        role: String,
        port_index: i64,
        diagram_id: Option<i64>,
        instrument_tag: Option<String>,
        diagram_code: Option<String>,
    }
    let row: Option<LinkRow> = sqlx::query_as(
        "SELECT sl.sif_id, sl.instrument_id, sl.role, sl.port_index, sl.diagram_id,
                i.tag AS instrument_tag, d.code AS diagram_code
         FROM sif_instrument sl
         JOIN sif s ON s.id = sl.sif_id AND s.org_id = ?
         JOIN instrument i ON i.id = sl.instrument_id AND i.org_id = ?
         LEFT JOIN diagram d ON d.id = sl.diagram_id AND d.org_id = ?
         WHERE sl.id = ?",
    )
    .bind(org_id)
    .bind(org_id)
    .bind(org_id)
    .bind(link_id)
    .fetch_optional(pool)
    .await?;

    // CASCADE 删除仍以 sif 的 org 归属兜底
    let n = sqlx::query(
        "DELETE FROM sif_instrument
         WHERE id = ?
           AND EXISTS (SELECT 1 FROM sif WHERE id = sif_instrument.sif_id AND org_id = ?)",
    )
    .bind(link_id)
    .bind(org_id)
    .execute(pool)
    .await?
    .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound(format!("link id={link_id}")));
    }

    if let Some(r) = row {
        let instrument_tag_str = r
            .instrument_tag
            .clone()
            .unwrap_or_else(|| format!("inst#{}", r.instrument_id));
        let diagram_code_clone = r.diagram_code.clone();
        let sif_id = r.sif_id;
        let role = r.role.clone();
        let port_index = r.port_index;
        let instrument_id = r.instrument_id;
        let diagram_id = r.diagram_id;
        let desc = format!(
            "解除 {} ({}) 与 sif_id={} 的关联（端口 {}）",
            instrument_tag_str, role, sif_id, port_index,
        );
        let mut payload = entity_audit_payload(
            Some(json!({
                "sif_id": sif_id,
                "instrument_id": instrument_id,
                "role": role,
                "port_index": port_index,
                "diagram_id": diagram_id,
                "instrument_tag": instrument_tag_str,
                "diagram_code": diagram_code_clone,
            })),
            None,
            vec!["unlink".to_string()],
        );
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("description".into(), serde_json::Value::String(desc));
        }
        let _ = write_audit(
            pool,
            org_id,
            actor,
            "sif_unlink",
            "sif",
            Some(sif_id),
            payload,
        )
        .await;
    }

    Ok(n)
}

// ===========================================================================
// M2.4 — SIF 修改历史（sif 自身 create/update/delete + 每次 link/unlink）
// ===========================================================================
pub async fn list_sif_history_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    sif_id: i64,
    limit: Option<i64>,
) -> AppResult<Vec<crate::commands::audit::AuditEntry>> {
    let lim = limit.map(|n| n.clamp(1, 1000)).or(Some(200));
    crate::commands::audit::list_history_for_target_inner(pool, org_id, "sif", sif_id, lim).await
}
