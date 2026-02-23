/**
 * Model Picker Component
 * 
 * A reusable dropdown component for selecting models from a provider.
 * Shows 4-5 recommended models with descriptions and capabilities.
 */

import React from 'react';

interface ModelInfo {
  id: string;
  name: string;
  provider: string;
  description: string;
  contextWindow: number;
  maxOutputTokens: number;
  capabilities?: string[];
  recommendedFor?: string[];
}

interface ModelPickerProps {
  models: ModelInfo[];
  selectedModel: string;
  onModelChange: (modelId: string) => void;
  label?: string;
  showDescriptions?: boolean;
  showCapabilities?: boolean;
  className?: string;
}

export const ModelPicker: React.FC<ModelPickerProps> = ({
  models,
  selectedModel,
  onModelChange,
  label = "Model",
  showDescriptions = true,
  showCapabilities = false,
  className = "",
}) => {
  return (
    <div className={`model-picker ${className}`}>
      {label && (
        <label className="model-picker-label" style={{ display: 'block', marginBottom: '8px', fontWeight: 600 }}>
          {label}
        </label>
      )}
      
      <select
        value={selectedModel}
        onChange={(e) => onModelChange(e.target.value)}
        className="model-picker-select"
        style={{
          width: '100%',
          padding: '10px',
          fontSize: '14px',
          border: '1px solid #ccc',
          borderRadius: '6px',
          backgroundColor: 'white',
          cursor: 'pointer',
        }}
      >
        {models.map((model) => (
          <option key={model.id} value={model.id}>
            {model.name}
          </option>
        ))}
      </select>
      
      {showDescriptions && selectedModel && (
        <div className="model-picker-info" style={{ marginTop: '12px', padding: '12px', backgroundColor: '#f5f5f5', borderRadius: '6px' }}>
          {(() => {
            const selected = models.find(m => m.id === selectedModel);
            if (!selected) return null;
            
            return (
              <div>
                <div style={{ fontWeight: 600, marginBottom: '4px' }}>
                  {selected.name}
                </div>
                <div style={{ fontSize: '13px', color: '#666', marginBottom: '8px' }}>
                  {selected.description}
                </div>
                
                <div style={{ display: 'flex', gap: '12px', fontSize: '12px', color: '#888' }}>
                  <span>
                    <strong>Context:</strong> {(selected.contextWindow / 1000).toFixed(0)}K tokens
                  </span>
                  <span>
                    <strong>Output:</strong> {selected.maxOutputTokens} tokens
                  </span>
                </div>
                
                {showCapabilities && selected.capabilities && selected.capabilities.length > 0 && (
                  <div style={{ marginTop: '8px' }}>
                    <strong style={{ fontSize: '12px' }}>Capabilities:</strong>
                    <div style={{ display: 'flex', gap: '6px', flexWrap: 'wrap', marginTop: '4px' }}>
                      {selected.capabilities.map(cap => (
                        <span
                          key={cap}
                          style={{
                            padding: '2px 8px',
                            backgroundColor: '#e0e0e0',
                            borderRadius: '12px',
                            fontSize: '11px',
                          }}
                        >
                          {cap.replace(/_/g, ' ')}
                        </span>
                      ))}
                    </div>
                  </div>
                )}
                
                {selected.recommendedFor && selected.recommendedFor.length > 0 && (
                  <div style={{ marginTop: '8px' }}>
                    <strong style={{ fontSize: '12px' }}>Best for:</strong>
                    <div style={{ display: 'flex', gap: '6px', flexWrap: 'wrap', marginTop: '4px' }}>
                      {selected.recommendedFor.map(use => (
                        <span
                          key={use}
                          style={{
                            padding: '2px 8px',
                            backgroundColor: '#e3f2fd',
                            borderRadius: '12px',
                            fontSize: '11px',
                            color: '#1976d2',
                          }}
                        >
                          {use.replace(/_/g, ' ')}
                        </span>
                      ))}
                    </div>
                  </div>
                )}
              </div>
            );
          })()}
        </div>
      )}
    </div>
  );
};

export default ModelPicker;
