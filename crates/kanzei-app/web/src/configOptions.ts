import type { ModelCatalogItem } from "./ipc/types";

export const MODEL_ROLE_IDS = new Set(["primary", "fast", "compact"]);
export const MANUAL_MODEL_PATTERN = /^[\w.-]+:.+$/;

export interface SelectOption {
  value: string;
  label: string;
  hint?: string;
}

export const REASONING_OPTIONS: ReadonlyArray<SelectOption> = [
  { value: "off", label: "off（服务商默认）" },
  { value: "none", label: "none（明确关闭思考）" },
  { value: "low", label: "low（低）" },
  { value: "medium", label: "medium（中）" },
  { value: "high", label: "high（高）" },
  { value: "xhigh", label: "xhigh（极高）" },
  { value: "max", label: "max（最高）" },
];

export const COMPOSER_REASONING_LABELS: Record<string, string> = {
  off: "默认/省略",
  none: "关闭",
  low: "低",
  medium: "中",
  high: "高",
  xhigh: "极高",
  max: "最高",
};

export function reasoningShortLabel(value?: string | null): string {
  if (!value) return "默认";
  return COMPOSER_REASONING_LABELS[value] ?? value;
}

export const ALL_PROFILE_OPTIONS: ReadonlyArray<SelectOption> = [
  { value: "dev-auto", label: "自主开发 (dev-auto)" },
  { value: "dev-pair", label: "结伴开发 (dev-pair)" },
  { value: "readonly", label: "只读讨论 (readonly)" },
  { value: "isolated", label: "隔离沙箱 (isolated)" },
  { value: "research", label: "研究分析 (research)" },
];

export const PROJECT_COMPOSER_PROFILES: ReadonlyArray<SelectOption> = [
  { value: "dev-auto", label: "自主开发" },
  { value: "dev-pair", label: "结伴开发" },
  { value: "readonly", label: "只读讨论" },
  { value: "isolated", label: "隔离沙箱" },
];

export const GENERAL_COMPOSER_PROFILES: ReadonlyArray<SelectOption> = [
  { value: "dev-auto", label: "自主开发" },
  { value: "dev-pair", label: "结伴开发" },
  { value: "isolated", label: "隔离沙箱" },
];

export function profileDisplayLabel(profile?: string | null): string {
  switch (profile) {
    case "dev-auto":
      return "自主开发";
    case "dev-pair":
      return "结伴开发";
    case "readonly":
      return "只读讨论";
    case "isolated":
      return "隔离沙箱";
    case "research":
      return "研究分析";
    default:
      return profile || "自主开发";
  }
}

export const SUBAGENT_MODE_OPTIONS: ReadonlyArray<SelectOption> = [
  { value: "auto", label: "子代理: 按需" },
  { value: "ultra", label: "子代理: 积极" },
  { value: "off", label: "子代理: 关" },
];

export const CADENCE_FULL_TEST_OPTIONS: ReadonlyArray<SelectOption> = [
  { value: "", label: "内置默认（条目关闭前）" },
  { value: "entry_close", label: "条目关闭前 (entry_close)" },
  { value: "every_commit", label: "每次提交前 (every_commit)" },
  { value: "every_n_batches", label: "每 N 批全量一次 (every_n_batches)" },
  { value: "release_only", label: "仅发布前 (release_only)" },
];

export const CADENCE_TARGETED_TEST_OPTIONS: ReadonlyArray<SelectOption> = [
  { value: "", label: "内置默认（每次提交前）" },
  { value: "every_commit", label: "每次提交前 (every_commit)" },
  { value: "off", label: "按需决定 (off)" },
];

export const CADENCE_COMMIT_OPTIONS: ReadonlyArray<SelectOption> = [
  { value: "", label: "内置默认（每批提交）" },
  { value: "per_batch", label: "每批提交 (per_batch)" },
  { value: "per_entry", label: "每项提交 (per_entry)" },
];

export const CADENCE_PUSH_OPTIONS: ReadonlyArray<SelectOption> = [
  { value: "", label: "内置默认（条目完成后）" },
  { value: "per_entry", label: "条目完成后推送 (per_entry)" },
  { value: "per_commit", label: "每次提交后推送 (per_commit)" },
  { value: "periodic", label: "按约定定期推送 (periodic)" },
];

export function sourceDisplayLabel(source?: string | null): string {
  switch (source) {
    case "line":
      return "本线临时";
    case "project":
      return "项目覆盖";
    case "global":
      return "全局设置";
    case "builtin":
      return "内置默认";
    case "agent":
      return "Agent 指定";
    default:
      return source ?? "";
  }
}

export function filterDirectModels(catalog: ModelCatalogItem[]): ModelCatalogItem[] {
  const seen = new Set<string>();
  const out: ModelCatalogItem[] = [];
  for (const item of catalog) {
    if (!item?.id || MODEL_ROLE_IDS.has(item.id) || seen.has(item.id)) continue;
    seen.add(item.id);
    out.push(item);
  }
  return out;
}

export function withFallbackOption(
  options: ReadonlyArray<SelectOption>,
  currentValue: string | null | undefined,
  fallbackLabel?: (value: string) => string
): SelectOption[] {
  const list = [...options];
  const val = currentValue ?? "";
  if (val && !list.some((o) => o.value === val)) {
    list.push({ value: val, label: fallbackLabel ? fallbackLabel(val) : val });
  }
  return list;
}
