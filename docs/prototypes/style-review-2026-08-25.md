# SuperTask 原型 H · 样式问题评审（2026-08-25）

> 对象：`docs/prototypes/prototype-h-linear.html`
> 触发：用户反馈「日志和环境有问题 / 健康页面很重复 / 4×3 布局回到旧版」+ `/Web Design` 评审请求
> 方法：完整源码审查（浏览器渲染本次环境卡死，改用 CSS 精确定位）

## 严重程度一览

| # | 区域 | 严重度 | 现象 | 根因（行） |
|---|------|--------|------|-----------|
| 0 | **Tab 切换** | 🔴 **高** | **健康面板永远盖在所有 Tab 上，点哪个都看健康** | **`.p-health{display:flex}` 覆盖 `.panel{display:none}`，且该面板无对应的 `:checked` 切换规则** |
| 1 | 日志 | 🔴 高 | 终端下方大片白空，不撑满 | `.p-logs` 激活为 `display:block`，`.term{flex:1}` 失效 |
| 2 | 环境 | 🔴 高 | 面板右侧约 500px 死白 | `.p-env{max-width:38rem}` 作用在 `position:absolute;inset:0` 面板 |
| 3 | 环境 | 🔴 高 | notice 与端口字段等紧贴无间距 | `.p-env` 为 block，`gap:1rem` 失效 |
| 4 | 健康 | 🟡 中 | 与上方 facts 网格重复 | 健康检查 URL、间隔/超时 两处都出现 |
| 5 | 健康 | 🟡 中 | “最近结果”与结果表首行重复 | 左参数行 vs 结果表第一行 |
| 6 | 健康 | 🟡 中 | 结果表 URL 列 12 行全重复 | 每行都是 `GET /actuator/health` |
| 7 | 一致性 | 🟡 中 | 端口数据不自洽 | facts/health 显示 :8080，env 面板显示 8081 |
| 8 | facts | 🟡 中 | 注释“4×3”与实际 3 列不符 | 注释 L290 vs `repeat(3,…)` L291；密集 12 项与三 Tab 重叠 |
| 9 | 代码 | 🟢 低 | `.head-actions` 重复定义 | L167 与 L171 冲突（margin-left 0 → auto） |
| 10 | 代码 | 🟢 低 | `.copy svg` 选择器重复列出 | L56 列表里出现两次 |
| 11 | 代码 | 🟢 低 | `.panel.is-flex` 定义从未使用 | L346 死代码 |

---

## 1. 日志面板：终端不撑满（🔴）

`.panel`（L342）激活态规则（L347-351）把 `.p-logs` 设为 `display:block`。
`.term`（L367）写了 `flex:1`，但父级是 block 上下文，`flex` 完全失效；终端高度只取到 `min-height:13rem`。
面板本身靠 `position:absolute;inset:0` 撑满 `.panels`，于是**终端下方留下一大片白空**（正是 H9 之前“底部只占一半”的同类问题，当时只修了健康面板，漏了日志）。

修复方向：
```css
/* 让日志面板成为纵向 flex 容器 */
#dt-logs:checked ~ .shell .panel.p-logs { display: flex; flex-direction: column; }
.panel.p-logs .term { flex: 1; min-height: 0; }   /* 真正撑满，内部滚动 */
```

## 2. 环境面板：右侧死白 + 子元素贴死（🔴🔴）

`.p-env`（L385）同时有 `max-width:38rem` 和来自 `.panel` 的 `position:absolute;inset:0`。
`inset:0` 已把面板拉满容器宽度，再加 `max-width:38rem` → 面板盒子被压成 38rem 宽、左对齐，
右侧约 500px 全是空白（窗口越宽越明显）。

同时 `.p-env` 是 block，`gap:1rem` 对 block 无效 → `.notice`、`.field`×2、`.env-actions` **紧贴在一起**，没有呼吸间距。

修复方向：
```css
.p-env { max-width: none; display: flex; flex-direction: column; gap: 1rem; }
/* 若想限制表单宽度，用内层 wrapper 限宽并居中，而不是限死面板本身 */
```

## 3. 健康面板：信息冗余（🟡）

健康面板是“健康检查”这个 fact 的下钻，但当前它把 facts 网格里已有的内容又显示了一遍：

- **与 facts 网格重复**：facts 已有「健康检查 GET /actuator/health」「间隔/超时 2s/2s」，
  健康左栏又列了「URL」「间隔/超时」（L822/L826 vs L967-969）。
- **“最近结果”重复**：左栏 `最近结果 200·46ms·16:40:49`（L971）与结果表首行（L999）是同一信息。
- **结果表 URL 列 12 行全重复**：每行 `.url` 都是 `GET /actuator/health`（L999-1010），
  只有耗时在变，中间列是纯重复文字。

