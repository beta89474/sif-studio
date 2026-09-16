<template>
  <div class="sif">
    <!-- 统计 -->
    <section class="stats">
      <div class="panel stat-card">
        <div class="panel-header">检测元件 · DETECTOR</div>
        <div class="panel-value">{{ totalDetector }}</div>
        <div class="panel-foot">全库汇总</div>
      </div>
      <div class="panel stat-card">
        <div class="panel-header">最终元件 · FINAL</div>
        <div class="panel-value">{{ totalFinal }}</div>
        <div class="panel-foot">安全动作</div>
      </div>
      <div class="panel stat-card">
        <div class="panel-header">旁路关联 · AUX</div>
        <div class="panel-value">{{ totalAux }}</div>
        <div class="panel-foot">需限时 + 报警</div>
      </div>
      <div class="panel stat-card">
        <div class="panel-header">SIF 数 · COUNT</div>
        <div class="panel-value">{{ filtered.length }}</div>
        <div class="panel-foot">跨图汇总</div>
      </div>
    </section>

    <!-- 过滤 -->
    <section class="head card">
      <input class="search" v-model="query" placeholder="按编号 / 名称搜索…" />
      <select v-model="silFilter">
        <option value="">全部 SIL</option>
        <option v-for="s in ['NA','A','B','C','D']" :key="s" :value="s">SIL {{ s }}</option>
      </select>
      <select v-if="!locked" v-model="projectFilter">
        <option value="">全部项目</option>
        <option v-for="p in store.projects" :key="p.id" :value="p.id">{{ p.code }} {{ p.name }}</option>
      </select>
      <button v-if="auth.canWrite" class="primary" @click="openCreate">+ 新建 SIF</button>
    </section>

    <!-- 列表 -->
    <table v-if="filtered.length">
      <thead>
        <tr>
          <th style="width: 32px"></th>
          <th style="width: 90px">项目 · PRJ</th>
          <th style="width: 120px">编号 · CODE</th>
          <th>名称 · NAME</th>
          <th style="width: 80px">SIL</th>
          <th class="num" style="width: 60px">图</th>
          <th class="num" style="width: 60px">检测</th>
          <th class="num" style="width: 60px">最终</th>
          <th class="num" style="width: 60px">旁路</th>
          <th style="width: 140px">操作</th>
        </tr>
      </thead>
      <tbody>
        <template v-for="s in filtered" :key="s.id">
          <tr :class="{ active: expanded === s.id }">
            <td>
              <button class="icon" @click="toggle(s.id)">
                <ChevronRight v-if="expanded !== s.id" :size="14" />
                <ChevronDown v-else :size="14" />
              </button>
            </td>
            <td class="mono">{{ s.projectCode }}</td>
            <td class="mono">{{ s.code }}</td>
            <td>{{ s.name }}</td>
            <td>
              <span class="tag" :class="`sil-${s.silVerified.toLowerCase()}`">{{ s.silVerified }}</span>
              <span class="tag vs" :class="`vs-${s.silVerifyStatus}`" :title="`silAchieved=${s.silAchieved}`">{{ verifyCN(s.silVerifyStatus) }}</span>
            </td>
            <td class="num">{{ s.diagramCount }}</td>
            <td class="num">{{ s.detectorCount }}</td>
            <td class="num">{{ s.finalCount }}</td>
            <td class="num">{{ s.auxCount }}</td>
            <td class="row-actions">
              <button v-if="auth.canWrite" class="sm" @click="openEdit(s)">编辑</button>
              <button class="sm" @click="openHistory(s.id)">历史</button>
            </td>
          </tr>
          <tr v-if="expanded === s.id" class="detail-row">
            <td colspan="9">
              <div class="detail">
                <!-- 元数据 -->
                <div class="detail-block">
                  <h4>元数据 · META</h4>
                  <dl class="kv">
                    <dt>项目</dt><dd class="mono">{{ s.projectCode }}</dd>
                    <dt>设计 SIL</dt><dd class="mono">SIL {{ s.silDesign }}</dd>
                    <dt>需求模式</dt><dd>{{ s.demandMode }}</dd>
                    <dt>PFDavg 目标</dt><dd class="mono">{{ s.pfdavgTarget ?? "—" }}</dd>
                    <dt>PFDavg 计算</dt><dd class="mono">{{ s.pfdavgCalculated != null ? s.pfdavgCalculated.toExponential(2) : "—" }}</dd>
                    <template v-if="s.demandMode === 'high'">
                      <dt>PFH 计算 (1/h)</dt><dd class="mono">{{ s.pfhCalculated != null ? s.pfhCalculated.toExponential(2) : "—" }}</dd>
                    </template>
                    <dt>达到 SIL</dt><dd><span class="tag" :class="`sil-${s.silAchieved.toLowerCase()}`">{{ s.silAchieved }}</span></dd>
                    <dt>验证状态</dt><dd><span class="tag vs" :class="`vs-${s.silVerifyStatus}`">{{ verifyCN(s.silVerifyStatus) }}</span></dd>
                    <dt>检验周期</dt><dd class="mono">{{ s.proofInterval }} 月</dd>
                    <dt>MTTR</dt><dd class="mono">{{ s.mttrHours }} h</dd>
                    <dt>共因因子 β</dt><dd class="mono">{{ s.betaFactor }}</dd>
                    <dt>检测架构</dt><dd class="mono">{{ s.sensorArch }}</dd>
                    <dt>逻辑架构</dt><dd class="mono">{{ s.logicArch }}</dd>
                    <dt>最终架构</dt><dd class="mono">{{ s.finalArch }}</dd>
                    <dt>生命周期</dt><dd class="mono">{{ phaseCN(s.lifecyclePhase) }}</dd>
                    <dt>说明</dt><dd class="full">{{ s.description || "—" }}</dd>
                  </dl>
                </div>

                <!-- Route 1H 硬件安全完整性（IEC 61508-2 表 2/3 × SFF × HFT） -->
                <div class="detail-block full">
                  <h4>硬件安全完整性 · ROUTE 1H</h4>
                  <table class="r1h">
                    <thead>
                      <tr>
                        <th>子系统</th>
                        <th>表决架构</th>
                        <th class="num">有效 HFT</th>
                        <th class="num">实际通道</th>
                        <th class="num">要求通道</th>
                        <th>匹配状态</th>
                      </tr>
                    </thead>
                    <tbody>
                      <tr v-for="sub in [
                        { label: '检测', arch: s.sensorArch, n: s.detectorCount, m: s.sensorMatch },
                        { label: '逻辑', arch: s.logicArch, n: s.logicCount, m: s.logicMatch },
                        { label: '最终', arch: s.finalArch, n: s.finalCount, m: s.finalMatch },
                      ]" :key="sub.label">
                        <td>{{ sub.label }}</td>
                        <td class="mono">{{ sub.arch }}</td>
                        <td class="num mono">{{ hftOf(sub.arch) }}</td>
                        <td class="num mono">{{ sub.n }}</td>
                        <td class="num mono">{{ needChannels(sub.arch) }}</td>
                        <td>
                          <span
                            class="tag"
                            :class="sub.m === 'matched' ? 'vs-verified' : 'vs-overclaimed'"
                            :title="sub.m === 'degraded'
                              ? '通道数与表决架构不符，PFD/PFH 已按串联退化公式计算，请补齐仪表或调整架构'
                              : sub.m === 'empty' ? '该子系统未关联任何仪表' : ''"
                          >{{ matchCN(sub.m) }}</span>
                        </td>
                      </tr>
                    </tbody>
                  </table>
                </div>

                <!-- 检测位号（ISA 5.1 bubble 风格） -->
                <div class="detail-block">
                  <h4>检测位号 · DETECTOR</h4>
                  <div class="chips">
                    <span v-for="t in s.detectorsCsv.split(',').filter(Boolean)" :key="t" class="bubble detector mono">{{ t }}</span>
                    <span v-if="!s.detectorsCsv" class="muted">未关联</span>
                  </div>
                </div>

                <!-- 最终元件 -->
                <div class="detail-block">
                  <h4>最终元件 · FINAL</h4>
                  <div class="chips">
                    <span v-for="t in s.finalsCsv.split(',').filter(Boolean)" :key="t" class="bubble final mono">{{ t }}</span>
                    <span v-if="!s.finalsCsv" class="muted">未关联</span>
                  </div>
                </div>

                <!-- 出现位置 -->
                <div class="detail-block">
                  <h4>出现位置 · LOC</h4>
                  <div class="chips">
                    <span v-for="t in s.diagramsCsv.split(',').filter(Boolean)" :key="t" class="chip-loc mono">{{ t }}</span>
                    <span v-if="!s.diagramsCsv" class="muted">未挂图</span>
                  </div>
                </div>

                <!-- 已关联仪表 -->
                <div class="detail-block full">
                  <div class="block-head">
                    <h4>已关联的仪表 · LINKED INSTRUMENTS</h4>
                    <button v-if="auth.canWrite" class="sm primary" @click="openLink(s.id)">+ 关联仪表</button>
                  </div>
                  <table v-if="sifLinks.length" class="links">
                    <thead>
                      <tr>
                        <th>位号</th><th>角色</th><th>类型</th><th>SIL</th><th>备注</th><th></th>
                      </tr>
                    </thead>
                    <tbody>
                      <tr v-for="l in sifLinks" :key="l.id">
                        <td class="mono">{{ l.tag }}</td>
                        <td><span class="tag" :class="l.role">{{ roleCN(l.role) }}</span></td>
                        <td class="mono">{{ l.kind }}</td>
                        <td><span class="tag" :class="`sil-${l.silTarget.toLowerCase()}`">{{ l.silTarget }}</span></td>
                        <td>{{ l.note || "—" }}</td>
                        <td v-if="auth.canWrite" class="row-actions"><button class="danger sm" @click="unlink(l.id, s.id)">解除</button></td>
                        <td v-else></td>
                      </tr>
                    </tbody>
                  </table>
                  <div v-else class="muted">还没有任何关联，点击上方按钮加一条。</div>
                </div>

                <!-- 活动旁路（M2.2 IEC 61511-1 §11.5.2） -->
                <div class="detail-block full">
                  <div class="block-head">
                    <h4>活动旁路 · ACTIVE BYPASS</h4>
                    <router-link to="/bypass" class="block-link">登记新旁路 →</router-link>
                  </div>
                  <BypassCard :sif-id="s.id" :max="5" />
                </div>

                <!-- 检验测试（IEC 61511-1 §16.3） -->
                <div class="detail-block full">
                  <div class="block-head">
                    <h4>检验测试 · PROOF TEST</h4>
                    <button v-if="auth.canWrite" class="sm primary" @click="openProofTest(s.id)">+ 记录检验</button>
                  </div>
                  <table v-if="proofTestsFor(s.id).length" class="sub-table">
                    <thead>
                      <tr>
                        <th style="width: 110px">测试日期</th>
                        <th style="width: 60px">结果</th>
                        <th style="width: 80px">测试人</th>
                        <th style="width: 110px">下次到期</th>
                        <th style="width: 60px">状态</th>
                        <th style="width: 130px">规程 · SOP</th>
                        <th>发现</th>
                        <th style="width: 80px"></th>
                      </tr>
                    </thead>
                    <tbody>
                      <tr v-for="pt in proofTestsFor(s.id)" :key="pt.id">
                        <td class="mono">{{ pt.testedAt }}</td>
                        <td><span class="tag" :class="`pt-${pt.result}`">{{ pt.result }}</span></td>
                        <td>{{ pt.testedBy || "—" }}</td>
                        <td class="mono">{{ pt.nextDueAt }}</td>
                        <td><span class="tag" :class="`pts-${pt.status}`">{{ pt.status === "overdue" ? "逾期" : "正常" }}</span></td>
                        <td>
                          <span v-if="pt.sopLinked" class="tag vs vs-verified" :title="pt.sopTitle ?? ''">
                            {{ pt.sopCode }} {{ pt.sopVersion }}
                          </span>
                          <span v-else class="tag vs vs-pending" title="IEC 61511-1 §16.2.2 要求按成文规程执行">无规程</span>
                        </td>
                        <td>{{ pt.findings || "—" }}</td>
                        <td class="row-actions">
                          <template v-if="auth.canWrite">
                            <button class="sm" @click="openEditProofTest(pt)">改</button>
                            <button class="danger sm" @click="onDeleteProofTest(pt.id)">删</button>
                          </template>
                        </td>
                      </tr>
                    </tbody>
                  </table>
                  <div v-else class="muted">还没有检验记录。定期检验是 IEC 61511-1 §16.3 的硬性要求。</div>
                </div>
              </div>
            </td>
          </tr>
        </template>
      </tbody>
    </table>
    <div v-else class="empty">
      还没 SIF 记录。请新建一条 SIF（如 SIF-201），再从仪表台账里挑真实位号绑上去 —— 这样生成的台账才能直接出审计资料。
    </div>

    <!-- 修改历史抽屉（M2.4） -->
    <EntityHistoryDrawer entity-type="sif" />

    <!-- 新建 / 编辑 SIF 弹窗 -->
    <div v-if="creating" class="modal-mask" @click.self="closeEditor">
      <div class="modal">
        <div class="modal-header">
          <span>{{ editing ? "编辑 SIF · EDIT" : "新建 SIF · NEW" }}</span>
          <span class="ref">{{ editing ? `SIF-${editing}` : "FORM-SIF-01" }}</span>
        </div>
        <div class="modal-body">
          <div class="form">
            <label class="full">项目 · PROJECT
              <!-- 项目锁定模式：归属恒为当前项目，不允许改 -->
              <select v-model="form.projectId" :disabled="locked">
                <option v-for="p in store.projects" :key="p.id" :value="p.id">{{ p.code }} {{ p.name }}</option>
              </select>
            </label>
            <label>SIF 编号 · CODE <input v-model="form.code" placeholder="SIF-201" /></label>
            <label>SIF 名称 · NAME <input v-model="form.name" placeholder="反应器超压联锁" /></label>
            <label>设计 SIL · SIL DESIGN
              <select v-model="form.silDesign">
                <option v-for="s in ['NA','A','B','C','D']" :key="s" :value="s">SIL {{ s }}</option>
              </select>
            </label>
            <label>验证 SIL · SIL VERIFIED
              <select v-model="form.silVerified">
                <option v-for="s in ['NA','A','B','C','D']" :key="s" :value="s">SIL {{ s }}</option>
              </select>
            </label>
            <label>需求模式 · DEMAND
              <select v-model="form.demandMode">
                <option value="low">低需求 LOW</option>
                <option value="high">高需求 HIGH</option>
              </select>
            </label>
            <label>生命周期 · LIFECYCLE
              <select v-model="form.lifecyclePhase">
                <option value="design">设计</option>
                <option value="construction">建造</option>
                <option value="commissioning">调试</option>
                <option value="operation">运行</option>
                <option value="closed">停用</option>
              </select>
            </label>
            <label>PFDavg 目标 · PFD <input type="number" step="1e-7" v-model.number="form.pfdavgTarget" /></label>
            <label>检验周期(月) · TI <input type="number" v-model.number="form.proofInterval" /></label>
            <label>修复时间(h) · MTTR
              <input type="number" step="0.5" min="0" v-model.number="form.mttrHours" />
            </label>
            <label>共因因子 · β
              <input type="number" step="0.01" min="0" max="1" v-model.number="form.betaFactor" />
            </label>
            <label>检测架构 · SENSOR
              <select v-model="form.sensorArch">
                <option v-for="a in ARCHES" :key="a" :value="a">{{ a }}</option>
              </select>
            </label>
            <label>逻辑架构 · LOGIC
              <select v-model="form.logicArch">
                <option v-for="a in ARCHES" :key="a" :value="a">{{ a }}</option>
              </select>
            </label>
            <label>最终架构 · FINAL
              <select v-model="form.finalArch">
                <option v-for="a in ARCHES" :key="a" :value="a">{{ a }}</option>
              </select>
            </label>
            <label class="full">说明 · NOTE <textarea v-model="form.description" rows="3"></textarea></label>
            <div class="full" style="margin-top:6px;border-top:1px dashed #3a4256;padding-top:6px">
              <div class="ref" style="margin-bottom:4px">SRS 安全需求规格 · IEC 61511-1 §10/§12</div>
            </div>
            <label>项目/装置 · PLANT <input v-model="form.plant" placeholder="如 200#催化裂化" /></label>
            <label>工艺单元 · UNIT <input v-model="form.unit" placeholder="如 反应-再生" /></label>
            <label>关联设备 · EQUIP <input v-model="form.equip" placeholder="如 R-201" /></label>
            <label>响应时间 · RESPONSE <input v-model="form.responseTime" placeholder="如 30 s" /></label>
            <label>设计依据标准 · STD <input v-model="form.designStandard" placeholder="IEC 61511 / GB/T 21109" /></label>
            <label class="full">复位要求 · RESET <input v-model="form.resetReq" placeholder="如 手动复位 / 自动复位" /></label>
            <label class="full">旁路管理 · BYPASS <input v-model="form.bypassReq" placeholder="如 限时旁路 ≤8h + 报警" /></label>
            <label class="full">安全状态 · SAFE STATE <textarea v-model="form.safeState" rows="2" placeholder="如 切断进料、泄压至火炬"></textarea></label>
          </div>
        </div>
        <div class="modal-footer">
          <span class="ref">IEC 61511-1 §11.5</span>
          <button @click="closeEditor">取消</button>
          <button class="primary" :disabled="!form.code || !form.name || !form.projectId" @click="saveNow">保存</button>
        </div>
      </div>
    </div>

    <!-- 关联仪表弹窗 -->
    <div v-if="linking" class="modal-mask" @click.self="linking = null">
      <div class="modal">
        <div class="modal-header">
          <span>关联仪表 · LINK</span>
          <span class="ref">SIF-{{ linking.sifId }}</span>
        </div>
        <div class="modal-body">
          <div class="form">
            <label class="full">仪表位号 · INSTRUMENT
              <select v-model="linkForm.instrumentId">
                <option value="">—</option>
                <option v-for="i in store.instruments" :key="i.id" :value="i.id">
                  {{ i.tag }}（{{ roleCN(i.role) }} · SIL {{ i.silTarget }}）
                </option>
              </select>
            </label>
            <label>角色 · ROLE
              <select v-model="linkForm.role">
                <option value="detector">检测 DETECTOR</option>
                <option value="final">最终 FINAL</option>
                <option value="logic">逻辑 LOGIC</option>
                <option value="aux">旁路 AUX</option>
              </select>
            </label>
            <label>端口 · PORT <input type="number" v-model.number="linkForm.portIndex" :min="1" /></label>
            <label class="full">备注 · NOTE <input v-model="linkForm.note" /></label>
          </div>
        </div>
        <div class="modal-footer">
          <span class="ref">FORM-LINK-01</span>
          <button @click="linking = null">取消</button>
          <button class="primary" :disabled="!linkForm.instrumentId" @click="linkNow">关联</button>
        </div>
      </div>
    </div>

    <!-- 检验测试弹窗（IEC 61511-1 §16.3） -->
    <div v-if="ptEditing !== null" class="modal-mask" @click.self="closeProofTest">
      <div class="modal">
        <div class="modal-header">
          <span>{{ ptEditing === "new" ? "登记检验测试 · PROOF TEST" : "编辑检验记录 · EDIT" }}</span>
          <span class="ref">IEC 61511-1 §16.3</span>
        </div>
        <div class="modal-body">
          <div class="form">
            <label>测试日期 · TESTED AT
              <input type="date" v-model="ptDraft.testedAt" />
            </label>
            <label>结果 · RESULT
              <select v-model="ptDraft.result">
                <option value="pass">通过 PASS</option>
                <option value="fail">失败 FAIL</option>
                <option value="conditional">有条件通过 CONDITIONAL</option>
              </select>
            </label>
            <label>测试人 · TESTED BY
              <input v-model="ptDraft.testedBy" placeholder="工程师姓名 / 工号" />
            </label>
            <label class="full">检验规程 · SOP（IEC 61511-1 §16.2.2）
              <select v-model="ptDraft.sopId">
                <option :value="null">— 无成文规程（不合规）—</option>
                <option v-for="sop in store.proofTestSops" :key="sop.id" :value="sop.id">
                  {{ sop.code }} {{ sop.version }} · {{ sop.title }}
                </option>
              </select>
            </label>
            <label class="full">发现 · FINDINGS
              <textarea v-model="ptDraft.findings" rows="2" placeholder="实测值、偏差、异常等"></textarea>
            </label>
            <label class="full">备注 · NOTES
              <textarea v-model="ptDraft.notes" rows="2" placeholder="校准证书编号、下次计划等"></textarea>
            </label>
          </div>
        </div>
        <div class="modal-footer">
          <span class="ref">下次到期自动按 SIF 检验周期推算</span>
          <button @click="closeProofTest">取消</button>
          <button class="primary" :disabled="!ptDraft.testedAt || !ptDraft.testedBy?.trim()" @click="saveProofTest">保存</button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { ChevronRight, ChevronDown } from "lucide-vue-next";
