//! M2.8 集成测试 —— 联锁逻辑图（diagram）CRUD（B 阶段：org 作用域 + 乐观锁）
//!
//! 单独一个 test target（与 integration_test.rs 并行跑，互不干扰）。
//!
//! 覆盖：
//!   - save_diagram_data_inner 写库（编辑器 → DB 主路径），version CAS 成功后 +1
//!   - save_diagram_data_inner 幂等（连续两次按最新版本写后 read 拿最新）
//!   - 乐观锁：expected_version 过期 → 409 Conflict
//!   - get_diagram_inner 取单图 + data 字段完整
//!   - get_diagram_inner 404（NotFound 错误）
//!   - create_diagram_inner 必填校验 + UNIQUE 冲突（org_id+project_id+code）
//!   - ensure_default_diagram_inner 首次进入空白项目自动建占位图
//!     （并守住契约：默认 data 不得含 blocks 数组，否则编辑器会清成空白）
//!   - ensure_default_diagram_inner 第二次进入同一项目返已有图（不重复建）
//!   - 快照往返：编辑器快照原样落库 + 往返后仍是有效快照（防「打开即空白」）
//!   - list_diagrams_inner 按 org+project_id 过滤 + 全量
//!   - 删除 project 后 diagram CASCADE 清掉

use sif_studio_lib::commands::diagrams::{
    create_diagram_inner, ensure_default_diagram_inner, get_diagram_inner, list_diagrams_inner,
    save_diagram_data_inner, DiagramInput,
};
use sif_studio_lib::commands::projects::{
    create_project_inner, delete_project_inner, ProjectInput,
};
use sif_studio_lib::db::open_in_memory;
use sif_studio_lib::AppError;
use sqlx::{Pool, Sqlite};

const ORG: i64 = 1;
const ACTOR: &str = "tester";

// ===========================================================================
// helpers
// ===========================================================================

