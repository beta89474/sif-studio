-- ===========================================================================
-- 012_sif_mttr_beta.sql
--
-- PFDavg / PFH 计算引擎补全（IEC 61511-2 §6）：
--   mttr_hours  ：平均修复时间（小时），危险已检测失效 λDD 在修复期间暴露，
--                 PFD 补 λDD × MTTR 项；典型值 8h
--   beta_factor ：冗余通道共因失效因子（0~1，典型 2%~10%），
--                 仅作用于 1oo2/2oo3/2oo4 表决架构：
--                 PFD 补 β·λDU·TI/2，PFH 补 β·λDU
-- ===========================================================================

ALTER TABLE sif ADD COLUMN mttr_hours REAL NOT NULL DEFAULT 8.0;
ALTER TABLE sif ADD COLUMN beta_factor REAL NOT NULL DEFAULT 0.10;
