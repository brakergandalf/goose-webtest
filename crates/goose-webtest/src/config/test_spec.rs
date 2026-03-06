use anyhow::Result;
use std::path::Path;

/// A natural language test specification parsed from a markdown file.
/// Each scenario is a bullet point under a heading.
#[derive(Debug, Clone)]
pub struct TestSpec {
    pub title: String,
    pub scenarios: Vec<TestScenario>,
}

#[derive(Debug, Clone)]
pub struct TestScenario {
    pub description: String,
}

impl TestSpec {
    /// Parse a markdown test spec file
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::from_markdown(&content)
    }

    /// Parse a markdown string into a test spec
    pub fn from_markdown(content: &str) -> Result<Self> {
        let mut title = String::new();
        let mut scenarios = Vec::new();

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("# ") {
                title = trimmed.trim_start_matches("# ").to_string();
            } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
                let desc = trimmed
                    .trim_start_matches("- ")
                    .trim_start_matches("* ")
                    .to_string();
                if !desc.is_empty() {
                    scenarios.push(TestScenario { description: desc });
                }
            }
        }

        if title.is_empty() {
            title = "Unnamed Test Spec".to_string();
        }

        Ok(Self { title, scenarios })
    }

    /// Convert the test spec into a prompt for the agentic node
    pub fn to_prompt(&self) -> String {
        let mut prompt = format!("Execute the following test scenarios for '{}':\n\n", self.title);
        for (i, scenario) in self.scenarios.iter().enumerate() {
            prompt.push_str(&format!("{}. {}\n", i + 1, scenario.description));
        }
        prompt.push_str("\nFor each scenario:\n");
        prompt.push_str("- Navigate to the relevant page\n");
        prompt.push_str("- Perform the described actions\n");
        prompt.push_str("- Use assert tools to validate expected outcomes\n");
        prompt.push_str("- Take a screenshot at each important step\n");
        prompt.push_str("- Log pass/fail for each scenario with log_step\n");
        prompt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_markdown_spec() {
        let md = r#"# Sparkasse Demo - Basic Tests
- After login, check that account balance (Kontostand) is visible
- Navigate to account transactions (Umsätze)
- Verify the transaction list shows recent entries
"#;
        let spec = TestSpec::from_markdown(md).unwrap();
        assert_eq!(spec.title, "Sparkasse Demo - Basic Tests");
        assert_eq!(spec.scenarios.len(), 3);
        assert!(spec.scenarios[0]
            .description
            .contains("account balance"));
    }

    #[test]
    fn test_to_prompt() {
        let spec = TestSpec {
            title: "Test".to_string(),
            scenarios: vec![
                TestScenario {
                    description: "Check login".to_string(),
                },
                TestScenario {
                    description: "Check dashboard".to_string(),
                },
            ],
        };
        let prompt = spec.to_prompt();
        assert!(prompt.contains("1. Check login"));
        assert!(prompt.contains("2. Check dashboard"));
    }
}