import { useStudioStore, type SifLink, type SifSummary, type ProofTest, type ProofTestInput } from "../stores/studio";
import { useAuthStore } from "../stores/auth";
import BypassCard from "./BypassCard.vue";
import EntityHistoryDrawer from "./EntityHistoryDrawer.vue";

/**
 * SifPanel — SIF 汇总双模式面板
 * - 无 projectId prop：全局总览模式（项目筛选下拉）
 * - 有 projectId：项目锁定模式（隐藏筛选，新建/编辑归属恒为该项目）
 */
const props = defineProps<{ projectId?: number }>();

const store = useStudioStore();
const auth = useAuthStore();
const locked = computed(() => props.projectId != null);

onMounted(async () => {
  // SIF 汇总可能由「新建联锁图」自动生成，进 Tab 时必须重新拉取，
  // 不能只依赖登录时 bootstrap 的缓存
  await Promise.all([
    store.refreshSifSummary(),
    // 旁路台账预先拉（详情展开时 BypassCard 直接读 store）
    store.bypasses.length === 0 ? store.refreshBypasses("all") : Promise.resolve(),
  ]);
});

const query = ref("");
const silFilter = ref("");
const projectFilter = ref("");

const filtered = computed(() => {
  const q = query.value.trim().toLowerCase();
  return store.sifSummary.filter((s) => {
    if (silFilter.value && s.silVerified !== silFilter.value) return false;
    if (locked.value) {
      if (s.projectId !== props.projectId) return false;
    } else if (projectFilter.value && String(s.projectId) !== projectFilter.value) {
      return false;
    }
    if (!q) return true;
    return s.code.toLowerCase().includes(q) || s.name.toLowerCase().includes(q);
  });
});

