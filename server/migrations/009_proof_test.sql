-- ===========================================================================
-- 009_proof_test.sql
--
-- 检验测试管理（IEC 61511-1 §16.3）
-- 每个 SIF 按其 proof_interval（月）定期做检验测试，记录测试结果与到期日。
-- next_due_at = tested_at + proof_interval 个月（由后端 INSERT 时用 SQLite date() 计算）。
-- 历史记录的 next_due_at 是固化的合规证据，不随 SIF.proof_interval 变更回填。
--
-- 状态计算（运行时 SQL CASE，不存冗余列）：
--   overdue = 同 sif_id 的最新一条 AND next_due_at < today
--   current = 其它
--
-- SIF 删除 → proof_test CASCADE（与 bypass_record 一致）。
-- ===========================================================================

CREATE TABLE IF NOT EXISTS proof_test (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    org_id       INTEGER NOT NULL REFERENCES org(id) ON DELETE CASCADE,
    sif_id       INTEGER NOT NULL REFERENCES sif(id) ON DELETE CASCADE,
    tested_at    TEXT    NOT NULL,            -- 测试执行日期（ISO 8601 date）
    result       TEXT    NOT NULL DEFAULT 'pass',
    tested_by    TEXT    NOT NULL DEFAULT '',
    next_due_at  TEXT    NOT NULL,            -- 下次到期日
    findings     TEXT    NOT NULL DEFAULT '',  -- 发现的问题
    notes        TEXT    NOT NULL DEFAULT '',
    created_at   TEXT    NOT NULL DEFAULT (datetime('now')),
    CHECK (result IN ('pass', 'fail', 'conditional'))
);

CREATE INDEX IF NOT EXISTS idx_pt_sif  ON proof_test(sif_id);
CREATE INDEX IF NOT EXISTS idx_pt_org  ON proof_test(org_id);
CREATE INDEX IF NOT EXISTS idx_pt_due  ON proof_test(next_due_at);
