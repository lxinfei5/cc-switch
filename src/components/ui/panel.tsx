import * as React from "react";

import { cn } from "@/lib/utils";

/**
 * Panel — a small elevated sub-surface used inside cards/footers to group a
 * stat or a cluster of related info. Shares the Glacier glass-card material so
 * every footer/sub-panel renders with one consistent recipe.
 */
const Panel = React.forwardRef<
  HTMLDivElement,
  React.HTMLAttributes<HTMLDivElement>
>(({ className, ...props }, ref) => (
  <div
    ref={ref}
    className={cn("rounded-xl glass-card px-4 py-3", className)}
    {...props}
  />
));
Panel.displayName = "Panel";

export { Panel };
