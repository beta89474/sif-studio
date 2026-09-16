-- 016 — 检验测试规程（SOP/Test Plan）库 —— IEC 61511-1 §16.2.2
--
-- §16.2.2 要求功能测试按成文规程（documented procedure）执行：
-- 规程应规定测试步骤、通过判据、负责人与外部文档编号。SOP 可跨 SIF
-- 复用（同型号仪表共用规程），并以 version 留痕；检验记录软关联 SOP，
-- 删除规程时 proof_test.sop_id SET NULL，保留检验历史不被破坏。

CREATE TABLE IF NOT EXISTS proof_test_sop (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    org_id        INTEGER NOT NULL REFERENCES org(id) ON DELETE CASCADE,
    code          TEXT    NOT NULL,                 -- 规程编号（组织内唯一）
    title         TEXT    NOT NULL,                 -- 规程名称
    version       TEXT    NOT NULL DEFAULT 'v1.0',  -- 规程版本
    doc_ref       TEXT    NOT NULL DEFAULT '',      -- 外部文档编号（QMS/DMS 索引）
    scope         TEXT    NOT NULL DEFAULT '',      -- 适用范围（元件类型/SIF 类别）
    test_method   TEXT    NOT NULL DEFAULT '',      -- 检验方法与步骤（成文规程正文）
    pass_criteria TEXT    NOT NULL DEFAULT '',      -- 通过判据
    notes         TEXT    NOT NULL DEFAULT '',
    created_at    TEXT    NOT NULL DEFAULT (datetime('now')),
    updated_at    TEXT    NOT NULL DEFAULT (datetime('now')),
    UNIQUE (org_id, code)
);

CREATE INDEX IF NOT EXISTS idx_ptsop_org ON proof_test_sop(org_id);

ALTER TABLE proof_test ADD COLUMN sop_id INTEGER
    REFERENCES proof_test_sop(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_pt_sop ON proof_test(sop_id);
