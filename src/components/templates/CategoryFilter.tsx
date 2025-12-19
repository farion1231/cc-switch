import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";

interface CategoryFilterProps {
  categories: string[];
  selectedCategory?: string;
  onSelectCategory: (category: string | undefined) => void;
}

export function CategoryFilter({
  categories,
  selectedCategory,
  onSelectCategory,
}: CategoryFilterProps) {
  const { t } = useTranslation();

  return (
    <div className="glass-card rounded-xl p-4 sticky top-0">
      <h3 className="text-sm font-semibold text-foreground mb-3">
        {t("templates.category.title", { defaultValue: "分类" })}
      </h3>
      <div className="h-[calc(100vh-16rem)] overflow-y-auto">
        <div className="space-y-1 pr-2">
          {/* 全部选项 */}
          <Button
            variant={selectedCategory === undefined ? "secondary" : "ghost"}
            size="sm"
            onClick={() => onSelectCategory(undefined)}
            className="w-full justify-start text-sm h-9"
          >
            {t("templates.category.all", { defaultValue: "全部" })}
            {selectedCategory === undefined && (
              <Badge variant="secondary" className="ml-auto text-xs">
                ✓
              </Badge>
            )}
          </Button>

          {/* 分类列表 */}
          {categories.length > 0 && (
            <>
              <div className="h-px bg-border my-2" />
              {categories.map((category) => (
                <Button
                  key={category}
                  variant={
                    selectedCategory === category ? "secondary" : "ghost"
                  }
                  size="sm"
                  onClick={() => onSelectCategory(category)}
                  className="w-full justify-start text-sm h-9"
                >
                  <span className="truncate">{category}</span>
                  {selectedCategory === category && (
                    <Badge variant="secondary" className="ml-auto text-xs">
                      ✓
                    </Badge>
                  )}
                </Button>
              ))}
            </>
          )}

          {/* 无分类提示 */}
          {categories.length === 0 && selectedCategory === undefined && (
            <p className="text-xs text-muted-foreground text-center py-4">
              {t("templates.category.empty", { defaultValue: "暂无分类" })}
            </p>
          )}
        </div>
      </div>
    </div>
  );
}
