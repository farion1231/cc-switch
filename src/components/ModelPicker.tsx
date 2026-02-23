/**
 * Model Picker Component for CC Switch
 * 
 * A reusable dropdown component for selecting models from a provider.
 * Shows exactly 5 recommended models with descriptions and capabilities.
 * Compatible with Tauri backend.
 */

import React, { useState, useMemo } from 'react';

export interface ModelInfo {
  id: string;
  name: string;
  provider: string;
  description: string;
  contextWindow: number;
  maxOutputTokens: number;
  inputCostPerToken?: number;
  outputCostPerToken?: number;
  capabilities?: string[];
  recommendedFor?: string[];
}

export interface ModelPickerProps {
  models: ModelInfo[];  // Array of exactly 5 models
  selectedModel: string;
  onModelChange: (modelId: string) => void;
  label?: string;
  showDescriptions?: boolean;
  showCapabilities?: boolean;
  showPricing?: boolean;
  className?: string;
  compact?: boolean;
}

export const ModelPicker: React.FC<ModelPickerProps> = ({
  models,
  selectedModel,
  onModelChange,
  label = "Model",
  showDescriptions = true,
  showCapabilities = false,
  showPricing = false,
  className = "",
  compact = false,
}) => {
  const [showDetails, setShowDetails] = useState(false);
  
  const selectedModelInfo = useMemo(() => {
    return models.find(m => m.id === selectedModel);
  }, [models, selectedModel]);

  // Format cost for display
  const formatCost = (cost?: number) => {
    if (cost === undefined || cost === 0) return 'Free';
    if (cost < 0.000001) {
      return `$${(cost * 1000000).toFixed(2)}/1M`;
    }
    return `$${(cost * 1000000).toFixed(1)}/1M`;
  };

  // Format context window
  const formatContext = (tokens: number) => {
    if (tokens >= 1000000) {
      return `${(tokens / 1000000).toFixed(0)}M`;
    }
    return `${(tokens / 1000).toFixed(0)}K`;
  };

  if (models.length === 0) {
    return (
      <div className={`model-picker ${className}`}>
        <div style={{ color: '#999', fontStyle: 'italic' }}>
          No models available for this provider
        </div>
      </div>
    );
  }

  return (
    <div className={`model-picker ${className}`}>
      {/* Label */}
      {label && (
        <label 
          className="model-picker-label" 
          style={{ 
            display: 'block', 
            marginBottom: '8px', 
            fontWeight: 600,
            fontSize: '14px',
            color: '#333',
          }}
        >
          {label}
          {models.length === 5 && (
            <span style={{ 
              marginLeft: '8px', 
              fontSize: '12px', 
              color: '#666',
              fontWeight: 'normal',
            }}>
              ({models.length} models)
            </span>
          )}
        </label>
      )}
      
      {/* Model Dropdown */}
      <select
        value={selectedModel}
        onChange={(e) => {
          onModelChange(e.target.value);
          setShowDetails(true);
        }}
        onFocus={() => setShowDetails(true)}
        className="model-picker-select"
        style={{
          width: '100%',
          padding: compact ? '8px' : '10px',
          fontSize: '14px',
          border: '1px solid #ccc',
          borderRadius: '6px',
          backgroundColor: 'white',
          cursor: 'pointer',
          outline: 'none',
        }}
      >
        {models.map((model) => (
          <option key={model.id} value={model.id}>
            {model.name}
          </option>
        ))}
      </select>
      
      {/* Model Details */}
      {showDescriptions && showDetails && selectedModelInfo && (
        <div 
          className="model-picker-info" 
          style={{ 
            marginTop: '12px', 
            padding: compact ? '10px' : '12px', 
            backgroundColor: '#f5f5f5', 
            borderRadius: '6px',
            border: '1px solid #e0e0e0',
          }}
        >
          {/* Model Name */}
          <div style={{ 
            fontWeight: 600, 
            marginBottom: '6px',
            fontSize: '15px',
            color: '#333',
          }}>
            {selectedModelInfo.name}
          </div>
          
          {/* Description */}
          <div style={{ 
            fontSize: '13px', 
            color: '#666', 
            marginBottom: '10px',
            lineHeight: '1.4',
          }}>
            {selectedModelInfo.description}
          </div>
          
          {/* Stats Row */}
          <div style={{ 
            display: 'flex', 
            gap: '16px', 
            fontSize: '12px', 
            color: '#888',
            flexWrap: 'wrap',
          }}>
            <span>
              <strong>Context:</strong> {formatContext(selectedModelInfo.contextWindow)} tokens
            </span>
            <span>
              <strong>Output:</strong> {selectedModelInfo.maxOutputTokens.toLocaleString()} tokens
            </span>
          </div>
          
          {/* Pricing */}
          {showPricing && (selectedModelInfo.inputCostPerToken !== undefined || selectedModelInfo.outputCostPerToken !== undefined) && (
            <div style={{ 
              marginTop: '10px', 
              paddingTop: '10px', 
              borderTop: '1px solid #e0e0e0',
              fontSize: '12px',
              color: '#666',
            }}>
              <div style={{ fontWeight: 600, marginBottom: '4px' }}>Pricing:</div>
              <div style={{ display: 'flex', gap: '16px' }}>
                <span>
                  <strong>Input:</strong> {formatCost(selectedModelInfo.inputCostPerToken)}
                </span>
                <span>
                  <strong>Output:</strong> {formatCost(selectedModelInfo.outputCostPerToken)}
                </span>
              </div>
            </div>
          )}
          
          {/* Capabilities */}
          {showCapabilities && selectedModelInfo.capabilities && selectedModelInfo.capabilities.length > 0 && (
            <div style={{ marginTop: '10px' }}>
              <div style={{ fontSize: '12px', fontWeight: 600, marginBottom: '6px' }}>
                Capabilities:
              </div>
              <div style={{ display: 'flex', gap: '6px', flexWrap: 'wrap' }}>
                {selectedModelInfo.capabilities.map(cap => (
                  <span
                    key={cap}
                    style={{
                      padding: '3px 10px',
                      backgroundColor: '#e0e0e0',
                      borderRadius: '12px',
                      fontSize: '11px',
                      color: '#555',
                      textTransform: 'capitalize',
                    }}
                  >
                    {cap.replace(/_/g, ' ')}
                  </span>
                ))}
              </div>
            </div>
          )}
          
          {/* Recommended For */}
          {selectedModelInfo.recommendedFor && selectedModelInfo.recommendedFor.length > 0 && (
            <div style={{ marginTop: '10px' }}>
              <div style={{ fontSize: '12px', fontWeight: 600, marginBottom: '6px' }}>
                Best for:
              </div>
              <div style={{ display: 'flex', gap: '6px', flexWrap: 'wrap' }}>
                {selectedModelInfo.recommendedFor.map(use => (
                  <span
                    key={use}
                    style={{
                      padding: '3px 10px',
                      backgroundColor: '#e3f2fd',
                      borderRadius: '12px',
                      fontSize: '11px',
                      color: '#1976d2',
                      textTransform: 'capitalize',
                    }}
                  >
                    {use.replace(/_/g, ' ')}
                  </span>
                ))}
              </div>
            </div>
          )}
          
          {/* Toggle Details */}
          <div style={{ marginTop: '10px', textAlign: 'right' }}>
            <button
              onClick={() => setShowDetails(false)}
              style={{
                background: 'none',
                border: 'none',
                color: '#666',
                fontSize: '12px',
                cursor: 'pointer',
                textDecoration: 'underline',
                padding: 0,
              }}
            >
              Hide details
            </button>
          </div>
        </div>
      )}
      
      {/* Show Details Button (when hidden) */}
      {showDescriptions && !showDetails && (
        <div style={{ marginTop: '8px' }}>
          <button
            onClick={() => setShowDetails(true)}
            style={{
              background: 'none',
              border: 'none',
              color: '#1976d2',
              fontSize: '12px',
              cursor: 'pointer',
              textDecoration: 'underline',
              padding: 0,
            }}
          >
            Show model details
          </button>
        </div>
      )}
    </div>
  );
};

export default ModelPicker;
