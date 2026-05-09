import * as React from "react";
import { cn } from "@/lib/utils";

type SwitchProps = Omit<
  React.InputHTMLAttributes<HTMLInputElement>,
  "type"
>;

const Switch = React.forwardRef<HTMLInputElement, SwitchProps>(
  ({ className, disabled, ...props }, ref) => (
    <span
      className={cn(
        "relative inline-flex h-5 w-9 shrink-0 cursor-pointer items-center",
        disabled && "cursor-not-allowed opacity-50",
        className,
      )}
    >
      <input
        ref={ref}
        type="checkbox"
        disabled={disabled}
        className="peer absolute inset-0 h-full w-full cursor-[inherit] appearance-none opacity-0"
        {...props}
      />
      <span
        aria-hidden="true"
        className={cn(
          "pointer-events-none absolute inset-0 rounded-full border border-input bg-input/60 transition-colors",
          "peer-checked:border-primary peer-checked:bg-primary",
          "peer-focus-visible:outline-none peer-focus-visible:ring-2 peer-focus-visible:ring-ring peer-focus-visible:ring-offset-2 peer-focus-visible:ring-offset-background",
        )}
      />
      <span
        aria-hidden="true"
        className={cn(
          "pointer-events-none relative ml-0.5 inline-block h-4 w-4 translate-x-0 rounded-full bg-background shadow-sm ring-0 transition-transform",
          "peer-checked:translate-x-4",
        )}
      />
    </span>
  ),
);
Switch.displayName = "Switch";

export { Switch };
