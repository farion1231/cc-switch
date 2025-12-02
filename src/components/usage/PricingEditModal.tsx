import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { useUpdateModelPricing } from '@/lib/query/usage';
import { useToast } from '@/hooks/use-toast';
import type { ModelPricing } from '@/types/usage';

interface PricingEditModalProps {
  model: ModelPricing;
  onClose: () => void;
}

export function PricingEditModal({ model, onClose }: PricingEditModalProps) {
  const { t } = useTranslation();
  const { toast } = useToast();
  const updatePricing = useUpdateModelPricing();

  const [formData, setFormData] = useState({
    displayName: model.displayName,
    inputCost: model.inputCostPerMillion,
    outputCost: model.outputCostPerMillion,
    cacheReadCost: model.cacheReadCostPerMillion,
    cacheCreationCost: model.cacheCreationCostPerMillion,
  });

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();

    // 验证非负数
    const values = [
      formData.inputCost,
      formData.outputCost,
      formData.cacheReadCost,
      formData.cacheCreationCost,
    ];

    for (const value of values) {
      const num = parseFloat(value);
      if (isNaN(num) || num < 0) {
        toast({
          title: t('common.error', '错误'),
          description: t('usage.invalidPrice', '价格必须为非负数'),
          variant: 'destructive',
        });
        return;
      }
    }

    try {
      await updatePricing.mutateAsync({
        modelId: model.modelId,
        displayName: formData.displayName,
        inputCost: formData.inputCost,
        outputCost: formData.outputCost,
        cacheReadCost: formData.cacheReadCost,
        cacheCreationCost: formData.cacheCreationCost,
      });

      toast({
        title: t('common.success', '成功'),
        description: t('usage.pricingUpdated', '定价已更新'),
      });

      onClose();
    } catch (error) {
      toast({
        title: t('common.error', '错误'),
        description: String(error),
        variant: 'destructive',
      });
    }
  };

  return (
    <Dialog open onOpenChange={onClose}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {t('usage.editPricing', '编辑定价')} - {model.modelId}
          </DialogTitle>
        </DialogHeader>

        <form onSubmit={handleSubmit} className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="displayName">{t('usage.displayName', '显示名称')}</Label>
            <Input
              id="displayName"
              value={formData.displayName}
              onChange={(e) => setFormData({ ...formData, displayName: e.target.value })}
              required
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="inputCost">
              {t('usage.inputCostPerMillion', '输入成本 (每百万 tokens, USD)')}
            </Label>
            <Input
              id="inputCost"
              type="number"
              step="0.01"
              min="0"
              value={formData.inputCost}
              onChange={(e) => setFormData({ ...formData, inputCost: e.target.value })}
              required
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="outputCost">
              {t('usage.outputCostPerMillion', '输出成本 (每百万 tokens, USD)')}
            </Label>
            <Input
              id="outputCost"
              type="number"
              step="0.01"
              min="0"
              value={formData.outputCost}
              onChange={(e) => setFormData({ ...formData, outputCost: e.target.value })}
              required
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="cacheReadCost">
              {t('usage.cacheReadCostPerMillion', '缓存读取成本 (每百万 tokens, USD)')}
            </Label>
            <Input
              id="cacheReadCost"
              type="number"
              step="0.01"
              min="0"
              value={formData.cacheReadCost}
              onChange={(e) => setFormData({ ...formData, cacheReadCost: e.target.value })}
              required
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="cacheCreationCost">
              {t('usage.cacheCreationCostPerMillion', '缓存写入成本 (每百万 tokens, USD)')}
            </Label>
            <Input
              id="cacheCreationCost"
              type="number"
              step="0.01"
              min="0"
              value={formData.cacheCreationCost}
              onChange={(e) => setFormData({ ...formData, cacheCreationCost: e.target.value })}
              required
            />
          </div>

          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose}>
              {t('common.cancel', '取消')}
            </Button>
            <Button type="submit" disabled={updatePricing.isPending}>
              {updatePricing.isPending ? t('common.saving', '保存中...') : t('common.save', '保存')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
