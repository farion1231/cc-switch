# registry/global.ts

import type { OnboardingTask } from './types';

// Global tasks from setup flow
const DEFAULT_STEPS: OnboardingTask[] = [
  // Merge CONTEXT-based steps here (collect from SetupStore)
  // Example: connect-shop -> single task
];

// Legacy steps from SetupChecklist
const LEGACY_STEPS: OnboardingTask[] = [
  // Example: set-budget, connect-ads
];

// Merge and deduplicate
export const ONBOARDING_GLOBAL_REGISTRY: OnboardingTask[] = [... DEFAULT_STEPS, ... LEGACY_STEPS] // filtered by scope/optional