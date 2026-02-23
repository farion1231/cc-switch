# Model Picker for II-Agent and CC Switch

## Overview

The Model Picker provides **exactly 5 pre-configured models** for each provider, making it easy for users to select the right model for their use case. Both II-Agent (Python) and CC Switch (TypeScript/React) share the same model lists for consistency.

## Features

- **Exactly 5 Models Per Provider**: Curated selection for optimal choice
- **Model Information**: Context window, output tokens, capabilities, pricing
- **Use Case Recommendations**: Suggested models for coding, chat, analysis, etc.
- **Provider Support**: OpenRouter, Anthropic, OpenAI, Google, DeepSeek
- **Tauri Compatible**: Ready for Tauri backend integration
- **Consistent Across Products**: Same models in both II-Agent and CC Switch

## Files

### II-Agent (Python)
- `src/ii_agent/llm/proxy/model_picker.py` - Model definitions with Pydantic schemas
- Tauri command exports for backend integration

### CC Switch (TypeScript/React)
- `src/config/modelPicker.ts` - Model definitions with TypeScript interfaces
- `src/components/ModelPicker.tsx` - React UI component
- `docs/MODEL_PICKER.md` - Documentation

## Pre-configured Providers (5 Models Each)

### OpenRouter
1. **Claude 3.5 Sonnet** (Anthropic) - Best overall performance
2. **GPT-4o** (OpenAI) - Fast multimodal
3. **Gemini 1.5 Pro** (Google) - 1M context window
4. **Llama 3.1 405B** (Meta) - Powerful open source
5. **Mistral Large** (Mistral) - Multilingual excellence

### Anthropic
1. **Claude Sonnet 4** - Balanced performance
2. **Claude Opus 4** - Most powerful
3. **Claude 3.5 Haiku** - Fast & cost-effective
4. **Claude Haiku 3** - Ultra-fast
5. **Claude Opus 3** - Previous flagship

### OpenAI
1. **GPT-4.1** - Latest GPT-4
2. **GPT-4o** - Fast multimodal
3. **GPT-4o Mini** - Cost-effective
4. **o1** - Advanced reasoning
5. **o3 Mini** - Fast reasoning

### Google
1. **Gemini 2.5 Pro** - Latest Pro
2. **Gemini 2.5 Flash** - Fast & efficient
3. **Gemini 2.0 Flash** - Previous fast
4. **Gemini 2.0 Flash Lite** - Most cost-effective
5. **Gemini 1.5 Pro** - Previous Pro

### DeepSeek
1. **DeepSeek Chat** - Conversational
2. **DeepSeek Coder** - Code specialist
3. **DeepSeek Reasoner** - Enhanced reasoning
4. **DeepSeek V3** - Latest general
5. **DeepSeek V2.5** - Previous balanced

## Usage

### II-Agent (Python)

```python
from ii_agent.llm.proxy.model_picker import (
    get_model_picker,
    get_model_ids,
    get_default_model,
    get_models_array,  # For Tauri/JSON
    get_all_providers_info,
    MODEL_PICKERS,
)

# Get model picker for a provider
picker = get_model_picker("openrouter")
if picker:
    print(f"Provider: {picker.provider_name}")
    print(f"Default model: {picker.default_model}")
    print(f"Models: {len(picker.models)}")  # Always 5
    
    for model in picker.models:
        print(f"  - {model.name}: {model.description}")

# Get model IDs for dropdown
model_ids = get_model_ids("anthropic")
# Returns exactly 5 model IDs

# Get models as array for Tauri
models_array = get_models_array("openai")
# Returns list of dicts with all model info

# Get all providers info
providers = get_all_providers_info()
# [{'provider_id': 'openrouter', 'provider_name': 'OpenRouter', 'model_count': 5, ...}, ...]
```

### CC Switch (TypeScript/React)

```typescript
import { ModelPicker } from './ModelPicker';
import { 
  getModelPicker, 
  getModelsArray,
  getAllProvidersInfo 
} from './modelPicker';

// In your component
function ProviderSettings({ providerId }: { providerId: string }) {
  const [selectedModel, setSelectedModel] = useState('');
  
  const picker = getModelPicker(providerId);
  
  if (!picker) return null;
  
  return (
    <ModelPicker
      models={picker.models}  // Exactly 5 models
      selectedModel={selectedModel}
      onModelChange={setSelectedModel}
      label="Select Model"
      showDescriptions={true}
      showCapabilities={true}
      showPricing={true}
    />
  );
}

// Get models array for Tauri
const models = getModelsArray("openrouter");  // ModelInfo[]

// Get all providers
const providers = getAllProvidersInfo();
// [{ providerId, providerName, modelCount: 5, defaultModel }, ...]
```

## ModelPicker Component Props

| Prop | Type | Default | Description |
|------|------|---------|-------------|
| `models` | `ModelInfo[]` | required | Array of exactly 5 models |
| `selectedModel` | `string` | required | Currently selected model ID |
| `onModelChange` | `(id: string) => void` | required | Callback when model changes |
| `label` | `string` | "Model" | Label for the dropdown |
| `showDescriptions` | `boolean` | `true` | Show model descriptions |
| `showCapabilities` | `boolean` | `false` | Show capability badges |
| `showPricing` | `boolean` | `false` | Show pricing information |
| `className` | `string` | `""` | Additional CSS classes |
| `compact` | `boolean` | `false` | Compact mode |

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

## Tauri Integration

### Python Backend Commands

```python
# In your Tauri commands
from ii_agent.llm.proxy.model_picker import (
    get_models_array,
    get_all_providers_info,
)

@tauri_command
def get_provider_models(provider_id: str) -> list:
    """Get 5 models for a provider."""
    return get_models_array(provider_id)

@tauri_command
def list_all_providers() -> list:
    """List all providers with model counts."""
    return get_all_providers_info()
```

### TypeScript Frontend

```typescript
// Invoke Tauri commands
import { invoke } from '@tauri-apps/api/core';

// Get models for provider
const models = await invoke<ModelInfo[]>('get_provider_models', {
  providerId: 'openrouter'
});

// List all providers
const providers = await invoke<ProviderInfo[]>('list_all_providers');
```

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
        # Add exactly 4 more models...
    ],
)

# Add to MODEL_PICKERS
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
    // Add exactly 4 more models...
  ],
};

// Add to MODEL_PICKERS
import { MODEL_PICKERS } from './modelPicker';
MODEL_PICKERS["myprovider"] = NEW_PROVIDER_MODELS;
```

## Benefits

✅ **Consistent Experience**: Same 5 models across both products  
✅ **Curated Selection**: Best models for each use case  
✅ **User-Friendly**: Easy model selection with descriptions  
✅ **Cost Awareness**: Pricing information displayed  
✅ **Capability Matching**: Find models with needed features  
✅ **Tauri Ready**: Built for backend integration  
✅ **Exactly 5 Models**: Optimal choice without overwhelm  

## Future Enhancements

- [ ] Real-time model availability checking
- [ ] Usage-based recommendations
- [ ] Cost calculator
- [ ] Model comparison view
- [ ] Custom model lists per user
- [ ] A/B testing for model performance
- [ ] Provider-specific model icons
