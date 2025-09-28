import React from "react";
import { useTranslation } from "react-i18next";
import { Globe } from "lucide-react";
import { buttonStyles } from "../lib/styles";

const LanguageSwitcher: React.FC = () => {
  const { i18n } = useTranslation();

  const toggleLanguage = () => {
    const newLang = i18n.language === "en" ? "zh" : "en";
    i18n.changeLanguage(newLang);
  };

  return (
    <button
      onClick={toggleLanguage}
      className={buttonStyles.icon}
      title={i18n.language === "en" ? "切换到中文" : "Switch to English"}
    >
      <Globe size={18} />
    </button>
  );
};

export default LanguageSwitcher;
