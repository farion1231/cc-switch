import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Button } from "@/components/ui/button";
import { Alert, AlertDescription } from "@/components/ui/alert";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { useModelPricing, useDeleteModelPricing } from "@/lib/query/usage";
import { PricingEditModal } from "./PricingEditModal";
import type { ModelPricing } from "@/types/usage";
import { Plus, Pencil, Trash2, ChevronDown, ChevronRight } from "lucide-react";

export function PricingConfigPanel() {
  const { t } = useTranslation();
  const { data: pricing, isLoading, error } = useModelPricing();
  const deleteMutation = useDeleteModelPricing();
  const [editingModel, setEditingModel] = useState<ModelPricing | null>(null);
  const [isAddingNew, setIsAddingNew] = useState(false);
  const [deleteConfirm, setDeleteConfirm] = useState<string | null>(null);
  const [isExpanded, setIsExpanded] = useState(false);

  const handleDelete = (modelId: string) => {
    deleteMutation.mutate(modelId, {
      onSuccess: () => {
        setDeleteConfirm(null);
      },
    });
  };

  const handleAddNew = () => {
    setIsAddingNew(true);
    setEditingModel({
      modelId: "",
      displayName: "",
      inputCostPerMillion: "0",
      outputCostPerMillion: "0",
      cacheReadCostPerMillion: "0",
      cacheCreationCostPerMillion: "0",
    });
  };

  if (isLoading) {
    return (
      <Card className="border rounded-lg">
        <CardHeader
          className="cursor-pointer"
          onClick={() => setIsExpanded(!isExpanded)}
        >
          <div className="flex items-center gap-2">
            <ChevronRight className="h-4 w-4" />
            <CardTitle className="text-base">
              {t("usage.modelPricing")}
            </CardTitle>
          </div>
        </CardHeader>
      </Card>
    );
  }

  if (error) {
    return (
      <Card className="border rounded-lg">
        <CardHeader
          className="cursor-pointer"
          onClick={() => setIsExpanded(!isExpanded)}
        >
          <div className="flex items-center gap-2">
            {isExpanded ? (
              <ChevronDown className="h-4 w-4" />
            ) : (
              <ChevronRight className="h-4 w-4" />
            )}
            <CardTitle className="text-base">
              {t("usage.modelPricing")}
            </CardTitle>
          </div>
        </CardHeader>
        {isExpanded && (
          <CardContent>
            <Alert variant="destructive">
              <AlertDescription>
                {t("usage.loadPricingError")}: {String(error)}
              </AlertDescription>
            </Alert>
          </CardContent>
        )}
      </Card>
    );
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between mb-4">
        <h4 className="text-sm font-medium text-muted-foreground">
          {t("usage.modelPricingDesc")} {t("usage.perMillion")}
        </h4>
        <Button
          onClick={(e) => {
            e.stopPropagation();
            handleAddNew();
          }}
          size="sm"
        >
          <Plus className="mr-1 h-4 w-4" />
          {t("common.add")}
        </Button>
      </div>

      <div className="space-y-4">
        {!pricing || pricing.length === 0 ? (
          <Alert>
            <AlertDescription>{t("usage.noPricingData")}</AlertDescription>
          </Alert>
        ) : (
          <div className="rounded-md bg-card/60 shadow-sm">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>{t("usage.model")}</TableHead>
                  <TableHead>{t("usage.displayName")}</TableHead>
                  <TableHead className="text-right">
                    {t("usage.inputCost")}
                  </TableHead>
                  <TableHead className="text-right">
                    {t("usage.outputCost")}
                  </TableHead>
                  <TableHead className="text-right">
                    {t("usage.cacheReadCost")}
                  </TableHead>
                  <TableHead className="text-right">
                    {t("usage.cacheWriteCost")}
                  </TableHead>
                  <TableHead className="text-right">
                    {t("common.actions")}
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {pricing.map((model) => (
                  <TableRow key={model.modelId}>
                    <TableCell className="font-mono text-sm">
                      {model.modelId}
                    </TableCell>
                    <TableCell>{model.displayName}</TableCell>
                    <TableCell className="text-right font-mono text-sm">
                      ${model.inputCostPerMillion}
                    </TableCell>
                    <TableCell className="text-right font-mono text-sm">
                      ${model.outputCostPerMillion}
                    </TableCell>
                    <TableCell className="text-right font-mono text-sm">
                      ${model.cacheReadCostPerMillion}
                    </TableCell>
                    <TableCell className="text-right font-mono text-sm">
                      ${model.cacheCreationCostPerMillion}
                    </TableCell>
                    <TableCell className="text-right">
                      <div className="flex justify-end gap-1">
                        <Button
                          variant="ghost"
                          size="icon"
                          onClick={() => {
                            setIsAddingNew(false);
                            setEditingModel(model);
                          }}
                          title={t("common.edit")}
                        >
                          <Pencil className="h-4 w-4" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          onClick={() => setDeleteConfirm(model.modelId)}
                          title={t("common.delete")}
                          className="text-destructive hover:text-destructive"
                        >
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      </div>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
        )}
      </div>

      {editingModel && (
        <PricingEditModal
          open={!!editingModel}
          model={editingModel}
          isNew={isAddingNew}
          onClose={() => {
            setEditingModel(null);
            setIsAddingNew(false);
          }}
        />
      )}

      <Dialog
        open={!!deleteConfirm}
        onOpenChange={() => setDeleteConfirm(null)}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("usage.deleteConfirmTitle")}</DialogTitle>
            <DialogDescription>
              {t("usage.deleteConfirmDesc")}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteConfirm(null)}>
              {t("common.cancel")}
            </Button>
            <Button
              variant="destructive"
              onClick={() => deleteConfirm && handleDelete(deleteConfirm)}
              disabled={deleteMutation.isPending}
            >
              {deleteMutation.isPending
                ? t("common.deleting")
                : t("common.delete")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
