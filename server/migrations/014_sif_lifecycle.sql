-- ===========================================================================
-- 014_sif_lifecycle.sql
--
-- SIF 生命周期阶段（IEC 61511-1 §5  Safety Lifecycle）：
--   project 表已有 phase（design/construction/commissioning/operation/closed）
--   用于跟踪装置级工程阶段；sif 表此前无独立生命周期字段，无法跟踪单条
--   SIF 的工程流转（设计→建造→调试→运行→停用）。本轮补 sif.lifecycle_phase，
--   复用与 project.phase 一致的 5 阶段枚举，默认 'design'。
--
--   说明：sif.lifecycle_phase 与 project.phase 独立维护——同一装置在不同阶段
--   可能有处于不同生命周期的 SIF（如新建 SIF 仍在 design，既有 SIF 已 operation）。
-- ===========================================================================

ALTER TABLE sif ADD COLUMN lifecycle_phase TEXT NOT NULL DEFAULT 'design'
    CHECK (lifecycle_phase IN ('design','construction','commissioning','operation','closed'));
