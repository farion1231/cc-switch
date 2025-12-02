import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table';
import { Button } from '@/components/ui/button';
import { useModelPricing } from '@/lib/query/usage';
import { PricingEditModal } from './PricingEditModal';
import type { ModelPricing } from '@/types/usage';

export function PricingConfigPanel() {
  const { t } = useTranslation();
  const { data: pricing, isLoading } = useModelPricing();
  const [editingModel, setEditingModel] = useState<ModelPricing | null>(null);

  if (isLoading) {
    return <div className="h-[400px] animate-pulse rounded bg-gray-100" />;
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h2 className="text-lg font-semibold">{t('usage.modelPricing', '模型定价')}</h2>
      </div>

      <div className="rounded-md border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>{t('usage.model', '模型')}</TableHead>
              <TableHead>{t('usage.displayName', '显示名称')}</TableHead>
              <TableHead className="text-right">{t('usage.inputCost', '输入成本')}</TableHead>
              <TableHead className="text-right">{t('usage.outputCost', '输出成本')}</TableHead>
              <TableHead className="text-right">{t('usage.cacheReadCost', '缓存读取')}</TableHead>
              <TableHead className="text-right">{t('usage.cacheWriteCost', '缓存写入')}</TableHead>
              <TableHead className="text-right">{t('common.actions', '操作')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {pricing?.map((model) => (
              <TableRow key={model.modelId}>
                <TableCell className="font-mono text-sm">{model.modelId}</TableCell>
                <TableCell>{model.displayName}</TableCell>
                <TableCell className="text-right">${model.inputCostPerMillion}</TableCell>
                <TableCell className="text-right">${model.outputCostPerMillion}</TableCell>
                <TableCell className="text-right">${model.cacheReadCostPerMillion}</TableCell>
                <TableCell className="text-right">${model.cacheCreationCostPerMillion}</TableCell>
                <TableCell className="text-right">
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => setEditingModel(model)}
                  >
                    {t('common.edit', '编辑')}
                  </Button>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>

      {editingModel && (
        <PricingEditModal
          model={editingModel}
          onClose={() => setEditingModel(null)}
        />
      )}
    </div>
  );
}
