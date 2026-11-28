import React, { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

interface Props {
  label: string;
  trigger: React.ReactNode;
  title?: string;
  disabled?: boolean;
  onOpen?: () => void;
  children: (close: () => void) => React.ReactNode;
}

export function ControlPopover({ label, trigger, title, disabled, onOpen, children }: Props) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const id = useId();
  const close = () => {
    setOpen(false);
    anchor.current?.focus();
  };
  useLayoutEffect(() => {
    if (!open || !panel.current || !anchor.current) return;
    const surface = panel.current;
    const button = anchor.current;
    const position = () => {
      const rect = button.getBoundingClientRect();
      const above = rect.top > window.innerHeight - rect.bottom;
      surface.style.maxHeight =
        Math.max(80, (above ? rect.top : window.innerHeight - rect.bottom) - 12) + "px";
      surface.style.left =
        Math.max(6, Math.min(rect.left, window.innerWidth - surface.offsetWidth - 6)) + "px";
      surface.style.top = (above ? rect.top - surface.offsetHeight - 4 : rect.bottom + 4) + "px";
    };
    const observer = new ResizeObserver(position);
    observer.observe(surface);
    position();
    (surface.querySelector<HTMLElement>("input:not(:disabled),button:not(:disabled)") ?? surface).focus();
    window.addEventListener("resize", position);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", position);
    };
  }, [open]);
  useEffect(() => {
    if (!open) return;
    const outside = (event: Event) => {
      if (
        event.target instanceof Node &&
        !panel.current?.contains(event.target) &&
        !anchor.current?.contains(event.target)
      )
        setOpen(false);
    };
    document.addEventListener("pointerdown", outside);
    document.addEventListener("focusin", outside);
    return () => {
      document.removeEventListener("pointerdown", outside);
      document.removeEventListener("focusin", outside);
    };
  }, [open]);
  return (
    <>
      <button
        ref={anchor}
        type="button"
        className="btn btn-ghost btn-sm"
        aria-label={label}
        title={title ?? label}
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        disabled={disabled}
        onClick={() => {
          if (!open) onOpen?.();
          setOpen(!open);
        }}
      >
        {trigger}
      </button>
      {open &&
        createPortal(
          <div
            id={id}
            role="dialog"
            aria-label={label}
            ref={panel}
            tabIndex={-1}
            className="control-popover"
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.stopPropagation();
                close();
              }
              if (
                (event.key === "ArrowDown" || event.key === "ArrowUp") &&
                event.target instanceof HTMLButtonElement
              ) {
                const buttons = Array.from(
                  event.currentTarget.querySelectorAll<HTMLButtonElement>("button:not(:disabled)"),
                );
                const index = buttons.indexOf(event.target);
                if (index >= 0) {
                  event.preventDefault();
                  buttons[
                    (index + (event.key === "ArrowDown" ? 1 : buttons.length - 1)) % buttons.length
                  ]?.focus();
                }
              }
            }}
          >
            {children(close)}
          </div>,
          document.body,
        )}
    </>
  );
}

export function ChoiceGroup({
  label,
  value,
  options,
  disabled,
  onChange,
}: {
  label: string;
  value: string;
  options: ReadonlyArray<{ value: string; label: string }>;
  disabled?: boolean;
  onChange: (value: string) => void;
}) {
  const name = useId();
  return (
    <fieldset className="choice-group" disabled={disabled}>
      <legend>{label}</legend>
      {options.map((option) => (
        <label key={option.value}>
          <input
            name={name}
            type="radio"
            checked={value === option.value}
            onChange={() => onChange(option.value)}
          />{" "}
          {option.label}
        </label>
      ))}
    </fieldset>
  );
}
