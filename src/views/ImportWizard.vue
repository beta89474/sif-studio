<script setup lang="ts">
/**
 * ImportWizard.vue — 全局导入页（薄壳）
 *
 * 实现已抽取为 components/ImportPanel.vue（双模式面板）：
 *   - 全局模式：本页，STEP 3 手动选目标项目
 *   - 项目锁定模式：项目详情页 /projects/:id 的"数据导入"Tab
 */
import ImportPanel from "../components/ImportPanel.vue";
</script>

<template>
  <ImportPanel />
</template>
