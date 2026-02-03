use serde::{Deserialize, Deserializer, Serialize, de};
use std::env;
use std::fmt;
use once_cell::sync::Lazy;

use super::metrics::{increment_requests, increment_errors, RequestTimer};

use rmcp::{
    ServerHandler,
    handler::server::router::tool::ToolRouter,
    handler::server::wrapper::Parameters,
    model::{ServerCapabilities, ServerInfo, CallToolResult, Content},
    ErrorData as McpError,
    schemars, tool, tool_handler, tool_router
};

// =================== 1. CONFIGURATION ===================
// Defaults loaded from environment variables for penalty logic.

#[derive(Debug, Clone)]
pub struct EngineConfig {
    // Penalty calculation defaults
    pub default_rate_per_day: f64,
    pub default_cap: f64,
    pub default_interest_rate: f64,
}

impl EngineConfig {
    pub fn from_env() -> Self {
        Self {
            default_rate_per_day: env::var("ENGINE_DEFAULT_RATE_PER_DAY")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(100.0),  // From LyFin-Compliance-Annex.md: "100 per day"
                
            default_cap: env::var("ENGINE_DEFAULT_CAP")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1000.0),  // From LyFin-Compliance-Annex.md: "Maximum Cap: 1000"
                
            default_interest_rate: env::var("ENGINE_DEFAULT_INTEREST_RATE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.05),  // From LyFin-Compliance-Annex.md: "5 percent annual"
        }
    }
}

static CONFIG: Lazy<EngineConfig> = Lazy::new(EngineConfig::from_env);

// =================== 2. CUSTOM DESERIALIZERS ===================
// Serde visitors that accept numbers or strings and store them as strings.

/// Custom deserializer that accepts both f64 numbers and strings, then parses them
fn deserialize_flexible_f64<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    struct FlexibleF64Visitor;

    impl<'de> de::Visitor<'de> for FlexibleF64Visitor {
        type Value = String;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a number or a string representing a number")
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value.to_string())
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value.to_string())
        }

        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value.to_string())
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value.to_string())
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value)
        }
    }

    deserializer.deserialize_any(FlexibleF64Visitor)
}

// =================== 3. PARSING UTILITIES ===================
// Input validation and string-to-number parsing helpers.

/// Sanitize user input for safe inclusion in error messages
/// Prevents JSON injection, XSS, log injection, and other attacks
fn sanitize_for_error_message(input: &str) -> String {
    // Limit length to prevent DoS and overly verbose errors
    let truncated = if input.len() > 50 { 
        format!("{}...", &input[..47])
    } else { 
        input.to_string() 
    };
    
    // Replace dangerous and non-printable characters
    truncated
        .chars()
        .map(|c| match c {
            // Replace line breaks and control chars that could break JSON/logs
            '\n' | '\r' | '\t' => ' ',
            // Replace quote chars that could break JSON structure  
            '"' | '\'' | '`' => '?',
            // Replace backslashes that could escape JSON
            '\\' => '?',
            // Replace HTML/script chars for XSS prevention
            '<' | '>' => '?',
            // Keep normal printable ASCII and space
            c if c.is_ascii_graphic() || c == ' ' => c,
            // Replace any other non-printable or unicode control chars
            _ => '?'
        })
        .collect()
}

/// Validate input length and format for security
fn validate_input_security(input: &str, field_name: &str) -> Result<(), String> {
    // Check maximum length to prevent DoS
    if input.len() > 100 {
        return Err(format!("Invalid {}: input too long (max 100 characters)", field_name));
    }
    
    // Check for null bytes (can cause issues in some contexts)
    if input.contains('\0') {
        return Err(format!("Invalid {}: input contains null bytes", field_name));
    }
    
    // Check for excessive control characters (potential log injection)
    let control_char_count = input.chars().filter(|c| c.is_control()).count();
    if control_char_count > 2 {  // Allow a couple for legitimate formatting
        return Err(format!("Invalid {}: input contains too many control characters", field_name));
    }
    
    Ok(())
}

