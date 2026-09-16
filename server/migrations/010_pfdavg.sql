-- ===========================================================================
-- 010_pfdavg.sql
--
-- PFDavg 计算引擎（IEC 61511-2 §6）
--
-- 1. instrument 表补充失效参数字段（来自 FMEDA / 设备手册）：
--      lambda_du   危险未检测失效率（/h）
--      lambda_dd   危险已检测失效率（/h）
--      lambda_su   安全未检测失效率（/h）
--      lambda_sd   安全已检测失效率（/h）
--      sff         安全失效分数 SFF（0~1）
--      pt_coverage 检验测试覆盖率 PTC（0~1）
--      hft         硬件故障容忍度（0/1/2）
--
-- 2. sif 表补充三个子系统的表决架构（IEC 61511-2 §6.2）：
--      sensor_arch  检测元件架构
--      logic_arch   逻辑解算器架构
--      final_arch   最终元件架构
--      取值：1oo1 | 1oo2 | 2oo2 | 2oo3 | 2oo4
--
-- 说明：
-- - PFDavg 不在库里存冗余列，由后端 Rust 运行时计算（失效参数/架构/TI 变更即重算）
-- - 迁移使用 ALTER TABLE ADD COLUMN，SQLite 支持且不破坏现有数据
-- ===========================================================================

-- instrument 失效参数（IEC 61511-2 表 3 / IEC 61508-2）
ALTER TABLE instrument ADD COLUMN lambda_du    REAL NOT NULL DEFAULT 0;  -- /h
ALTER TABLE instrument ADD COLUMN lambda_dd    REAL NOT NULL DEFAULT 0;  -- /h
ALTER TABLE instrument ADD COLUMN lambda_su    REAL NOT NULL DEFAULT 0;  -- /h
ALTER TABLE instrument ADD COLUMN lambda_sd    REAL NOT NULL DEFAULT 0;  -- /h
ALTER TABLE instrument ADD COLUMN sff          REAL NOT NULL DEFAULT 0;  -- 0~1
ALTER TABLE instrument ADD COLUMN pt_coverage  REAL NOT NULL DEFAULT 1;  -- 0~1
ALTER TABLE instrument ADD COLUMN hft          INTEGER NOT NULL DEFAULT 0;  -- 0/1/2

-- sif 子系统表决架构
ALTER TABLE sif ADD COLUMN sensor_arch TEXT NOT NULL DEFAULT '1oo1';
ALTER TABLE sif ADD COLUMN logic_arch  TEXT NOT NULL DEFAULT '1oo1';
ALTER TABLE sif ADD COLUMN final_arch  TEXT NOT NULL DEFAULT '1oo1';

-- 约束：架构取值
-- （SQLite 不支持 ALTER TABLE ADD CHECK，改用触发器或应用层校验；
--   这里用索引 + 应用层 CHECK 保证）