const totalDetector = computed(() => store.sifSummary.reduce((a, s) => a + s.detectorCount, 0));
const totalFinal = computed(() => store.sifSummary.reduce((a, s) => a + s.finalCount, 0));
const totalAux = computed(() => store.sifSummary.reduce((a, s) => a + s.auxCount, 0));

const ROLE_CN: Record<string, string> = {
  detector: "检测",
  final: "最终",
  logic: "逻辑",
  aux: "旁路",
};
function roleCN(r: string) {
  return ROLE_CN[r] ?? r;
}

/** silVerified vs silAchieved 双源一致性状态中文标签 */
const VERIFY_CN: Record<string, string> = {
  pending: "待数据",
  unverified: "未验证",
  verified: "已验证",
  overclaimed: "超标",
  downgraded: "降级",
};
function verifyCN(s: string) {
  return VERIFY_CN[s] ?? s;
}

const PHASE_CN: Record<string, string> = {
  design: "设计",
  construction: "建造",
  commissioning: "调试",
  operation: "运行",
  closed: "停用",
};
function phaseCN(s: string) {
  return PHASE_CN[s] ?? s;
}

// Route 1H — 架构 → 有效 HFT（与后端 hft_from_arch 一致）
function hftOf(arch: string) {
  return { "1oo1": 0, "2oo2": 0, "1oo2": 1, "2oo3": 1, "2oo4": 2 }[arch] ?? 0;
}
// Route 1H — 架构要求的通道数（与后端 arch_required_channels 一致）
function needChannels(arch: string) {
  return { "1oo1": 1, "1oo2": 2, "2oo2": 2, "2oo3": 3, "2oo4": 4 }[arch] ?? 1;
}
const MATCH_CN: Record<string, string> = {
  matched: "匹配",
  degraded: "退化",
  empty: "无通道",
};
function matchCN(s: string) {
  return MATCH_CN[s] ?? s;
}

