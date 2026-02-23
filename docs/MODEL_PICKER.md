# Model Picker for II-Agent and CC Switch

## Overview

The Model Picker provides pre-configured lists of 4-5 recommended models for each provider, making it easy for users to select the right model for their use case.

## Features

- **Pre-configured Model Lists**: 4-5 recommended models per provider
- **Model Information**: Context window, output tokens, capabilities, pricing
- **Use Case Recommendations**: Suggested models for coding, chat, analysis, etc.
- **Provider Support**: OpenRouter, Anthropic, OpenAI (easily extensible)

## Files

### II-Agent (Python)
- `src/ii_agent/llm/proxy/model_picker.py` - Model definitions and helper functions

### CC Switch (TypeScript/React)
- `cc-switch-preset/modelPicker.ts` - Model definitions
- `cc-switch-preset/ModelPicker.tsx` - React UI component

## Usage

### II-Agent (Python)

```python
from ii_agent.llm.proxy.model_picker import (
    get_model_picker,
    get_model_ids,
    get_default_model,
    MODEL_PICKERS,
)

# Get model picker for a provider
picker = get_model_picker("openrouter")
if picker:
    print(f"Provider: {picker.provider_name}")
    print(f"Default model: {picker.default_model}")
    print(f"Available models: {len(picker.models)}")
    
    for model in picker.models:
        print(f"  - {model.name}: {model.description}")

# Get model IDs for dropdown
model_ids = get_model_ids("anthropic")
# ['claude-sonnet-4-20250514', 'claude-opus-4-20250514', ...]

# Get default model
default = get_default_model("openai")
# 'gpt-4.1'
```

### CC Switch (TypeScript/React)

```typescript
import { ModelPicker } from './ModelPicker';
import { MODEL_PICKERS, getModelPicker } from './modelPicker';

// In your component
function ProviderSettings({ providerId }) {
  const [selectedModel, setSelectedModel] = useState('');
  
  const picker = getModelPicker(providerId);
  
  if (!picker) return null;
  
  return (
    <ModelPicker
      models={picker.models}
      selectedModel={selectedModel}
      onModelChange={setSelectedModel}
      label="Select Model"
      showDescriptions={true}
      showCapabilities={true}
    />
  );
}
```

## Pre-configured Providers

### OpenRouter (5 models)
1. **Claude 3.5 Sonnet** - Best overall performance
2. **GPT-4o** - Fast multimodal
3. **Gemini 1.5 Pro** - Large context (1M tokens)
4. **Llama 3 70B** - Cost-effective open source
5. **Mistral Large** - Multilingual support

### Anthropic (4 models)
1. **Claude Sonnet 4** - Balanced performance
2. **Claude Opus 4** - Most powerful
3. **Claude Haiku 3** - Fast & cheap
4. **Claude 3.5 Haiku** - Improved Haiku

### OpenAI (4 models)
1. **GPT-4.1** - Latest GPT-4
2. **GPT-4o** - Fast multimodal
3. **GPT-4o Mini** - Cost-effective
4. **o1 Preview** - Reasoning model

## Adding New Providers

### Python (II-Agent)

```python
from ii_agent.llm.proxy.model_picker import ProviderModelPicker, ModelInfo

NEW_PROVIDER_MODELS = ProviderModelPicker(
    provider_id="myprovider",
    provider_name="My Provider",
    default_model="best-model",
    models=[
        ModelInfo(
            id="best-model",
            name="Best Model",
            provider="MyProvider",
            description="Description here",
            context_window=128000,
            max_output_tokens=4096,
            capabilities=["vision", "function_calling"],
            recommended_for=["coding", "chat"],
        ),
        # Add 3-4 more models...
    ],
)

# Add to MODEL_PICKERS dict
from ii_agent.llm.proxy.model_picker import MODEL_PICKERS
MODEL_PICKERS["myprovider"] = NEW_PROVIDER_MODELS
```

### TypeScript (CC Switch)

```typescript
import { ProviderModelPicker, ModelInfo } from './modelPicker';

const NEW_PROVIDER_MODELS: ProviderModelPicker = {
  providerId: "myprovider",
  providerName: "My Provider",
  defaultModel: "best-model",
  models: [
    {
      id: "best-model",
      name: "Best Model",
      provider: "MyProvider",
      description: "Description here",
      contextWindow: 128000,
      maxOutputTokens: 4096,
      capabilities: ["vision", "function_calling"],
      recommendedFor: ["coding", "chat"],
    },
    // Add 3-4 more models...
  ],
};

// Add to MODEL_PICKERS
import { MODEL_PICKERS } from './modelPicker';
MODEL_PICKERS["myprovider"] = NEW_PROVIDER_MODELS;
```

## Integration with Provider Presets

### II-Agent

Update provider presets to include model picker:

```python
from ii_agent.llm.proxy import ProviderManager
from ii_agent.llm.proxy.model_picker import get_model_picker

manager = ProviderManager()

# When adding a provider
provider = Provider(
    id="openrouter",
    name="OpenRouter",
    settings_config={"api_key": "sk-..."},
)
manager.add_provider(provider)

# Get recommended models
picker = get_model_picker("openrouter")
if picker:
    print(f"Recommended models for {picker.provider_name}:")
    for model in picker.models:
        print(f"  - {model.name}: {model.description}")
```

### CC Switch

Update provider configuration UI:

```typescript
import { getModelPicker } from './modelPicker';

function ProviderForm({ providerId }) {
  const picker = getModelPicker(providerId);
  
  return (
    <div>
      {/* Provider settings */}
      
      {/* Model picker dropdown */}
      {picker && (
        <ModelPicker
          models={picker.models}
          selectedModel={formData.model}
          onModelChange={(model) => setFormData({...formData, model})}
        />
      )}
    </div>
  );
}
```

## UI Component Props

| Prop | Type | Default | Description |
|------|------|---------|-------------|
| `models` | `ModelInfo[]` | required | List of models to display |
| `selectedModel` | `string` | required | Currently selected model ID |
| `onModelChange` | `(id: string) => void` | required | Callback when model changes |
| `label` | `string` | "Model" | Label for the dropdown |
| `showDescriptions` | `boolean` | `true` | Show model descriptions |
| `showCapabilities` | `boolean` | `false` | Show capability badges |
| `className` | `string` | `""` | Additional CSS classes |

## Model Info Structure

```typescript
interface ModelInfo {
  id: string;                    // Unique model identifier
  name: string;                  // Display name
  provider: string;              // Provider name
  description: string;           // Short description
  contextWindow: number;         // Context window size
  maxOutputTokens: number;       // Max output tokens
  inputCostPerToken?: number;    // Cost per input token
  outputCostPerToken?: number;   // Cost per output token
  capabilities?: string[];       // Model capabilities
  recommendedFor?: string[];     // Recommended use cases
}
```

## Benefits

✅ **User-Friendly**: Easy model selection with descriptions  
✅ **Best Practices**: Pre-selected recommended models  
✅ **Cost Awareness**: Pricing information displayed  
✅ **Capability Matching**: Find models with needed features  
✅ **Consistent**: Same model lists across both products  

## Future Enhancements

- [ ] Real-time model availability checking
- [ ] Usage-based recommendations
- [ ] Cost calculator
- [ ] Model comparison view
- [ ] Custom model lists per user
- [ ] A/B testing for model performance
