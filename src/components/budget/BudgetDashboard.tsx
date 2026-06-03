import { useTranslation } from "react-i18next";
import { motion } from "framer-motion";
import { useBudgetEventBridge } from "@/hooks/useBudgetEventBridge";
import { BudgetList } from "./BudgetList";

export function BudgetDashboard() {
  useBudgetEventBridge();
  const { t } = useTranslation();

  return (
    <motion.div
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.4 }}
      className="space-y-6 pb-8"
    >
      <div>
        <h2 className="text-2xl font-bold">{t("budget.title")}</h2>
        <p className="text-sm text-muted-foreground">{t("budget.subtitle")}</p>
      </div>
      <BudgetList />
    </motion.div>
  );
}