const expanded = ref<number | null>(null);
const sifLinks = ref<SifLink[]>([]);
async function toggle(id: number) {
  if (expanded.value === id) {
    expanded.value = null;
    sifLinks.value = [];
    return;
  }
  expanded.value = id;
  sifLinks.value = await store.listSifLinks(id);
  // 展开时同步拉取该 SIF 的检验测试记录（IEC 61511-1 §16.3）
  await store.refreshProofTests(id);
}

async function unlink(linkId: number, sifId: number) {
  if (!confirm("确认解除该仪表与 SIF 的关联？")) return;
  await store.unlinkInstrument(linkId);
  sifLinks.value = await store.listSifLinks(sifId);
}

const creating = ref<boolean | null>(false);
const editing = ref<number | null>(null);
const ARCHES = ["1oo1", "1oo2", "2oo2", "2oo3", "2oo4"];
const form = reactive({
  projectId: 0,
  code: "",
  name: "",
  description: "",
  silDesign: "NA",
  silVerified: "NA",
  demandMode: "low",
  pfdavgTarget: null as number | null,
  proofInterval: 12,
  sensorArch: "1oo1",
  logicArch: "1oo1",
  finalArch: "1oo1",
  mttrHours: 8,
  betaFactor: 0.1,
  plant: "",
  unit: "",
  equip: "",
  responseTime: "",
  safeState: "",
  resetReq: "",
  bypassReq: "",
  designStandard: "",
  lifecyclePhase: "design",
});
function openCreate() {
  form.projectId = props.projectId ?? store.projects[0]?.id ?? 0;
  form.code = "";
  form.name = "";
  form.description = "";
  form.silDesign = "NA";
  form.silVerified = "NA";
  form.demandMode = "low";
  form.pfdavgTarget = null;
  form.proofInterval = 12;
  form.sensorArch = "1oo1";
  form.logicArch = "1oo1";
  form.finalArch = "1oo1";
  form.mttrHours = 8;
  form.betaFactor = 0.1;
  form.plant = "";
  form.unit = "";
  form.equip = "";
  form.responseTime = "";
  form.safeState = "";
  form.resetReq = "";
  form.bypassReq = "";
  form.designStandard = "";
  form.lifecyclePhase = "design";
  editing.value = null;
  creating.value = true;
}
async function openEdit(s: SifSummary) {
  form.projectId = s.projectId;
  form.code = s.code;
  form.name = s.name;
  form.description = s.description || "";
  form.silDesign = s.silDesign;
  form.silVerified = s.silVerified;
  form.demandMode = s.demandMode;
  form.pfdavgTarget = s.pfdavgTarget;
  form.proofInterval = s.proofInterval;
  form.sensorArch = s.sensorArch || "1oo1";
  form.logicArch = s.logicArch || "1oo1";
  form.finalArch = s.finalArch || "1oo1";
  form.mttrHours = s.mttrHours ?? 8;
  form.betaFactor = s.betaFactor ?? 0.1;
  form.plant = s.plant || "";
  form.unit = s.unit || "";
  form.equip = s.equip || "";
  form.responseTime = s.responseTime || "";
  form.safeState = s.safeState || "";
  form.resetReq = s.resetReq || "";
  form.bypassReq = s.bypassReq || "";
  form.designStandard = s.designStandard || "";
  form.lifecyclePhase = s.lifecyclePhase || "design";
  editing.value = s.id;
  creating.value = true;
}
function closeEditor() {
  creating.value = null;
  editing.value = null;
}
function openHistory(id: number) {
  store.openEntityHistory("sif", id);
}
async function saveNow() {
  if (!form.code || !form.name) return;
  if (!form.projectId) {
    await store.ensureDefaultProject();
  }
  const payload = {
    projectId: form.projectId,
    code: form.code,
    name: form.name,
    description: form.description,
    silDesign: form.silDesign,
    silVerified: form.silVerified,
    demandMode: form.demandMode,
    pfdavgTarget: form.pfdavgTarget,
    proofInterval: form.proofInterval,
    sensorArch: form.sensorArch,
    logicArch: form.logicArch,
    finalArch: form.finalArch,
    mttrHours: form.mttrHours,
    betaFactor: form.betaFactor,
    plant: form.plant,
    unit: form.unit,
    equip: form.equip,
    responseTime: form.responseTime,
    safeState: form.safeState,
    resetReq: form.resetReq,
    bypassReq: form.bypassReq,
    designStandard: form.designStandard,
    lifecyclePhase: form.lifecyclePhase,
  };
  if (editing.value !== null) {
    await store.updateSif(editing.value, payload);
  } else {
    await store.createSif({
      ...payload,
      projectId:
        form.projectId || (await store.ensureDefaultProject()).id,
    });
  }
  creating.value = false;
  editing.value = null;
}

