import React, { useEffect, useId, useMemo, useRef, useState } from "react";
import type { ModelCatalogItem } from "../ipc/types";
import { filterDirectModels, MANUAL_MODEL_PATTERN } from "../configOptions";
import { IconClose, IconRefresh } from "./Icons";

export interface ModelComboboxProps {
  id?: string;
  value: string;
  onChange: (next: string) => void;
  models: ModelCatalogItem[];
  loading?: boolean;
  disabled?: boolean;
  placeholder?: string;
  allowEmpty?: boolean;
  emptyLabel?: string;
  ariaLabel?: string;
  onOpen?: () => void;
  onRefresh?: () => void;
}

interface FlatOption {
  kind: "empty" | "model" | "custom";
  value: string;
  label: string;
  provider?: string;
}

function splitProvider(id: string): { provider: string; model: string } {
  const idx = id.indexOf(":");
  if (idx <= 0) return { provider: "其他", model: id };
  return { provider: id.slice(0, idx), model: id.slice(idx + 1) };
}

export function ModelCombobox({
  id,
  value,
  onChange,
  models,
  loading = false,
  disabled = false,
  placeholder = "选择或输入 provider:model",
  allowEmpty = false,
  emptyLabel = "留空（跟随默认）",
  ariaLabel,
  onOpen,
  onRefresh,
}: ModelComboboxProps): React.ReactElement {
  const listboxId = useId();
  const rootRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const [open, setOpen] = useState(false);
  // 打开下拉时默认不按当前已选值过滤，避免原生 datalist 只显示已选项的缺陷；
  // 只有用户实际键入筛选词时才启用过滤。
  const [filterActive, setFilterActive] = useState(false);
  const [highlightIndex, setHighlightIndex] = useState(-1);

  const directModels = useMemo(() => filterDirectModels(models), [models]);

  const query = filterActive ? value.trim().toLowerCase() : "";

  const flatOptions = useMemo<FlatOption[]>(() => {
    const items: FlatOption[] = [];
    if (allowEmpty && !query) {
      items.push({ kind: "empty", value: "", label: emptyLabel });
    }
    const matched = directModels.filter((m) => {
      if (!query) return true;
      return m.id.toLowerCase().includes(query) || m.label.toLowerCase().includes(query);
    });
    for (const m of matched) {
      const { provider } = splitProvider(m.id);
      items.push({ kind: "model", value: m.id, label: m.id, provider });
    }
    const trimmed = value.trim();
    if (
      trimmed &&
      !directModels.some((m) => m.id === trimmed) &&
      (!query || trimmed.toLowerCase().includes(query))
    ) {
      items.push({
        kind: "custom",
        value: trimmed,
        label: MANUAL_MODEL_PATTERN.test(trimmed)
          ? `使用手填模型：${trimmed}`
          : `自定义输入：${trimmed}（格式需为 provider:model）`,
      });
    }
    return items;
  }, [allowEmpty, emptyLabel, directModels, query, value]);

  const groupedModels = useMemo(() => {
    const groups = new Map<string, Array<{ opt: FlatOption; flatIdx: number }>>();
    const topItems: Array<{ opt: FlatOption; flatIdx: number }> = [];
    const bottomItems: Array<{ opt: FlatOption; flatIdx: number }> = [];

    flatOptions.forEach((opt, flatIdx) => {
      if (opt.kind === "empty") {
        topItems.push({ opt, flatIdx });
      } else if (opt.kind === "custom") {
        bottomItems.push({ opt, flatIdx });
      } else {
        const key = opt.provider ?? "其他";
        const bucket = groups.get(key) ?? [];
        bucket.push({ opt, flatIdx });
        groups.set(key, bucket);
      }
    });
    return { topItems, groups: Array.from(groups.entries()), bottomItems };
  }, [flatOptions]);

  useEffect(() => {
    if (!open) return;
    const onDocMouseDown = (event: MouseEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) {
        setOpen(false);
        setFilterActive(false);
      }
    };
    document.addEventListener("mousedown", onDocMouseDown);
    return () => document.removeEventListener("mousedown", onDocMouseDown);
  }, [open]);

  const openMenu = (activateFilter = false) => {
    if (disabled) return;
    if (!open) {
      onOpen?.();
    }
    setOpen(true);
    setFilterActive(activateFilter);
    const currentIdx = flatOptions.findIndex((o) => o.value === value.trim());
    setHighlightIndex(currentIdx >= 0 ? currentIdx : 0);
  };

  const selectOption = (nextVal: string) => {
    onChange(nextVal);
    setOpen(false);
    setFilterActive(false);
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (disabled) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      if (!open) {
        openMenu(false);
      } else if (flatOptions.length > 0) {
        setHighlightIndex((prev) => (prev + 1) % flatOptions.length);
      }
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      if (!open) {
        openMenu(false);
      } else if (flatOptions.length > 0) {
        setHighlightIndex((prev) => (prev <= 0 ? flatOptions.length - 1 : prev - 1));
      }
    } else if (e.key === "Enter" && open) {
      if (highlightIndex >= 0 && highlightIndex < flatOptions.length) {
        e.preventDefault();
        selectOption(flatOptions[highlightIndex].value);
      } else {
        setOpen(false);
        setFilterActive(false);
      }
    } else if (e.key === "Escape" && open) {
      e.preventDefault();
      setOpen(false);
      setFilterActive(false);
    }
  };

  return (
    <div ref={rootRef} className={`model-combobox${open ? " open" : ""}`}>
      <div className="model-combobox-control">
        <input
          id={id}
          ref={inputRef}
          type="text"
          role="combobox"
          aria-expanded={open}
          aria-controls={listboxId}
          aria-autocomplete="list"
          aria-label={ariaLabel}
          className="input mono model-combobox-input"
          value={value}
          disabled={disabled}
          placeholder={placeholder}
          autoComplete="off"
          spellCheck={false}
          onFocus={() => openMenu(false)}
          onClick={() => openMenu(false)}
          onChange={(e) => {
            onChange(e.target.value);
            if (!open) onOpen?.();
            setOpen(true);
            setFilterActive(true);
            setHighlightIndex(0);
          }}
          onKeyDown={handleKeyDown}
        />
        <div className="model-combobox-actions">
          {allowEmpty && value && !disabled && (
            <button
              type="button"
              className="model-combobox-btn"
              title="清空恢复默认"
              aria-label="清空"
              tabIndex={-1}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => {
                onChange("");
                setFilterActive(false);
                inputRef.current?.focus();
              }}
            >
              <IconClose size={11} />
            </button>
          )}
          <button
            type="button"
            className="model-combobox-btn"
            title={open ? "收起模型列表" : "展开全部可用模型"}
            aria-label={open ? "收起模型列表" : "展开全部可用模型"}
            disabled={disabled}
            tabIndex={-1}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => {
              if (open) {
                setOpen(false);
                setFilterActive(false);
              } else {
                openMenu(false);
                inputRef.current?.focus();
              }
            }}
          >
            <svg width="11" height="11" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <path d={open ? "M3 7.5 6 4.5 9 7.5" : "M3 4.5 6 7.5 9 4.5"} />
            </svg>
          </button>
        </div>
      </div>

      {open && (
        <div id={listboxId} role="listbox" className="model-combobox-menu">
          <div className="model-combobox-menu-head">
            <span>
              {loading
                ? "正在探测服务商模型…"
                : filterActive && query
                ? `筛选 “${value.trim()}” · ${directModels.length} 个候选`
                : `全部可用模型 (${directModels.length}) · 可直接输入手填`}
            </span>
            {onRefresh && (
              <button
                type="button"
                className="btn btn-ghost btn-sm"
                style={{ height: 20, padding: "0 6px", fontSize: 11 }}
                disabled={loading}
                onMouseDown={(e) => e.preventDefault()}
                onClick={onRefresh}
              >
                <IconRefresh size={11} />
                <span>刷新</span>
              </button>
            )}
          </div>

          <div className="model-combobox-menu-body">
            {groupedModels.topItems.map(({ opt, flatIdx }) => {
              const selected = value.trim() === "";
              return (
                <button
                  key="__empty__"
                  type="button"
                  role="option"
                  aria-selected={selected}
                  className={`model-combobox-option${selected ? " selected" : ""}${
                    highlightIndex === flatIdx ? " highlighted" : ""
                  }`}
                  onMouseDown={(e) => e.preventDefault()}
                  onMouseEnter={() => setHighlightIndex(flatIdx)}
                  onClick={() => selectOption("")}
                >
                  <span className="model-combobox-check" aria-hidden="true">
                    {selected ? "✓" : ""}
                  </span>
                  <span>{opt.label}</span>
                </button>
              );
            })}

            {groupedModels.groups.map(([provider, entries]) => (
              <div key={provider} className="model-combobox-group">
                <div className="model-combobox-group-title">{provider}</div>
                {entries.map(({ opt, flatIdx }) => {
                  const selected = value.trim() === opt.value;
                  return (
                    <button
                      key={opt.value}
                      type="button"
                      role="option"
                      aria-selected={selected}
                      className={`model-combobox-option mono${selected ? " selected" : ""}${
                        highlightIndex === flatIdx ? " highlighted" : ""
                      }`}
                      onMouseDown={(e) => e.preventDefault()}
                      onMouseEnter={() => setHighlightIndex(flatIdx)}
                      onClick={() => selectOption(opt.value)}
                    >
                      <span className="model-combobox-check" aria-hidden="true">
                        {selected ? "✓" : ""}
                      </span>
                      <span className="model-combobox-option-text">{opt.label}</span>
                    </button>
                  );
                })}
              </div>
            ))}

            {groupedModels.bottomItems.map(({ opt, flatIdx }) => {
              const selected = value.trim() === opt.value;
              return (
                <button
                  key={`__custom__:${opt.value}`}
                  type="button"
                  role="option"
                  aria-selected={selected}
                  className={`model-combobox-option${selected ? " selected" : ""}${
                    highlightIndex === flatIdx ? " highlighted" : ""
                  }`}
                  onMouseDown={(e) => e.preventDefault()}
                  onMouseEnter={() => setHighlightIndex(flatIdx)}
                  onClick={() => selectOption(opt.value)}
                >
                  <span className="model-combobox-check" aria-hidden="true">
                    {selected ? "✓" : ""}
                  </span>
                  <span className="model-combobox-option-text">{opt.label}</span>
                </button>
              );
            })}

            {flatOptions.length === 0 && (
              <div className="model-combobox-empty">
                {loading
                  ? "正在探测模型列表…"
                  : "未匹配到已探测模型，可直接输入 provider:model"}
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