/// Parse a string to f64, handling various formats with security validation
fn parse_f64_from_string(s: &str) -> Result<f64, String> {
    let trimmed = s.trim();
    
    // Security validation first
    if let Err(e) = validate_input_security(trimmed, "number") {
        return Err(e);
    }
    
    // Handle empty strings
    if trimmed.is_empty() {
        return Err("Empty string cannot be parsed as number".to_string());
    }
    
    // Sanitize input for error messages
    let sanitized = sanitize_for_error_message(trimmed);
    
    // Remove common formatting characters
    let cleaned = trimmed
        .replace(',', "")  // Remove thousands separators
        .replace('$', "")  // Remove dollar, euro, pound, etc. signs
        .replace('€', "")  // Remove euro signs
        .replace('£', "")  // Remove pound signs
        .replace('¥', "")  // Remove yen signs
        .replace('%', ""); // Remove percentage signs
    
    match cleaned.parse::<f64>() {
        Ok(value) => {
            if value.is_infinite() || value.is_nan() {
                Err(format!("Invalid number: '{}'", sanitized))
            } else {
                Ok(value)
            }
        },
        Err(_) => Err(format!("Cannot parse '{}' as a number", sanitized))
    }
}

// =================== 4. DATA STRUCTURES ===================
// Request/response types for calc_penalty.

#[derive(Debug, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct CalcPenaltyParams {
    #[serde(deserialize_with = "deserialize_flexible_f64")]
    #[schemars(description = "Number of days late")]
    pub days_late: String,
}

// Response structures with explanations
#[derive(Debug, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct CalcPenaltyResponse {
    #[schemars(description = "Calculated penalty amount")]
    pub penalty: f64,
    #[schemars(description = "Explanation of calculation steps")]
    pub explanation: String,
    #[schemars(description = "Any errors in input validation")]
    pub errors: Vec<String>,
    #[schemars(description = "Warnings or additional information")]
    pub warnings: Vec<String>,
}

// =================== 5. COMPATIBILITY ENGINE ===================
// Core calculation logic for penalties and progressive tax.

#[derive(Debug, Clone)]
pub struct CompatibilityEngine {
    tool_router: ToolRouter<Self>,
}

impl CompatibilityEngine {
    /// Calculate penalty with cap and interest
    fn calc_penalty_internal(
        days_late: f64, 
        rate_per_day: f64, 
        cap: f64, 
        interest_rate: f64
    ) -> CalcPenaltyResponse {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let mut explanation_parts = Vec::new();
        
        // Validation
        if days_late < 0.0 {
            errors.push("Days late cannot be negative".to_string());
        }
        if rate_per_day < 0.0 {
            errors.push("Rate per day cannot be negative".to_string());
        }
        if cap < 0.0 {
            errors.push("Cap cannot be negative".to_string());
        }
        if interest_rate < 0.0 {
            errors.push("Interest rate cannot be negative".to_string());
        }
        
        if !errors.is_empty() {
            return CalcPenaltyResponse {
                penalty: 0.0,
                explanation: "Calculation failed due to invalid inputs".to_string(),
                errors,
                warnings,
            };
        }
        
        // Calculate base penalty
        let base_penalty = days_late * rate_per_day;
        explanation_parts.push(format!("Base penalty: {} days × {} = {:.2}", days_late, rate_per_day, base_penalty));
        
        // Apply cap
        let penalty = base_penalty.min(cap);
        if base_penalty > cap {
            explanation_parts.push(format!("Applied cap: {:.2} capped at {:.2}", base_penalty, cap));
            warnings.push(format!("Base penalty {:.2} exceeded cap of {:.2}", base_penalty, cap));
        } else {
            explanation_parts.push(format!("No cap applied ({:.2} ≤ {:.2})", base_penalty, cap));
        }
        
        // Calculate interest
        let interest = penalty * interest_rate;
        explanation_parts.push(format!("Interest: {:.2} × {:.1}% = {:.2}", penalty, interest_rate * 100.0, interest));
        
        let final_penalty = penalty + interest;
        explanation_parts.push(format!("Final penalty: {:.2} + {:.2} = {:.2}", penalty, interest, final_penalty));
        
        if interest_rate > 0.1 {
            warnings.push(format!("High interest rate: {:.1}%", interest_rate * 100.0));
        }
        
        CalcPenaltyResponse {
            penalty: final_penalty,
            explanation: explanation_parts.join(". "),
            errors,
            warnings,
        }
    }

    // Other tool implementations removed.
}

