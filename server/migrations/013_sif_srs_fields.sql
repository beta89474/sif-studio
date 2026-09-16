-- ===========================================================================
-- 013_sif_srs_fields.sql
--
-- SRS（安全需求规格书）字段持久化（IEC 61511-1 §10/§12）：
--   editor.html 的 SRS Tab 原本仅存图编辑器内存（diagram JSON），
--   后端 sif 表无对应列，无法备份/查询/跨端同步。本轮补 8 个 SRS 专属
--   TEXT 列，使 SRS 摘要随 SIF 实体一起落库、审计、备份/恢复。
--
--   SRS Tab 已有但已落到后端的字段（不重复加列）：
--     sifNo=code / sil=sil_design/sil_verified / mode=demand_mode /
--     proofTest=proof_interval / pfdTarget=pfdavg_target / pfdCalc=运行时计算
--     desc=description
--
--   本轮新增 8 列（全部 TEXT NOT NULL DEFAULT ''，允许空串占位）：
--     plant          项目/装置（装置标识）
--     unit           工艺单元
--     equip          关联设备（被保护设备）
--     response_time  响应时间要求（PST / 响应规格）
--     safe_state     安全状态定义（安全面）
--     reset_req      复位要求（复位逻辑）
--     bypass_req     旁路管理（旁路规则）
--     design_standard 设计依据标准（IEC/GB 引用）
-- ===========================================================================

ALTER TABLE sif ADD COLUMN plant          TEXT NOT NULL DEFAULT '';
ALTER TABLE sif ADD COLUMN unit           TEXT NOT NULL DEFAULT '';
ALTER TABLE sif ADD COLUMN equip          TEXT NOT NULL DEFAULT '';
ALTER TABLE sif ADD COLUMN response_time  TEXT NOT NULL DEFAULT '';
ALTER TABLE sif ADD COLUMN safe_state     TEXT NOT NULL DEFAULT '';
ALTER TABLE sif ADD COLUMN reset_req      TEXT NOT NULL DEFAULT '';
ALTER TABLE sif ADD COLUMN bypass_req     TEXT NOT NULL DEFAULT '';
ALTER TABLE sif ADD COLUMN design_standard TEXT NOT NULL DEFAULT '';
