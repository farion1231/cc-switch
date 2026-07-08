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
      className={`group flex items-center gap-3 px-4 py-3 transition-all hover:bg-white/58 dark:hover:bg-white/[0.04] ${
        !isLast ? "border-b border-white/45 dark:border-white/8" : ""
      }`}
    >
      {children}
    </div>
  );
};