// =================== 6. MCP TOOL ROUTES ===================
// Tool registration and request handling.
#[tool_router]
impl CompatibilityEngine {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    /// Calculate penalty with cap and interest
    /// Logic: penalty = min(days_late × rate_per_day, cap), then add interest = penalty × interest_rate
    #[tool(description = "Calculate penalty with cap and interest. Returns structured response with penalty amount, detailed explanation of calculation steps, errors for invalid inputs, and warnings. Logic: penalty = min(days_late × rate_per_day, cap), then add interest = penalty × interest_rate. Rate, cap, and interest values are configured via environment variables. Example: '12' days late → uses configured defaults")]
    pub async fn calc_penalty(
        &self,
        Parameters(params): Parameters<CalcPenaltyParams>
    ) -> Result<CallToolResult, McpError> {
        let _timer = RequestTimer::new();
        increment_requests();

        // Parse string parameter
        let days_late = match parse_f64_from_string(&params.days_late) {
            Ok(value) => value,
            Err(parse_error) => {
                increment_errors();
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Invalid days_late parameter: {}", parse_error
                ))]));
            }
        };

        let result = Self::calc_penalty_internal(
            days_late,
            CONFIG.default_rate_per_day,
            CONFIG.default_cap,
            CONFIG.default_interest_rate,
        );

        if !result.errors.is_empty() {
            increment_errors();
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "Calculation errors: {}", result.errors.join(", ")
            ))]));
        }

        match serde_json::to_string_pretty(&result) {
            Ok(json_str) => Ok(CallToolResult::success(vec![Content::text(json_str)])),
            Err(e) => {
                increment_errors();
                Ok(CallToolResult::error(vec![Content::text(format!(
                    "Error serializing response: {}", e
                ))]))
            }
        }
    }

    // Other tool routes removed.
}

// =================== 7. MCP SERVER HANDLER ===================
// Server metadata and capabilities exposed to MCP clients.
#[tool_handler]
impl ServerHandler for CompatibilityEngine {
    fn get_info(&self) -> ServerInfo {
        // Read basic information from .env file (replaced by sync script during release)
        let name = "penalty-engine-mcp-rs".to_string();
        let version = "2.0.2".to_string();
        let title = "Penalty Engine MCP Server".to_string();
        let website_url = "https://github.com/alpha-hack-program/compatibility-engine-mcp-rs.git#workshop".to_string();

        ServerInfo {
            instructions: Some(
                "Penalty Engine providing a penalty calculation function:\
            \n\n1. calc_penalty - Calculate penalty with cap and interest\
            \n\nThe function is strongly typed and provides explicit calculations.".into()
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: rmcp::model::Implementation {
                name: name,
                version: version, 
                title: Some(title), 
                icons: None, 
                website_url: Some(website_url) 
            },
            ..Default::default()
        }
    }
}

// =================== 8. TESTS ===================
// Unit tests covering parsing and tool behavior.
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_calc_penalty() {
        let engine = CompatibilityEngine::new();
        let params = CalcPenaltyParams {
            days_late: "12".to_string(),
        };
        
        let result = engine.calc_penalty(Parameters(params)).await;
        assert!(result.is_ok());
        
        let call_result = result.unwrap();
        let content = call_result.content;
        let json_text = content[0].raw.as_text().unwrap().text.as_str();
        let response: CalcPenaltyResponse = serde_json::from_str(json_text).unwrap();
        
        // Expected: min(12 * 100, 1000) = 1000, then 1000 + (1000 * 0.05) = 1050
        assert_eq!(response.penalty, 1050.0);
        assert!(response.errors.is_empty());
        assert!(response.explanation.contains("Applied cap"));
        assert!(response.explanation.contains("Interest"));
    }

    #[tokio::test]
    async fn test_calc_penalty_with_errors() {
        let engine = CompatibilityEngine::new();
        let params = CalcPenaltyParams {
            days_late: "-5".to_string(),  // Invalid: negative
        };
        
        let result = engine.calc_penalty(Parameters(params)).await;
        assert!(result.is_ok());
        
        let call_result = result.unwrap();
        // Should be an error response due to invalid input
        assert!(call_result.is_error.unwrap_or(false));
        let content = call_result.content;
        let error_text = content[0].raw.as_text().unwrap().text.as_str();
        // Now the error comes from parsing and calculation
        assert!(error_text.contains("Days late cannot be negative") || error_text.contains("Calculation errors"));
    }

