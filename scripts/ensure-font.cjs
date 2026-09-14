/**
 * prebuild 守卫：字体子集不存在时自动生成。
 *
 * 为什么需要：
 *   - `src/assets/fonts/SimHei-subset.ttf` 是从**系统字体**（Windows SimHei）派生的
 *     子集，SimHei 有版权、不可再分发 → 已加 .gitignore，不入库
 *   - 于是克隆仓库后首次 build 会 404 → 这里自动补生成
 *
 * 用法（已挂在 package.json 的 prebuild）：
 *   npm run build          # 自动触发
 *   npm run font:subset    # 强制重新生成
 *
 * 非 Windows / 无 SimHei 时：给出清晰错误 + 替代方案。
 */
const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

const OUT = path.join(__dirname, '..', 'src', 'assets', 'fonts', 'SimHei-subset.ttf');
const SRC_CANDIDATES = [
  'C:/Windows/Fonts/simhei.ttf',
  'C:/Windows/Fonts/msyh.ttc', // 微软雅黑（ttc 集合，subset-font 可能不支持）
  '/System/Library/Fonts/PingFang.ttc',
  '/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc',
];

if (fs.existsSync(OUT)) {
  const kb = (fs.statSync(OUT).size / 1024).toFixed(1);
  console.log(`[ensure-font] 字体子集已存在（${kb} KB），跳过生成`);
  process.exit(0);
}

const available = SRC_CANDIDATES.find((p) => fs.existsSync(p));
if (!available) {
  console.error(
    '\n[ensure-font] ✗ 未找到可用中文字体源。\n' +
      '  PDF 报告需要中文字体子集，请在以下任一位置准备字体后重试：\n' +
      SRC_CANDIDATES.map((p) => `    - ${p}`).join('\n') +
      '\n  或手工放置一份 TTF 到 src/assets/fonts/SimHei-subset.ttf\n',
  );
  process.exit(1);
}

if (available !== 'C:/Windows/Fonts/simhei.ttf') {
  console.warn(
    `[ensure-font] ⚠ 系统 SimHei 不可用，改用 ${available}\n` +
      '  （subset-font 对 .ttc 字体集合支持有限，可能失败；\n' +
      '   建议手工转换出单个 TTF face 后放到 scripts/subset-font.cjs 的 SRC 常量）',
  );
}

console.log(`[ensure-font] 字体子集缺失，从 ${available} 生成…`);
try {
  execFileSync(process.execPath, [path.join(__dirname, 'subset-font.cjs')], {
    stdio: 'inherit',
  });
} catch (e) {
  console.error('[ensure-font] ✗ 生成失败：', e.message);
  process.exit(1);
}
