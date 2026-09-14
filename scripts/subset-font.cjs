/**
 * 生成审计包 PDF 用中文字体子集（SimHei）。
 *
 * 字符集策略（宁多勿缺，按覆盖度分层）：
 *   1. **GB2312 全字集**（6763 汉字 + 682 符号）—— 覆盖 99.9% 现代中文，
 *      含人名（王李张刘…）、工程术语、标点。这是主力。
 *   2. **GBK 扩展**（GB2312 之外的常用 GBK 区）—— 补生僻工程字
 *   3. ASCII + 拉丁扩展 + 通用标点（U+2000-206F，含 em dash — / 省略号 … / 引号）
 *   4. 源码扫描（src 下所有 ts / vue）—— 保证 UI 文案 / 报告字面量 100% 覆盖
 *   5. 工程术语补充表
 *
 * 为什么不 bundle 完整 SimHei？
 *   - 完整 SimHei.ttf 9.5MB（20902 汉字）→ base64 12.7MB，bundle 灾难
 *   - 本方案 ~7000 字 → ~1.8MB TTF，且**懒加载**（仅点「导出 PDF」时拉取）
 *
 * 用法：
 *   node scripts/subset-font.cjs            # 默认 GB2312 + 扫描
 *   node scripts/subset-font.cjs --minimal  # 仅源码扫描（体积最小）
 *
 * 输出：
 *   src/assets/fonts/SimHei-subset.ttf
 */
const subsetFont = require('subset-font');
const fs = require('fs');
const path = require('path');

const SRC = 'C:/Windows/Fonts/simhei.ttf';
const OUT = path.join(__dirname, '..', 'src', 'assets', 'fonts', 'SimHei-subset.ttf');
const SRC_DIR = path.join(__dirname, '..', 'src');
const MINIMAL = process.argv.includes('--minimal');

const seen = new Set();
function add(s) {
  for (const ch of s) seen.add(ch);
}

// ============================================================================
// 1. ASCII + 拉丁扩展 + 通用标点
// ============================================================================
(() => {
  let s = '';
  for (let c = 0x20; c <= 0x7e; c++) s += String.fromCharCode(c); // ASCII
  for (let c = 0xa0; c <= 0xff; c++) s += String.fromCharCode(c); // Latin-1（含 · ° × §）
  add(s);
})();
// 通用标点（em dash — / en dash – / 省略号 … / 弯引号 “ ” ‘ ’ / 单书名号）
(() => {
  let s = '';
  for (let c = 0x2000; c <= 0x206f; c++) s += String.fromCharCode(c);
  for (let c = 0x2e00; c <= 0x2e7f; c++) s += String.fromCharCode(c);
  add(s);
})();
// CJK 标点 + 全角形式
(() => {
  let s = '';
  for (let c = 0x3000; c <= 0x303f; c++) s += String.fromCharCode(c);
  for (let c = 0xff00; c <= 0xffef; c++) s += String.fromCharCode(c);
  add(s);
})();

// ============================================================================
// 2. GB2312 / GBK 汉字（主力）
// ============================================================================
function gbkChars(leadStart, leadEnd) {
  const dec = new TextDecoder('gbk');
  const out = [];
  for (let hi = leadStart; hi <= leadEnd; hi++) {
    for (let lo = 0xa1; lo <= 0xfe; lo++) {
      const ch = dec.decode(Buffer.from([hi, lo]));
      // 有效解码 = 单字符且非替换符
      if (ch.length === 1 && ch !== '\ufffd') out.push(ch);
    }
  }
  return out.join('');
}

if (!MINIMAL) {
  // GB2312：区 1-9 符号 + 区 16-87 汉字（0xA1-0xF7）
  add(gbkChars(0xa1, 0xf7));
  console.log('GB2312/GBK 汉字池已加入');
}

// ============================================================================
// 3. 源码扫描（UI 文案 / 报告字面量）
// ============================================================================
const SCAN_FILES = [];
function walk(dir) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === 'node_modules' || entry.name.startsWith('.')) continue;
    const p = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(p);
    else if (/\.(ts|vue|js)$/.test(entry.name) && !entry.name.endsWith('.d.ts')) {
      SCAN_FILES.push(p);
    }
  }
}
walk(SRC_DIR);

