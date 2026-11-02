# registry/types.ts

export interface OnboardingScope {
  platform: string | null;
  purchasedModuleKeys: string[];
}

export interface OnboardingTask {
  id: string;
  scope: OnboardingScope;
  titleKey: string;
  descriptionKey: string;
  action: TaskAction;
  weight: number;
  optional?: boolean;
  isDone?: (ctx: OnboardingContext) => boolean;
}

export interface OnboardingContext {
  isReady: boolean;
  shop: Shop | null;
  platform: string | null;
  purchasedModuleKeys: string[];
  hasTableData: (table: string) => boolean;
  hasAutomation: (automation: string) => boolean;
  hasCompanyInfo: boolean;
  hasGoals: boolean;
}

export interface TaskAction {
  type: 'wizard' | 'modal' | 'link';
  params?: Record<string, any>;
}