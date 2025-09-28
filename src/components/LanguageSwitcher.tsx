import React from "react";
import { useTranslation } from "react-i18next";
import { Globe } from "lucide-react";
import { buttonStyles } from "../lib/styles";

const LanguageSwitcher: React.FC = () => {
  const { t, i18n } = useTranslation();

  const toggleLanguage = () => {
    const newLang = i18n.language === "en" ? "zh" : "en";
    i18n.changeLanguage(newLang);
  };

  const titleKey =
    i18n.language === "en"
      ? "header.switchToChinese"
      : "header.switchToEnglish";

  return (
    <button
      onClick={toggleLanguage}
      className={buttonStyles.icon}
      title={t(titleKey)}
      aria-label={t(titleKey)}
    >
      <Globe size={18} />
    </button>
  );
};

export default LanguageSwitcher;