interface Linking {
  sifId: number | null;
}
const linking = ref<Linking | null>(null);
const linkForm = reactive({
  instrumentId: 0,
  role: "detector",
  portIndex: 1,
  note: "",
});
function openLink(sifId: number) {
  linkForm.instrumentId = 0;
  linkForm.role = "detector";
  linkForm.portIndex = 1;
  linkForm.note = "";
  linking.value = { sifId };
}
async function linkNow() {
  if (!linking.value || !linkForm.instrumentId) return;
  await store.linkInstrument({
    sifId: linking.value.sifId!,
    instrumentId: linkForm.instrumentId,
    role: linkForm.role,
    portIndex: linkForm.portIndex,
    note: linkForm.note,
  });
  if (expanded.value) {
    sifLinks.value = await store.listSifLinks(expanded.value);
  }
  linking.value = null;
}

// ============================================================================
// 检验测试（IEC 61511-1 §16.3）
// ============================================================================
/** 按当前 SIF 过滤检验记录 */
function proofTestsFor(sifId: number): ProofTest[] {
  return store.proofTests.filter((pt) => pt.sifId === sifId);
}

/** 检验弹窗状态：null=关闭；number=编辑某 ID；"new"=新建 */
const ptEditing = ref<number | "new" | null>(null);
const ptDraft = reactive<ProofTestInput>({
  sifId: 0,
  testedAt: "",
  result: "pass",
  testedBy: "",
  findings: "",
  notes: "",
  sopId: null,
});

