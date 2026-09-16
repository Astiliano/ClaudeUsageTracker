import type { JSX } from "react";

interface Props {
  label: string;
  hint: string;
  checked: boolean;
  onChange: (next: boolean) => void;
}

export function Toggle({ label, hint, checked, onChange }: Props): JSX.Element {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      className="toggle-row"
      onClick={() => onChange(!checked)}
    >
      <div>
        <div className="toggle-text">{label}</div>
        <div className="toggle-hint">{hint}</div>
      </div>
      <span className={"switch" + (checked ? " switch-on" : "")}>
        <span className="knob" />
      </span>
    </button>
  );
}