async fn setup() -> (Pool<Sqlite>, i64) {
    let pool = open_in_memory().await.expect("open");
    let p = create_project_inner(
        &pool,
        ORG,
        &ProjectInput {
            code: "PRJ-D".into(),
            name: "测试项目".into(),
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

fn diag_input(pid: i64, code: &str, name: &str, data: &str) -> DiagramInput {
    DiagramInput {
        project_id: pid,
        code: code.into(),
        name: name.into(),
        sheet_size: "A1".into(),
        revision: "A0".into(),
        data: data.into(),
        sif_id: None,
    }
}

// ===========================================================================
// save_diagram_data_inner（乐观锁）
// ===========================================================================

#[tokio::test]
async fn save_diagram_data_writes_and_reads_back() {
    let (pool, pid) = setup().await;
    let d = create_diagram_inner(&pool, ORG, &diag_input(pid, "DGM-1", "测试图", "{}"))
        .await
        .unwrap();

    // 新图 version=0；按 0 提交，返回的新版本应为 1
    let saved = save_diagram_data_inner(&pool, ORG, d.id, 0, r#"{"blocks":[1,2,3]}"#)
        .await
        .expect("save");
    assert_eq!(saved.id, d.id);
    assert_eq!(saved.version, 1);

    let after = get_diagram_inner(&pool, ORG, d.id).await.unwrap();
    assert_eq!(after.data, r#"{"blocks":[1,2,3]}"#);
    assert_eq!(after.version, 1);
    assert!(!after.updated_at.is_empty());
}

#[tokio::test]
async fn save_diagram_data_overwrites_previous() {
    let (pool, pid) = setup().await;
    let d = create_diagram_inner(&pool, ORG, &diag_input(pid, "DGM-2", "覆盖图", "{}"))
        .await
        .unwrap();

    let v1 = save_diagram_data_inner(&pool, ORG, d.id, 0, r#"{"v":1}"#)
        .await
        .unwrap()
        .version;
    assert_eq!(v1, 1);
    // 第二次必须带上最新版本号
    let v2 = save_diagram_data_inner(&pool, ORG, d.id, v1, r#"{"v":2}"#)
        .await
        .unwrap()
        .version;
    assert_eq!(v2, 2);
    let after = get_diagram_inner(&pool, ORG, d.id).await.unwrap();
    assert_eq!(after.data, r#"{"v":2}"#);
    assert_eq!(after.version, 2);
}

#[tokio::test]
async fn save_diagram_data_stale_version_conflicts() {
    let (pool, pid) = setup().await;
    let d = create_diagram_inner(&pool, ORG, &diag_input(pid, "DGM-C", "冲突图", "{}"))
        .await
        .unwrap();

    // 第一笔：0 → 1
    save_diagram_data_inner(&pool, ORG, d.id, 0, r#"{"v":1}"#)
        .await
        .unwrap();
    // 另一会话仍拿旧版本 0 提交 → 409，数据不得被覆盖
    let err = save_diagram_data_inner(&pool, ORG, d.id, 0, r#"{"v":"stale"}"#)
        .await
        .expect_err("过期版本应冲突");
    assert!(
        matches!(err, AppError::Conflict(_)),
        "应返回 Conflict，实际: {err:?}"
    );
    let after = get_diagram_inner(&pool, ORG, d.id).await.unwrap();
    assert_eq!(after.data, r#"{"v":1}"#, "409 时数据不得被改写");
    assert_eq!(after.version, 1);
}

#[tokio::test]
async fn save_diagram_data_missing_diagram_returns_not_found() {
    let (pool, _) = setup().await;
    // 不存在的 diagram id（即便版本给 0）→ 404
    let r = save_diagram_data_inner(&pool, ORG, 99999, 0, r#"{"x":1}"#).await;
    assert!(r.is_err(), "写不存在的图应报错：{r:?}");
    assert!(matches!(r, Err(AppError::NotFound(_))));
}

// ===========================================================================
// get_diagram_inner
// ===========================================================================

#[tokio::test]
async fn get_diagram_returns_full_row() {
    let (pool, pid) = setup().await;
    let d = create_diagram_inner(
        &pool,
        ORG,
        &diag_input(
            pid,
            "DGM-3",
            "第一图",
            r#"{"layout":"SIF-201","drawings":[]}"#,
        ),
    )
    .await
    .unwrap();

    let got = get_diagram_inner(&pool, ORG, d.id).await.unwrap();
    assert_eq!(got.id, d.id);
    assert_eq!(got.code, "DGM-3");
    assert_eq!(got.name, "第一图");
    assert_eq!(got.data, r#"{"layout":"SIF-201","drawings":[]}"#);
    assert_eq!(got.version, 0);
    assert!(!got.created_at.is_empty());
    assert!(!got.updated_at.is_empty());
}

#[tokio::test]
async fn get_diagram_missing_returns_not_found() {
    let (pool, _) = setup().await;
    let r = get_diagram_inner(&pool, ORG, 99999).await;
    assert!(r.is_err(), "取不存在的图应报错：{r:?}");
    assert!(matches!(r, Err(AppError::NotFound(_))));
}

// ===========================================================================
// create_diagram_inner
// ===========================================================================

#[tokio::test]
async fn create_diagram_validation_rejects_blank_code() {
    let (pool, pid) = setup().await;
    let mut input = diag_input(pid, "  ", "无名", "{}");
    // 同时清掉 name 让校验两个分支都触发
    input.code = "".into();
    input.name = "  ".into();
    let r = create_diagram_inner(&pool, ORG, &input).await;
    assert!(r.is_err(), "空 code/name 应被拒：{r:?}");
}

#[tokio::test]
async fn create_diagram_unique_violation_per_project() {
    let (pool, pid) = setup().await;
    create_diagram_inner(&pool, ORG, &diag_input(pid, "DGM-SAME", "图甲", "{}"))
        .await
        .unwrap();
    let r = create_diagram_inner(&pool, ORG, &diag_input(pid, "DGM-SAME", "图乙", "{}")).await;
    assert!(r.is_err(), "同组织同项目同 code 应被拒：{r:?}");
}

#[tokio::test]
async fn create_diagram_same_code_in_other_project_allowed() {
    let (pool, pid1) = setup().await;
    // 第二个项目
    let p2 = create_project_inner(
        &pool,
        ORG,
        &ProjectInput {
            code: "PRJ-E".into(),
            name: "项目二".into(),
            client: "".into(),
            location: "".into(),
            phase: "design".into(),
            finished_at: "".into(),
            notes: "".into(),
        },
        ACTOR,
    )
    .await
    .unwrap();

    create_diagram_inner(&pool, ORG, &diag_input(pid1, "DGM-X", "图甲", "{}"))
        .await
        .unwrap();
    let r = create_diagram_inner(&pool, ORG, &diag_input(p2.id, "DGM-X", "图甲副本", "{}")).await;
    assert!(
        r.is_ok(),
        "同 code 跨项目应允许（UNIQUE 是 project_id+code 复合）：{r:?}"
    );
}

// ===========================================================================
// ensure_default_diagram_inner
// ===========================================================================

#[tokio::test]
async fn ensure_default_diagram_creates_placeholder_when_empty() {
    let (pool, pid) = setup().await;
    // 新项目刚建出来没有图
    let list = list_diagrams_inner(&pool, ORG, Some(pid)).await.unwrap();
    assert!(list.is_empty(), "新项目应无图");

    let d = ensure_default_diagram_inner(&pool, ORG, pid).await.unwrap();
    assert_eq!(d.project_id, pid);
    assert_eq!(d.code, "DGM-001");
    // ★ 契约：默认 data 必须是「明确的不可用快照」，绝不能被误当成有效快照。
    //   编辑器的判据是「JSON 对象里 blocks 是数组」，这份 data 必须不满足。
    //   历史事故：这里曾写入 {"version":1,"title":…,"sifs":[…]}（没有 blocks），
    //   编辑器 restore() 认了它 → blocks 落成 [] → 打开任何图都是空白画布；
    //   用户随手一保存，库里就永久只剩空图。见 M2.8.2。
    assert_eq!(
        d.data, "{}",
        "默认 data 约定为 \"{{}}\"（= 无可用快照 → 编辑器载入内置示例图当起点）"
    );
    let v: serde_json::Value = serde_json::from_str(&d.data).expect("默认 data 必须是合法 JSON");
    assert!(
        v.get("blocks").and_then(|b| b.as_array()).is_none(),
        "默认 data 不得含 blocks 数组，否则会被编辑器当成有效快照而清成空白：{}",
        d.data
    );
    // 应能在 list 里被查到
    let list2 = list_diagrams_inner(&pool, ORG, Some(pid)).await.unwrap();
    assert_eq!(list2.len(), 1);
    assert_eq!(list2[0].id, d.id);
}

#[tokio::test]
async fn ensure_default_diagram_returns_existing_when_present() {
    let (pool, pid) = setup().await;
    // 先手动建一张
    let manual = create_diagram_inner(&pool, ORG, &diag_input(pid, "DGM-MANUAL", "手工图", "{}"))
        .await
        .unwrap();
    // 再 ensure_default → 应返已有手工图（不重复建）
    let d = ensure_default_diagram_inner(&pool, ORG, pid).await.unwrap();
    assert_eq!(
        d.id, manual.id,
        "已有图时 ensure_default 必须返已有而不是再插一张"
    );
    assert_eq!(d.code, "DGM-MANUAL");

    let list = list_diagrams_inner(&pool, ORG, Some(pid)).await.unwrap();
    assert_eq!(list.len(), 1, "不能因为 ensure_default 再插一张");
}

// ===========================================================================
// 快照往返契约 —— 防「打开即空白」回归（M2.8.2）
// ===========================================================================

#[tokio::test]
async fn save_diagram_data_roundtrips_editor_snapshot_verbatim() {
    let (pool, pid) = setup().await;
    // 一份形状正确的 M0 编辑器快照：blocks 是数组（唯一有效性判据）
    let snap = concat!(
        r#"{"v":3,"blocks":[{"id":"b_1","type":"ai","col":0,"row":0,"tag":"PT-101","#,
        r#""inputs":[],"negated":[],"srcPort":[],"p":{}}],"rowCount":6,"#,
        r#""sifCols":["detect","vote","logic","latch","aux","final"],"sif":{},"silv":{},"#,
        r#""doc":{},"revs":[],"showSch":true,"autoFit":true,"showSlot":true,"wireMode":"cause"}"#
    );
    let d = create_diagram_inner(&pool, ORG, &diag_input(pid, "DWG-RT", "往返图", "{}"))
        .await
        .unwrap();

    save_diagram_data_inner(&pool, ORG, d.id, 0, snap)
        .await
        .unwrap();
    let got = get_diagram_inner(&pool, ORG, d.id).await.unwrap();

    // 必须原样往返：任何转义/截断都会让编辑器解析失败
    assert_eq!(got.data, snap, "编辑器快照必须原样落库，不得被加工");

    // 往返后仍满足编辑器的有效性判据
    let v: serde_json::Value = serde_json::from_str(&got.data).unwrap();
    assert!(
        v.get("blocks").and_then(|b| b.as_array()).is_some(),
        "往返后的 data 必须仍能被编辑器认作有效快照"
    );
    assert_eq!(v["blocks"][0]["tag"], "PT-101", "元素内容不能被改写");
}

// ===========================================================================
// list_diagrams_inner
// ===========================================================================

#[tokio::test]
async fn list_diagrams_filter_by_project() {
    let (pool, pid1) = setup().await;
    let p2 = create_project_inner(
        &pool,
        ORG,
        &ProjectInput {
            code: "PRJ-F".into(),
            name: "项目二".into(),
            client: "".into(),
            location: "".into(),
            phase: "design".into(),
            finished_at: "".into(),
            notes: "".into(),
        },
        ACTOR,
    )
    .await
    .unwrap();

    create_diagram_inner(&pool, ORG, &diag_input(pid1, "A1", "图 1", "{}"))
        .await
        .unwrap();
    create_diagram_inner(&pool, ORG, &diag_input(pid1, "A2", "图 2", "{}"))
        .await
        .unwrap();
    create_diagram_inner(&pool, ORG, &diag_input(p2.id, "B1", "另一项目图", "{}"))
        .await
        .unwrap();

    let l1 = list_diagrams_inner(&pool, ORG, Some(pid1)).await.unwrap();
    let l2 = list_diagrams_inner(&pool, ORG, Some(p2.id)).await.unwrap();
    let l_all = list_diagrams_inner(&pool, ORG, None).await.unwrap();
    assert_eq!(l1.len(), 2, "项目 1 应有 2 张图");
    assert_eq!(l2.len(), 1, "项目 2 应有 1 张图");
    assert_eq!(l_all.len(), 3, "无过滤时应返回 3 张图");
    // 按 code 排序
    assert_eq!(l1[0].code, "A1");
    assert_eq!(l1[1].code, "A2");
}

// ===========================================================================
// CASCADE：删除 project 后 diagram 不留
// ===========================================================================

#[tokio::test]
async fn deleting_project_cascades_diagrams() {
    let (pool, pid) = setup().await;
    let d = create_diagram_inner(&pool, ORG, &diag_input(pid, "CASCADE-1", "随项目走", "{}"))
        .await
        .unwrap();
    assert!(get_diagram_inner(&pool, ORG, d.id).await.is_ok());

    delete_project_inner(&pool, ORG, pid, ACTOR).await.unwrap();
    // CASCADE 后图必须不在
    assert!(
        get_diagram_inner(&pool, ORG, d.id).await.is_err(),
        "project 被删后图应一并清掉"
    );
}
