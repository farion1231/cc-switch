import React from "react";

interface ListItemRowProps {
  isLast?: boolean;
  children: React.ReactNode;
}

export const ListItemRow: React.FC<ListItemRowProps> = ({
  isLast,
  children,
}) => {
  return (
    <div
      className={`group flex items-center gap-3 px-4 py-2.5 glass-row transition-colors ${
        !isLast ? "border-b border-white/15 dark:border-white/8" : ""
      }`}
    >
      {children}
    </div>
  );
};
