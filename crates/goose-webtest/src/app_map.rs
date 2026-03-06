//! Application map types for structured discovery output.
//! The discover node outputs an AppMap as JSON, which flows via context
//! passing to the generate_and_execute node.

use serde::{Deserialize, Serialize};

/// Complete map of a discovered web application.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppMap {
    /// Base URL of the application
    pub base_url: String,
    /// Application title (from page title or heading)
    pub title: String,
    /// Whether authentication is required
    pub requires_auth: bool,
    /// Discovered pages/sections
    pub pages: Vec<PageInfo>,
    /// Navigation structure (top-level menu items)
    pub navigation: Vec<NavItem>,
}

/// Information about a single discovered page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageInfo {
    /// URL path (relative to base_url)
    pub path: String,
    /// Page title or heading
    pub title: String,
    /// Forms found on this page
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forms: Vec<FormInfo>,
    /// Interactive elements (buttons, dropdowns, tabs)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub interactive_elements: Vec<String>,
    /// Whether this page requires authentication
    pub requires_auth: bool,
}

/// Information about a form on a page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormInfo {
    /// Form purpose (e.g., "Transfer", "Search", "Settings")
    pub purpose: String,
    /// HTTP method if detectable
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// Fields in the form
    pub fields: Vec<FieldInfo>,
    /// Submit button text
    #[serde(skip_serializing_if = "Option::is_none")]
    pub submit_label: Option<String>,
}

/// Information about a form field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldInfo {
    /// Field label text
    pub label: String,
    /// Input type (text, password, select, checkbox, etc.)
    pub field_type: String,
    /// Whether the field is required
    pub required: bool,
    /// Placeholder text if any
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
}

/// Navigation menu item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavItem {
    /// Display label
    pub label: String,
    /// URL or path
    pub url: String,
    /// Sub-items
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<NavItem>,
}