/** 今日日期 YYYY-MM-DD（默认填充） */
function today(): string {
  return new Date().toISOString().slice(0, 10);
}

function openProofTest(sifId: number) {
  ptDraft.sifId = sifId;
  ptDraft.testedAt = today();
  ptDraft.result = "pass";
  ptDraft.testedBy = "";
  ptDraft.findings = "";
  ptDraft.notes = "";
  ptDraft.sopId = null;
  // 确保规程下拉有数据（§16.2.2）
  if (store.proofTestSops.length === 0) void store.refreshProofTestSops();
  ptEditing.value = "new";
}

function openEditProofTest(pt: ProofTest) {
  ptDraft.sifId = pt.sifId;
  ptDraft.testedAt = pt.testedAt;
  ptDraft.result = pt.result;
  ptDraft.testedBy = pt.testedBy;
  ptDraft.findings = pt.findings;
  ptDraft.notes = pt.notes;
  ptDraft.sopId = pt.sopId;
  if (store.proofTestSops.length === 0) void store.refreshProofTestSops();
  ptEditing.value = pt.id;
}

function closeProofTest() {
  ptEditing.value = null;
}

async function saveProofTest() {
  if (!ptDraft.testedAt || !ptDraft.testedBy?.trim()) return;
  if (ptEditing.value === "new") {
    await store.createProofTest({ ...ptDraft });
  } else if (typeof ptEditing.value === "number") {
    await store.updateProofTest(ptEditing.value, { ...ptDraft });
  }
  // 刷新当前展开 SIF 的检验列表 + 横幅
  if (expanded.value) {
    await store.refreshProofTests(expanded.value);
  }
  ptEditing.value = null;
}

