import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";

import { cn } from "@/lib/utils";

const badgeVariants = cva(
  "inline-flex w-fit shrink-0 items-center gap-1 rounded-md border px-1.5 py-0.5 text-[11px] font-medium leading-none",
  {
    variants: {
      variant: {
        default: "border-transparent bg-primary/12 text-primary",
        secondary: "border-border bg-secondary text-muted-foreground",
        success: "border-emerald-600/20 bg-emerald-500/10 text-emerald-700 dark:text-emerald-400",
        warning: "border-amber-600/20 bg-amber-500/10 text-amber-700 dark:text-amber-400",
        destructive: "border-red-600/20 bg-red-500/10 text-red-700 dark:text-red-400",
        info: "border-sky-600/20 bg-sky-500/10 text-sky-700 dark:text-sky-400",
        outline: "border-border bg-background text-foreground",
      },
    },
    defaultVariants: { variant: "default" },
  },
);

function Badge({ className, variant, ...props }: React.ComponentProps<"span"> & VariantProps<typeof badgeVariants>) {
  return <span className={cn(badgeVariants({ variant }), className)} {...props} />;
}

export { Badge, badgeVariants };