    #[tokio::test]
    async fn test_calc_penalty_small_amount() {
        let engine = CompatibilityEngine::new();
        let params = CalcPenaltyParams {
            days_late: "10".to_string(),
        };
        
        let result = engine.calc_penalty(Parameters(params)).await;
        assert!(result.is_ok());
        
        let call_result = result.unwrap();
        let content = call_result.content;
        let json_text = content[0].raw.as_text().unwrap().text.as_str();
        let response: CalcPenaltyResponse = serde_json::from_str(json_text).unwrap();
        
        // Uses configured defaults: rate_per_day=100.0, cap=1000.0, interest_rate=0.05
        // Expected: min(10 * 100, 1000) = 1000, then 1000 + (1000 * 0.05) = 1050
        assert_eq!(response.penalty, 1050.0);
        assert!(response.errors.is_empty());
    }

    #[tokio::test]
    async fn test_string_parsing_invalid_format() {
        let engine = CompatibilityEngine::new();
        let params = CalcPenaltyParams {
            days_late: "not-a-number".to_string(), // Invalid format
        };
        
        let result = engine.calc_penalty(Parameters(params)).await;
        assert!(result.is_ok());
        
        let call_result = result.unwrap();
        assert!(call_result.is_error.unwrap_or(false));
        let content = call_result.content;
        let error_text = content[0].raw.as_text().unwrap().text.as_str();
        assert!(error_text.contains("Invalid days_late parameter"));
        assert!(error_text.contains("Cannot parse 'not-a-number' as a number"));
    }

    #[tokio::test]
    async fn test_string_parsing_with_whitespace() {
        let engine = CompatibilityEngine::new();
        let params = CalcPenaltyParams {
            days_late: "  12.5  ".to_string(), // Test whitespace trimming
        };
        
        let result = engine.calc_penalty(Parameters(params)).await;
        assert!(result.is_ok());
        
        let call_result = result.unwrap();
        assert!(!call_result.is_error.unwrap_or(false));
        let content = call_result.content;
        let json_text = content[0].raw.as_text().unwrap().text.as_str();
        let response: CalcPenaltyResponse = serde_json::from_str(json_text).unwrap();
        
        // Should parse as 12.5 and calculate penalty
        assert!(response.penalty > 0.0);
        assert!(response.errors.is_empty());
    }

    // =================== SECURITY TESTS ===================

    #[tokio::test]
    async fn test_security_input_length_limit() {
        let engine = CompatibilityEngine::new();
        // Create a string longer than 100 characters
        let long_string = "1".repeat(101);
        let params = CalcPenaltyParams {
            days_late: long_string,
        };
        
        let result = engine.calc_penalty(Parameters(params)).await;
        assert!(result.is_ok());
        
        let call_result = result.unwrap();
        assert!(call_result.is_error.unwrap_or(false));
        let content = call_result.content;
        let error_text = content[0].raw.as_text().unwrap().text.as_str();
        assert!(error_text.contains("input too long"));
        assert!(error_text.contains("max 100 characters"));
    }