修复方向：
- 健康左栏只保留“下钻专属”的动态信息（最近一次状态、失败原因、判断规则），
  删掉与 facts 重复的 静态 URL/间隔。
- 结果表改为 `时间 | 状态 | 耗时`（或加迷你柱），**去掉重复的 URL 列**。
- 删掉左栏“最近结果”整行（表格首行即最新）。

## 4. 端口数据不自洽（🟡）

选中服务是 `gateway`，facts 与 health 都写 `:8080`，但环境面板端口输入框是 `8081`、
health-preview 也是 `8081`（L819/L968 vs L924/L935）。
叙事上这是“已改端口待保存”的草稿态，但三个面板数值不一致，第一眼像 bug。
建议：env 面板明确标注“草稿值，未保存”，或在 facts 同步显示“8080 → 8081(草稿)”。

## 5. 4×3 布局“回到旧版”（🟡）

- 注释（L290）写「4×3 紧凑属性网格」，实际 CSS（L291）是 `repeat(3, minmax(0,1fr))` =
  **3 列**（12 项 → 4 行）。注释与代码不符。
- 更关键：这 12 项里「健康检查 URL / 间隔·超时 / 端口」分别与健康、环境 Tab 重叠，
  加上上方 facts 已铺满，整页读起来像“把所有信息一次性堆在网格里”的旧版思路，
  与“facts 概要 + Tab 下钻”的分层架构相违背。

修复方向（二选一，需你拍板）：
- **A. 瘦身 facts**：只留 6 项概要（端口 / 类型·运行栈 / 拓扑 / PID树 / 日志路径 / 使用时长），
  健康检查、间隔、grace 等交给对应 Tab。与“分层”架构一致。
- **B. 真做 4 列 × 3 行**：若你记忆里的“新版”是 4 列，则把 `repeat(3,…)` 改成 `repeat(4,…)`，
  并把 12 项对齐成 4×3。需确认窗口宽度能容纳 4 列不拥挤。

---

## 顺手可清的代码异味（🟢）

- `.head-actions`（L167、L171）重复定义，后一条把 `margin-left` 从 0 改成 auto，前一条等于死代码。
- `.copy svg`（L56）在选择器列表里出现两次。
- `.panel.is-flex`（L346）定义后从未使用。

---

## v2 验收（2026-08-25 已修复）

修复方案：

- **#0 Tab 切换**：移除 `.p-health { display:flex }`，让 `.panel{display:none}` 正常隐藏；新增
  `#dt-health:checked ~ .shell .panel.p-health { display:flex; flex-direction:column }` 切换规则。
  顺带为 logs / env 加上同样的 flex 切换规则，确保 block 上下文里 `flex:1` 真正生效。
- **#1 日志撑满**：`.panel.p-logs` 激活态改 `display:flex; flex-direction:column`，`.term{flex:1; min-height:0}`。
- **#2 #3 环境去死白 + 间距**：去掉 `max-width:38rem`，激活态改 `display:flex; flex-direction:column; gap:1rem`。
- **#4 #5 #6 健康去重**：删除"最近结果"行；结果表列改成 `7rem 5rem 1fr`（时间/状态/耗时），
  移除 12 行重复的 `GET /actuator/health` URL 列。
- **#7 端口自洽**：env 端口输入、`SERVER_PORT` 行、health-preview 统一为 `:8080`；
  端口状态改为"运行中 / HTTP 200 · 46ms"；notice 改为中性提示（"保存会写回 yaml 的 port 与 SERVER_PORT…"）。
- **#8 facts 去网格**：4×3 网格（12 项）替换为轻量属性条（6 项），用细分割线连接，概览项不与 Tab 重叠。
- **#9–11 死代码**：删除 L167 重复 head-actions、`.copy svg` 重复选择器、`.panel.is-flex`。

验收（1440×900 真实 Chromium）：

- v2-logs.png：终端填满面板，无白空。
- v2-env.png：notice→port field→env field→actions 间有正确呼吸；端口 8080 + 运行中状态自洽。
- v2-health.png：左参数无重复；右表 3 列无 URL 重复。
- v2-config.png：YAML 高亮渲染，port 8080 与其它面板一致。
- 四个 Tab 切换正常（修复前怎么点都是健康面板）。

---

## 建议优先级

1. 先修 🔴 三个：日志撑满、环境取消 max-width + 改为 flex 间距 —— 直接解决“日志和环境有问题”。
2. 再修 🟡 冗余：健康左栏去重、结果表去 URL 列、端口三者自洽。
3. 最后定 facts 走向（A 瘦身 / B 真 4 列）并校正注释。
4. 顺手删 🟢 死代码。
