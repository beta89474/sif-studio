/**
 * 验证：PDF 报告里所有会出现的字符都在字体子集里。
 * 用法：node scripts/verify-font-coverage.cjs
 */
const fs = require('fs');
const path = require('path');

const FONT = path.join(__dirname, '..', 'src', 'assets', 'fonts', 'SimHei-subset.ttf');
const TARGETS = [
  path.join(__dirname, '..', 'src', 'utils', 'auditPdf.ts'),
  path.join(__dirname, '..', 'src', 'views', 'AuditCenter.vue'),
];

// 读字体表（用 subset-font 内部用的 harfbuzzjs 太重，改用简单方法：
// 直接读 TTF 的 cmap 表拿覆盖的 code point）
function readCmap(file) {
  const buf = fs.readFileSync(file);
  const numTables = buf.readUInt16BE(4);
  let cmapOffset = null;
  for (let i = 0; i < numTables; i++) {
    const off = 12 + i * 16;
    const tag = buf.toString('ascii', off, off + 4);
    if (tag === 'cmap') {
      cmapOffset = buf.readUInt32BE(off + 8);
      break;
    }
  }
  if (!cmapOffset) throw new Error('no cmap table');
  const numSubtables = buf.readUInt16BE(cmapOffset + 2);
  const covered = new Set();
  for (let s = 0; s < numSubtables; s++) {
    const subOff = cmapOffset + 4 + s * 8;
    const platformID = buf.readUInt16BE(subOff);
    const encodingID = buf.readUInt16BE(subOff + 2);
    const offset = buf.readUInt32BE(subOff + 4);
    // 只要 (3,1) Windows BMP 或 (3,10) Windows UCS-4
    if (!((platformID === 3 && encodingID === 1) || (platformID === 3 && encodingID === 10) || platformID === 0)) continue;
    const tableOff = cmapOffset + offset;
    const format = buf.readUInt16BE(tableOff);
    if (format === 4) {
      const segCount = buf.readUInt16BE(tableOff + 6) / 2;
      const endBase = tableOff + 14;
      const startBase = endBase + segCount * 2 + 2;
      for (let seg = 0; seg < segCount; seg++) {
        const end = buf.readUInt16BE(endBase + seg * 2);
        const start = buf.readUInt16BE(startBase + seg * 2);
        if (start === 0xffff && end === 0xffff) continue;
        for (let c = start; c <= end && c !== 0x10000; c++) covered.add(c);
      }
    } else if (format === 12) {
      const nGroups = buf.readUInt32BE(tableOff + 12);
      for (let g = 0; g < nGroups; g++) {
        const gOff = tableOff + 16 + g * 12;
        const start = buf.readUInt32BE(gOff);
        const end = buf.readUInt32BE(gOff + 4);
        for (let c = start; c <= end; c++) covered.add(c);
      }
    }
  }
  return covered;
}

const covered = readCmap(FONT);
console.log(`字体覆盖: ${covered.size} 个码位`);

// 收集报告字符串里所有字符
const CJK_RE = /[\u3000-\u303f\u4e00-\u9fff\uff00-\uffef]/g;
const missing = new Map(); // char -> files

for (const f of TARGETS) {
  const content = fs.readFileSync(f, 'utf-8');
  const chars = content.match(CJK_RE) || [];
  for (const ch of chars) {
    const cp = ch.codePointAt(0);
    if (!covered.has(cp)) {
      if (!missing.has(ch)) missing.set(ch, []);
      const rel = path.relative(path.join(__dirname, '..'), f);
      if (!missing.get(ch).includes(rel)) missing.get(ch).push(rel);
    }
  }
}

if (missing.size === 0) {
  console.log('✓ 全部覆盖，无缺字');
} else {
  console.log(`✗ 缺 ${missing.size} 个字符：`);
  for (const [ch, files] of missing) {
    console.log(`   '${ch}' (U+${ch.codePointAt(0).toString(16).toUpperCase()}) ← ${files.join(', ')}`);
  }
  process.exit(1);
}