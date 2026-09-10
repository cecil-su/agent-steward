import * as React from "react";
import { cn } from "../../lib/utils";

export interface PageHeadingProps extends Omit<React.HTMLAttributes<HTMLDivElement>, "title"> {
  title: React.ReactNode;
  description?: React.ReactNode;
  eyebrow?: React.ReactNode;
  actions?: React.ReactNode;
}

export const PageHeading = React.forwardRef<HTMLDivElement, PageHeadingProps>(
  ({ className, title, description, eyebrow, actions, children, ...props }, ref) => (
    <div
      ref={ref}
      className={cn("mb-7 flex flex-col items-start justify-between gap-4 sm:flex-row sm:items-center", className)}
      {...props}
    >
      <div className="min-w-0">
        {eyebrow != null && <div className="mb-2 text-[10px] font-bold tracking-[2px] text-primary">{eyebrow}</div>}
        <h1 className="m-0 break-words text-2xl font-semibold tracking-[-0.8px] text-foreground sm:text-[30px]">{title}</h1>
        {description != null && <div className="mt-2 break-words text-[13px] text-muted-foreground">{description}</div>}
        {children}
      </div>
      {actions != null && <div className="flex max-w-full shrink-0 flex-wrap items-center gap-2 sm:justify-end">{actions}</div>}
    </div>
  ),
);
PageHeading.displayName = "PageHeading";
