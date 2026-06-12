//! SectionWriter — incremental output for long-form content generation.
//!
//! Splits generation into manageable sections, each validated independently.
//! Useful for code review reports, architecture docs, and long-form analysis.

use crate::orchestration::model_caller::ModelCaller;
use serde::{Deserialize, Serialize};

/// A single section of generated content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Section {
    pub index: usize,
    pub title: String,
    pub content: String,
    pub word_count: usize,
    pub verified: bool,
}

/// Plan for generating a multi-section document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionPlan {
    pub sections: Vec<SectionSpec>,
    pub max_tokens_per_section: u32,
    pub model: String,
}

/// Specification for one section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionSpec {
    pub title: String,
    pub prompt: String,
    pub max_words: usize,
}

/// Writes documents incrementally, one section at a time.
pub struct SectionWriter {
    caller: ModelCaller,
}

impl SectionWriter {
    pub fn new(caller: ModelCaller) -> Self {
        Self { caller }
    }

    /// Execute a section plan, generating each section sequentially.
    /// Context from previous sections is available via `{previous}` placeholder.
    pub async fn execute(&self, plan: &SectionPlan) -> Result<Vec<Section>, String> {
        let mut sections: Vec<Section> = Vec::new();
        let mut previous_context = String::new();

        for (i, spec) in plan.sections.iter().enumerate() {
            let mut prompt = spec.prompt.clone();
            if !previous_context.is_empty() {
                prompt.push_str("\n\nPrevious sections:\n");
                prompt.push_str(&previous_context);
            }

            let system = format!(
                "You are writing section '{}' of a document. \
                 Write exactly this section. Max {} words. \
                 Output ONLY the section content, no preamble.",
                spec.title, spec.max_words
            );

            let messages = vec![
                serde_json::json!({"role": "system", "content": system}),
                serde_json::json!({"role": "user", "content": prompt}),
            ];

            let resp = self
                .caller
                .call(&plan.model, messages, None, Some(0.3))
                .await
                .map_err(|e| format!("Section '{}' generation failed: {}", spec.title, e))?;

            let word_count = resp.content.split_whitespace().count();
            let section = Section {
                index: i,
                title: spec.title.clone(),
                content: resp.content,
                word_count,
                verified: word_count <= spec.max_words + 50, // 50 word tolerance
            };

            previous_context.push_str(&format!(
                "## {}\n{}\n\n",
                section.title, section.content
            ));

            sections.push(section);
        }

        Ok(sections)
    }

    /// Generate a single section with validation.
    pub async fn write_section(
        &self,
        model: &str,
        title: &str,
        prompt: &str,
        max_words: usize,
        context: &str,
    ) -> Result<Section, String> {
        let spec = SectionSpec {
            title: title.to_string(),
            prompt: if context.is_empty() {
                prompt.to_string()
            } else {
                format!("{}\n\nContext:\n{}", prompt, context)
            },
            max_words,
        };
        let plan = SectionPlan {
            sections: vec![spec],
            max_tokens_per_section: 4096,
            model: model.to_string(),
        };
        let mut results = self.execute(&plan).await?;
        Ok(results.remove(0))
    }
}