async function onDeleteProofTest(id: number) {
  if (!confirm("确认删除该检验测试记录？")) return;
  await store.deleteProofTest(id);
  if (expanded.value) {
    await store.refreshProofTests(expanded.value);
  }
}
</script>

<style scoped>
.sif { display: flex; flex-direction: column; gap: var(--s-3); }
.stats { display: grid; grid-template-columns: repeat(4, 1fr); gap: var(--s-3); }
.head { display: flex; gap: var(--s-2); align-items: center; }
.search { flex: 1; }

tr.active td { background: var(--acc-bg); }
tr.active td:first-child { border-left: var(--rule-mid) solid var(--acc); }

.detail-row td {
  background: var(--paper-2);
  padding: var(--s-4) var(--s-5);
  border-top: var(--rule-fine) solid var(--rule-2);
}
.detail {
  display: grid;
  grid-template-columns: 1fr 1fr 1fr 1fr;
  gap: var(--s-3) var(--s-4);
}
.detail-block h4 {
  font-family: var(--font-mono);
  font-size: 10px;
  color: var(--ink-3);
  letter-spacing: 0.12em;
  margin: 0 0 var(--s-2);
  padding-bottom: var(--s-1);
  border-bottom: var(--rule-fine) solid var(--rule-2);
}
.detail-block.full { grid-column: 1 / -1; }
.block-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: var(--s-2);
}
.block-head h4 { margin: 0; padding: 0; border: 0; }
.block-link {
  font-family: var(--font-mono);
  font-size: 10px;
  color: var(--acc);
  text-decoration: none;
  letter-spacing: 0.04em;
}
.block-link:hover { text-decoration: underline; }

