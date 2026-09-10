import * as React from "react";
import { cn } from "../../lib/utils";

export interface EmptyStateProps extends Omit<React.HTMLAttributes<HTMLDivElement>, "title"> {
  title: React.ReactNode;
  description?: React.ReactNode;
  /** Decorative icon; convey meaningful information in the title or description. */
  icon?: React.ReactNode;
  action?: React.ReactNode;
}

export const EmptyState = React.forwardRef<HTMLDivElement, EmptyStateProps>(
  ({ className, title, description, icon, action, children, ...props }, ref) => (
    <div
      ref={ref}
      className={cn("flex flex-col items-center px-5 py-20 text-center text-sm text-muted-foreground", className)}
      {...props}
    >
      {icon != null && <div aria-hidden="true" className="mb-4 text-[32px] text-primary/60">{icon}</div>}
      <h2 className="m-0 max-w-full break-words text-lg font-semibold text-secondary-foreground">{title}</h2>
      {description != null && <div className="mt-1.5 max-w-md break-words">{description}</div>}
      {action != null && <div className="mt-5 flex flex-wrap justify-center gap-2">{action}</div>}
      {children}
    </div>
  ),
);
EmptyState.displayName = "EmptyState";
