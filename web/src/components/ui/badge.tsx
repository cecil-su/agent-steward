import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "../../lib/utils";

export const badgeVariants = cva(
  "inline-flex items-center gap-1 rounded-[5px] border px-[7px] py-0.5 text-[11px] font-medium leading-relaxed",
  {
    variants: {
      variant: {
        default: "border-transparent bg-muted text-muted-foreground",
        secondary: "border-transparent bg-secondary text-secondary-foreground",
        outline: "border-border bg-transparent text-secondary-foreground",
        in_progress: "border-transparent bg-[#e5f2ee] text-[#247757]",
        pending_release: "border-transparent bg-[#e9e8fc] text-[#5745a2]",
        blocked: "border-transparent bg-[#fff0dd] text-[#95661f]",
        closed: "border-transparent bg-[#eeeff0] text-[#737b80]",
        destructive: "border-[#e6c6bf] bg-[#fbeeea] text-destructive",
      },
    },
    defaultVariants: { variant: "default" },
  },
);

export interface BadgeProps
  extends React.HTMLAttributes<HTMLSpanElement>,
    VariantProps<typeof badgeVariants> {}

export const Badge = React.forwardRef<HTMLSpanElement, BadgeProps>(
  ({ className, variant, ...props }, ref) => (
    <span ref={ref} className={cn(badgeVariants({ variant }), className)} {...props} />
  ),
);
Badge.displayName = "Badge";