.chips { display: flex; flex-wrap: wrap; gap: var(--s-1); align-items: center; }
.chip-loc {
  display: inline-block;
  padding: 2px 8px;
  font-family: var(--font-mono);
  font-size: var(--fs-xs);
  background: var(--paper);
  color: var(--ink-1);
  border: var(--rule-fine) solid var(--rule);
  font-weight: 500;
}

table.links {
  border: var(--rule-fine) solid var(--rule);
  margin-top: var(--s-2);
}
table.links thead th { background: var(--paper-3); }

.row-actions { display: flex; gap: var(--s-1); justify-content: flex-end; }

/* 检验测试子表 */
table.sub-table {
  border: var(--rule-fine) solid var(--rule);
  margin-top: var(--s-2);
}
table.sub-table thead th { background: var(--paper-3); }
table.sub-table td { font-size: var(--fs-sm); }

/* Route 1H 硬件安全完整性子表 */
table.r1h {
  border: var(--rule-fine) solid var(--rule);
  margin-top: var(--s-2);
}
table.r1h thead th { background: var(--paper-3); }
table.r1h td { font-size: var(--fs-sm); }

/* 检验结果标签 */
.tag.pt-pass { background: var(--ok-bg, #e8f5e9); color: var(--ok, #2e7d32); }
.tag.pt-fail { background: var(--danger-bg, #fdecea); color: var(--danger, #c62828); }
.tag.pt-conditional { background: var(--warn-bg, #fff8e1); color: var(--warn, #f57f17); }

/* 检验状态标签 */
.tag.pts-overdue { background: var(--danger-bg, #fdecea); color: var(--danger, #c62828); }
.tag.pts-current { background: var(--ok-bg, #e8f5e9); color: var(--ok, #2e7d32); }

/* silVerified/silAchieved 双源验证状态 */
.tag.vs { font-size: 10px; margin-left: 4px; }
.tag.vs-verified { background: var(--ok-bg, #e8f5e9); color: var(--ok, #2e7d32); }
.tag.vs-overclaimed { background: var(--danger-bg, #fdecea); color: var(--danger, #c62828); }
.tag.vs-downgraded { background: var(--warn-bg, #fff8e1); color: var(--warn, #f57f17); }
.tag.vs-unverified,
.tag.vs-pending { background: var(--paper-3, #eceff1); color: var(--ink-3, #607d8b); }
</style>