    #[tokio::test]
    async fn test_security_json_injection_prevention() {
        let engine = CompatibilityEngine::new();
        let params = CalcPenaltyParams {
            days_late: r#"12", "malicious": "payload"#.to_string(),
        };
        
        let result = engine.calc_penalty(Parameters(params)).await;
        assert!(result.is_ok());
        
        let call_result = result.unwrap();
        assert!(call_result.is_error.unwrap_or(false));
        let content = call_result.content;
        let error_text = content[0].raw.as_text().unwrap().text.as_str();
        
        // Quotes should be sanitized to prevent JSON breaking
        assert!(!error_text.contains(r#""malicious""#));
        assert!(error_text.contains("12?, ?malicious?: ?payload"));
    }

    #[tokio::test]
    async fn test_security_xss_prevention() {
        let engine = CompatibilityEngine::new();
        let params = CalcPenaltyParams {
            days_late: "<script>alert('xss')</script>".to_string(),
        };
        
        let result = engine.calc_penalty(Parameters(params)).await;
        assert!(result.is_ok());
        
        let call_result = result.unwrap();
        assert!(call_result.is_error.unwrap_or(false));
        let content = call_result.content;
        let error_text = content[0].raw.as_text().unwrap().text.as_str();
        
        // HTML/script tags should be sanitized
        assert!(!error_text.contains("<script>"));
        assert!(!error_text.contains("</script>"));
        assert!(error_text.contains("?script?"));
    }

    #[tokio::test]
    async fn test_security_newline_injection_prevention() {
        let engine = CompatibilityEngine::new();
        let params = CalcPenaltyParams {
            days_late: "12\n\nFAKE LOG ENTRY: Unauthorized access".to_string(),
        };
        
        let result = engine.calc_penalty(Parameters(params)).await;
        assert!(result.is_ok());
        
        let call_result = result.unwrap();
        assert!(call_result.is_error.unwrap_or(false));
        let content = call_result.content;
        let error_text = content[0].raw.as_text().unwrap().text.as_str();
        
        // Newlines should be replaced with spaces
        assert!(!error_text.contains('\n'));
        assert!(error_text.contains("12  FAKE LOG ENTRY"));
    }

    #[tokio::test]
    async fn test_security_null_byte_prevention() {
        let engine = CompatibilityEngine::new();
        let params = CalcPenaltyParams {
            days_late: "12\0malicious".to_string(),
        };
        
        let result = engine.calc_penalty(Parameters(params)).await;
        assert!(result.is_ok());
        
        let call_result = result.unwrap();
        assert!(call_result.is_error.unwrap_or(false));
        let content = call_result.content;
        let error_text = content[0].raw.as_text().unwrap().text.as_str();
        
        // Should be rejected due to null bytes
        assert!(error_text.contains("null bytes"));
    }

    #[tokio::test]
    async fn test_security_control_character_limit() {
        let engine = CompatibilityEngine::new();
        // Create input with excessive control characters
        let malicious_input = "12\x01\x02\x03\x04\x05evil";
        let params = CalcPenaltyParams {
            days_late: malicious_input.to_string(),
        };
        
        let result = engine.calc_penalty(Parameters(params)).await;
        assert!(result.is_ok());
        
        let call_result = result.unwrap();
        assert!(call_result.is_error.unwrap_or(false));
        let content = call_result.content;
        let error_text = content[0].raw.as_text().unwrap().text.as_str();
        
        // Should be rejected due to too many control characters
        assert!(error_text.contains("too many control characters"));
    }

    #[tokio::test]
    async fn test_security_length_truncation_in_error() {
        let engine = CompatibilityEngine::new();
        // Create a 60-character invalid string (over the 50 error display limit but under input limit)
        let long_invalid = "not-a-number-".repeat(4) + "extra-text"; // ~60 chars of invalid input
        let params = CalcPenaltyParams {
            days_late: long_invalid,
        };
        
        let result = engine.calc_penalty(Parameters(params)).await;
        assert!(result.is_ok());
        
        let call_result = result.unwrap();
        assert!(call_result.is_error.unwrap_or(false));
        let content = call_result.content;
        let error_text = content[0].raw.as_text().unwrap().text.as_str();
        
        // Error message should be truncated with "..." since input is over 50 chars
        assert!(error_text.contains("..."));
        assert!(error_text.len() < 200); // Error message itself should be reasonable length
    }

    #[tokio::test]
    async fn test_security_backslash_sanitization() {
        let engine = CompatibilityEngine::new();
        let params = CalcPenaltyParams {
            days_late: r#"12\"malicious\"payload"#.to_string(),
        };
        
        let result = engine.calc_penalty(Parameters(params)).await;
        assert!(result.is_ok());
        
        let call_result = result.unwrap();
        assert!(call_result.is_error.unwrap_or(false));
        let content = call_result.content;
        let error_text = content[0].raw.as_text().unwrap().text.as_str();
        
        // Backslashes and quotes should be sanitized
        assert!(!error_text.contains(r#"\""#));
        assert!(error_text.contains("12??malicious??payload"));
    }

    #[tokio::test]
    async fn test_all_parameter_types_with_numbers() {
        // Test CalcPenaltyParams with native number
        let json_penalty = r#"{"days_late": 12.5}"#;
        let penalty_params: CalcPenaltyParams = serde_json::from_str(json_penalty).unwrap();
        assert_eq!(penalty_params.days_late, "12.5");
    }
}
