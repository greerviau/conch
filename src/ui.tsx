import type { ReactNode } from "react";

export function Row({
  title,
  note,
  children,
}: {
  title: string;
  note?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div className="row">
      <div className="row-text">
        <b>{title}</b>
        {note && <span>{note}</span>}
      </div>
      {children && <div className="row-control">{children}</div>}
    </div>
  );
}

export function Toggle({
  checked,
  onChange,
  label,
  disabled = false,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <label className="toggle">
      <input
        type="checkbox"
        aria-label={label}
        checked={checked}
        disabled={disabled}
        onChange={(e) => onChange(e.target.checked)}
      />
      <span aria-hidden="true" />
    </label>
  );
}
