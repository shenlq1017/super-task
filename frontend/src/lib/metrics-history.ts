import { useSyncExternalStore } from "react";
import type { HostMetrics } from "../ipc/protocol";
import { pct } from "./metrics";

/**
 * 跨页面存活的整机指标历史（「什么时候开始卡的」）。模块级环形缓冲：
 * 路由切换不丢、应用关闭即清空，**不持久化**（落盘是对「不持久化」约定的
 * 显式扩展，尚未做）。数据仍来自既有 `system.metrics` 轮询——状态栏
 * （~3s）与监控页（1 Hz）两个馈源都调 [`recordHostMetrics`]，按采样时间戳
 * 去重合并，不引入新采样面。
 */

/** 单个采样：CPU 总占用与内存压力（0–100 百分比；差分首采样 cpu 为 null）。 */
export type MetricSample = { at: number; cpu: number | null; mem: number | null };

/** ~1 小时 @1 Hz。有界窗口，超限丢弃最旧样本，不无限增长。 */
const CAPACITY = 3600;
/** 双馈源去重：距上一采样不足 800ms 的丢弃（监控页 1 Hz 与状态栏 3s 交叠时取密者）。 */
const MIN_INTERVAL_MS = 800;

let samples: MetricSample[] = [];
const listeners = new Set<() => void>();

/** 把一次 `system.metrics` 采样计入历史（重复/过密的采样静默丢弃）。 */
export function recordHostMetrics(m: HostMetrics): void {
  const at = m.sampledAtMs > 0 ? m.sampledAtMs : Date.now();
  const last = samples[samples.length - 1];
  if (last && at - last.at < MIN_INTERVAL_MS) return;
  const sample: MetricSample = {
    at,
    cpu: m.cpuPercent,
    mem: pct(m.memoryUsedBytes ?? null, m.memoryTotalBytes ?? null),
  };
  samples = samples.concat(sample).slice(-CAPACITY);
  listeners.forEach((l) => l());
}

/** 订阅历史快照（只读；引用仅在新增采样时更替）。 */
export function useMetricsHistory(): readonly MetricSample[] {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => samples,
  );
}