const CJK_RE = /[\u3000-\u303f\u4e00-\u9fff\uff00-\uffef]/g;
let scannedChars = 0;
for (const f of SCAN_FILES) {
  const content = fs.readFileSync(f, 'utf-8');
  const matches = content.match(CJK_RE);
  if (matches) {
    add(matches.join(''));
    scannedChars += matches.length;
  }
}
console.log(`扫描 ${SCAN_FILES.length} 个源文件，提取 ${scannedChars} 个 CJK 字符`);

// ============================================================================
// 4. 工程术语补充（业务词汇）
// ============================================================================
const ENGINEERING_EXTRA = [
  '报告生成时间','筛选条件','起止时间','汇总统计','详细记录','附录','备注','完整记录',
  '不限','全部','命中','预览显示','如需','请改用','以下显示','全字段','无变更',
  '仪表','台账','位号','量程','设定值','复位值','切断','放空','联锁','旁路','隔离',
  '反应器','再生器','吸收塔','解析塔','分馏塔','换热器','冷却器','加热器','冷凝器',
  '检测','逻辑','最终','手操','自动','手动','远程','就地','本安','隔爆',
  '压力','温度','流量','液位','分析','压差','转速','振动','开关','变送器','传感器',
  '调节阀','切断阀','执行器','定位器','手操器','报警器','指示灯',
  '安全','完整性','等级','目标','设计','验证','实际','期望','需求','供给',
  '平均','失效率','检验','周期','覆盖率','检修','维修','维护','故障','失效',
  '误动','拒动','误跳','需求模式','冗余','热备','冷备','切换',
  '系统','模块','组件','插件','服务','接口','配置','参数','设置','默认值','枚举',
  '常量','变量','字段','列','行','表','视图','表单','按钮','菜单','导航','页眉','页脚',
  '标签','提示','警告','错误','成功','失败','跳过','确认','取消','返回','前进',
  '下一页','上一页','首页','末页','创建','更新','删除','修改','变更',
  '操作人','操作员','工程师','主任','技术员','负责人','审查','审核','批准','驳回',
  '审计','审计包','审计日志','审计员','审计报告','合规','取证','留痕','追溯','证据',
  '变更前','变更后','字段变更','关联','解除','导出','导入',
  '季度','半年','年度','本周','上周','下周','本月','上月','下月','今年','去年','明年',
  '千克','毫克','毫升','立方米','厘米','毫米','微米','千米','千瓦','兆瓦','兆帕','千帕',
  '摄氏度','百分比','频率','赫兹','帕斯卡',
  '强制','推荐','施行','实施','修订','替代','废止','等同','采用','引用','依据','符合',
  '满足','达到','超出','低于','异常','备用','停用','启用','锁定','解锁','投入','退出','切除',
  '通过','合格','达标','过期','到期','进行中','已完成','未开始','已取消',
  '草稿','待审','已审','作废','正式','现行',
  '监理','环保','消防','职业','卫生','健康','应急','响应','预案','演练','培训','考核',
  '检查','督查','整改','复查','闭环',
];
ENGINEERING_EXTRA.forEach(add);

const allChars = Array.from(seen).sort().join('');
console.log(`字符集大小: ${allChars.length} 个字符`);

// ============================================================================
// 子集化
// ============================================================================
const ttf = fs.readFileSync(SRC);
console.log(`源字体: ${(ttf.length / 1024).toFixed(1)} KB`);

subsetFont(ttf, allChars, { targetFormat: 'truetype' })
  .then((subsetBuffer) => {
    fs.mkdirSync(path.dirname(OUT), { recursive: true });
    fs.writeFileSync(OUT, Buffer.from(subsetBuffer));
    console.log(`✓ 输出: ${OUT}`);
    console.log(`  大小:   ${(subsetBuffer.length / 1024).toFixed(1)} KB`);
    console.log(`  base64: ${((subsetBuffer.length * 4) / 3 / 1024).toFixed(1)} KB（inject 到 bundle 后）`);
    console.log(`  压缩比: ${((subsetBuffer.length / ttf.length) * 100).toFixed(2)}%`);
  })
  .catch((err) => {
    console.error('subset-font failed:', err);
    process.exit(1);
  });