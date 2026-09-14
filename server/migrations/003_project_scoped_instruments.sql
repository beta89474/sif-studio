-- ===========================================================================
-- SIF Studio / 联锁工坊 — v003 迁移：仪表台账按项目隔离（M2.9）
--
-- 触发场景：
--   M2.9  实施项目-仪表-图三段打通的第一步：仪表台账不再全局唯一 tag，
--         改为按项目隔离。同一台仪表在不同项目里可以同名（罕见但合法：
--         不同装置的两台独立 PT-101 不需要共享主键）；同一项目里 tag 必须唯一。
--
-- 设计原则：
--   - 联合唯一 (project_id, tag)，与 IEC 61511 / ISA 5.1 实务一致：
--     项目内部 KKS / 项目编号前缀通常已经保证全局唯一，但 schema 只强制项目内唯一
--   - project_id 列允许 NULL —— 这是为了迁移中断 / 旧数据无 project 时优雅降级。
--     应用层（Rust 命令 + Vue 表单）必须保证写入时非空，存量数据迁移后归零。
--   - 旧全局唯一索引 idx_instrument_tag 必须先 DROP：
--     若同一 project 内已有重复 tag，DROP 后才能加联合唯一。审计可追溯原始冲突。
--   - 旧数据归到一个新建项目 PRJ-LEGACY（用户可见，可在 Instruments.vue 里
--     手动重指到正式项目；归档时一并删）。
--
-- 旧数据策略：
--   1. 先 INSERT PRJ-LEGACY（如不存在）—— 拿它的 id
--   2. 给 instrument 加 project_id 列
--   3. 把所有 NULL project_id 的行指向 PRJ-LEGACY
--   4. 改造唯一索引
-- ===========================================================================

-- ─── 1. 创建兜底项目 ────────────────────────────────────────────────────────
INSERT INTO project (code, name, client, location, phase, notes)
SELECT 'PRJ-LEGACY',
       '未分配遗留库（迁移自动生成）',
       '系统', '系统', 'design',
       'M2.9 迁移前仪表表无 project_id 归属。请把仪表重指到正式项目后归档此项目。'
WHERE NOT EXISTS (SELECT 1 FROM project WHERE code = 'PRJ-LEGACY');

-- ─── 2. 加 project_id 列 ────────────────────────────────────────────────────
-- 注：SQLite ALTER TABLE 不支持 NOT NULL 重写，因此保留 NULL 兼容迁移中断场景。
-- 应用层 (create_instrument_inner / update_instrument_inner) 必须校验非空。
ALTER TABLE instrument ADD COLUMN project_id INTEGER REFERENCES project(id) ON DELETE CASCADE;

-- ─── 3. 旧数据归位：所有 NULL project_id → PRJ-LEGACY ────────────────────────
UPDATE instrument
   SET project_id = (SELECT id FROM project WHERE code = 'PRJ-LEGACY' LIMIT 1)
 WHERE project_id IS NULL;

-- ─── 4. 改造唯一索引：drop 全局，加 (project_id, tag) 联合唯一 ───────────────
-- DROP INDEX 在 SQLite 3.7+ 支持。Tauri 携带的 SQLite 已远高于此。
DROP INDEX IF EXISTS idx_instrument_tag;
CREATE UNIQUE INDEX IF NOT EXISTS idx_instrument_project_tag ON instrument(project_id, tag);

-- ─── 5. 高频查询索引：项目内按角色过滤（"列项目下所有 detector"） ────────────
CREATE INDEX IF NOT EXISTS idx_instrument_project      ON instrument(project_id);
CREATE INDEX IF NOT EXISTS idx_instrument_project_role ON instrument(project_id, role);