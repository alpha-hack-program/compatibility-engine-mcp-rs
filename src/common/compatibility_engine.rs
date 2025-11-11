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

// =================== CONFIGURATION ===================

#[derive(Debug, Clone)]
pub struct EngineConfig {
    // Penalty calculation defaults
    pub default_rate_per_day: f64,
    pub default_cap: f64,
    pub default_interest_rate: f64,
    
    // Tax calculation defaults
    pub default_thresholds: Vec<f64>,
    pub default_rates: Vec<f64>,
    pub default_surcharge_threshold: f64,
    pub default_surcharge_rate: f64,
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
                
            default_thresholds: env::var("ENGINE_DEFAULT_THRESHOLDS")
                .ok()
                .and_then(|s| Self::parse_vec_f64(&s))
                .unwrap_or_else(|| vec![10000.0]),  // From 2025_61-FR.md: "First bracket: 10% on income up to 10000"
                
            default_rates: env::var("ENGINE_DEFAULT_RATES")
                .ok()
                .and_then(|s| Self::parse_vec_f64(&s))
                .unwrap_or_else(|| vec![0.10, 0.20]),  // From 2025_61-FR.md: "10% up to 10000", "20% exceeding 10000"
                
            default_surcharge_threshold: env::var("ENGINE_DEFAULT_SURCHARGE_THRESHOLD")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(5000.0),  // From 2025_61-FR.md: "Where the tax calculated... exceeds 5000"
                
            default_surcharge_rate: env::var("ENGINE_DEFAULT_SURCHARGE_RATE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.02),  // From 2025_61-FR.md: "a surcharge of 2% of the total tax liability"
        }
    }
    
    fn parse_vec_f64(s: &str) -> Option<Vec<f64>> {
        let parsed: Result<Vec<f64>, _> = s
            .split(',')
            .map(|part| part.trim().parse::<f64>())
            .collect();
        parsed.ok()
    }
}

static CONFIG: Lazy<EngineConfig> = Lazy::new(EngineConfig::from_env);

// =================== PARSING UTILITIES ===================

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

/// Parse a string to i32, handling various formats with security validation
fn parse_i32_from_string(s: &str) -> Result<i32, String> {
    let trimmed = s.trim();
    
    // Security validation first
    if let Err(e) = validate_input_security(trimmed, "integer") {
        return Err(e);
    }
    
    // Handle empty strings
    if trimmed.is_empty() {
        return Err("Empty string cannot be parsed as integer".to_string());
    }
    
    // Sanitize input for error messages
    let sanitized = sanitize_for_error_message(trimmed);
    
    // Remove common formatting characters
    let cleaned = trimmed.replace(',', ""); // Remove thousands separators
    
    match cleaned.parse::<i32>() {
        Ok(value) => Ok(value),
        Err(_) => Err(format!("Cannot parse '{}' as an integer", sanitized))
    }
}

/// Parse a string to bool, handling various formats with security validation
fn parse_bool_from_string(s: &str) -> Result<bool, String> {
    let trimmed = s.trim();
    
    // Security validation first
    if let Err(e) = validate_input_security(trimmed, "boolean") {
        return Err(e);
    }
    
    // Handle empty strings
    if trimmed.is_empty() {
        return Err("Empty string cannot be parsed as boolean".to_string());
    }
    
    // Sanitize input for error messages
    let sanitized = sanitize_for_error_message(trimmed);
    
    // Parse various boolean representations (case-insensitive)
    match trimmed.to_lowercase().as_str() {
        "true" | "t" | "yes" | "y" | "1" | "on" => Ok(true),
        "false" | "f" | "no" | "n" | "0" | "off" => Ok(false),
        _ => Err(format!("Cannot parse '{}' as a boolean (expected: true/false, yes/no, 1/0, etc.)", sanitized))
    }
}

// =================== CUSTOM DESERIALIZERS ===================

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

/// Custom deserializer that accepts both i32 numbers and strings, then parses them
fn deserialize_flexible_i32<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    struct FlexibleI32Visitor;

    impl<'de> de::Visitor<'de> for FlexibleI32Visitor {
        type Value = String;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("an integer or a string representing an integer")
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
            // Convert float to int if it's a whole number
            if value.fract() == 0.0 {
                Ok((value as i64).to_string())
            } else {
                Err(E::custom(format!("Expected integer, got float: {}", value)))
            }
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

    deserializer.deserialize_any(FlexibleI32Visitor)
}

/// Custom deserializer that accepts both booleans and strings, then parses them
fn deserialize_flexible_bool<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    struct FlexibleBoolVisitor;

    impl<'de> de::Visitor<'de> for FlexibleBoolVisitor {
        type Value = String;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a boolean or a string representing a boolean")
        }

        fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(if value { "true".to_string() } else { "false".to_string() })
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

    deserializer.deserialize_any(FlexibleBoolVisitor)
}

// =================== DATA STRUCTURES ===================

#[derive(Debug, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct CalcPenaltyParams {
    #[serde(deserialize_with = "deserialize_flexible_f64")]
    #[schemars(description = "Number of days late")]
    pub days_late: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct CalcTaxParams {
    #[serde(deserialize_with = "deserialize_flexible_f64")]
    #[schemars(description = "Total income")]
    pub income: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct CheckVotingParams {
    #[serde(deserialize_with = "deserialize_flexible_i32")]
    #[schemars(description = "Total number of eligible voters")]
    pub eligible_voters: String,
    #[serde(deserialize_with = "deserialize_flexible_i32")]
    #[schemars(description = "Actual turnout (number of people who voted)")]
    pub turnout: String,
    #[serde(deserialize_with = "deserialize_flexible_i32")]
    #[schemars(description = "Number of yes votes")]
    pub yes_votes: String,
    #[schemars(description = "Type of proposal: 'general' or 'amendment'")]
    pub proposal_type: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct DistributeWaterfallParams {
    #[serde(deserialize_with = "deserialize_flexible_f64")]
    #[schemars(description = "Total cash available for distribution")]
    pub cash_available: String,
    #[serde(deserialize_with = "deserialize_flexible_f64")]
    #[schemars(description = "Senior debt amount")]
    pub senior_debt: String,
    #[serde(deserialize_with = "deserialize_flexible_f64")]
    #[schemars(description = "Junior debt amount")]
    pub junior_debt: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct DistributeWaterfallResult {
    #[schemars(description = "Amount allocated to senior debt")]
    pub senior: f64,
    #[schemars(description = "Amount allocated to junior debt")]
    pub junior: f64,
    #[schemars(description = "Amount allocated to equity")]
    pub equity: f64,
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

#[derive(Debug, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct CalcTaxResponse {
    #[schemars(description = "Calculated tax amount")]
    pub tax: f64,
    #[schemars(description = "Explanation of calculation steps")]
    pub explanation: String,
    #[schemars(description = "Any errors in input validation")]
    pub errors: Vec<String>,
    #[schemars(description = "Warnings or additional information")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct CheckVotingResponse {
    #[schemars(description = "Whether the proposal passes")]
    pub passes: bool,
    #[schemars(description = "Explanation of voting calculation")]
    pub explanation: String,
    #[schemars(description = "Any errors in input validation")]
    pub errors: Vec<String>,
    #[schemars(description = "Warnings or additional information")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct DistributeWaterfallResponse {
    #[schemars(description = "Distribution results")]
    pub distribution: DistributeWaterfallResult,
    #[schemars(description = "Explanation of waterfall distribution")]
    pub explanation: String,
    #[schemars(description = "Any errors in input validation")]
    pub errors: Vec<String>,
    #[schemars(description = "Warnings or additional information")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct CheckHousingGrantResponse {
    #[schemars(description = "Whether eligible for housing grant")]
    pub eligible: bool,
    #[schemars(description = "Explanation of eligibility calculation")]
    pub explanation: String,
    #[schemars(description = "Any errors in input validation")]
    pub errors: Vec<String>,
    #[schemars(description = "Additional requirements or warnings")]
    pub additional_requirements: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct CheckHousingGrantParams {
    #[serde(deserialize_with = "deserialize_flexible_f64")]
    #[schemars(description = "Area Median Income (AMI)")]
    pub ami: String,
    #[serde(deserialize_with = "deserialize_flexible_i32")]
    #[schemars(description = "Household size")]
    pub household_size: String,
    #[serde(deserialize_with = "deserialize_flexible_f64")]
    #[schemars(description = "Household income")]
    pub income: String,
    #[serde(deserialize_with = "deserialize_flexible_bool")]
    #[schemars(description = "Whether the household has another subsidy (true/false, yes/no, 1/0)")]
    pub has_other_subsidy: String,
}

// =================== COMPATIBILITY ENGINE ===================

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

    /// Calculate progressive tax with surcharge
    fn calc_tax_internal(
        income: f64,
        thresholds: Vec<f64>,
        rates: Vec<f64>,
        surcharge_threshold: f64,
        surcharge_rate: f64,
    ) -> CalcTaxResponse {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let mut explanation_parts = Vec::new();
        
        // Validation
        if income < 0.0 {
            errors.push("Income cannot be negative".to_string());
        }
        if rates.len() != thresholds.len() + 1 {
            errors.push(format!("Invalid bracket configuration: {} rates for {} thresholds (should be {} rates)", 
                rates.len(), thresholds.len(), thresholds.len() + 1));
        }
        if surcharge_threshold < 0.0 {
            errors.push("Surcharge threshold cannot be negative".to_string());
        }
        if surcharge_rate < 0.0 {
            errors.push("Surcharge rate cannot be negative".to_string());
        }
        
        // Check if thresholds are sorted
        for i in 1..thresholds.len() {
            if thresholds[i] <= thresholds[i-1] {
                errors.push("Tax thresholds must be in ascending order".to_string());
                break;
            }
        }
        
        if !errors.is_empty() {
            return CalcTaxResponse {
                tax: 0.0,
                explanation: "Tax calculation failed due to invalid inputs".to_string(),
                errors,
                warnings,
            };
        }

        let mut tax = 0.0;
        let mut remaining_income = income;
        explanation_parts.push(format!("Starting income: {:.2}", income));
        
        // Apply progressive brackets
        for (i, &threshold) in thresholds.iter().enumerate() {
            if remaining_income <= 0.0 {
                break;
            }
            
            let prev_threshold = if i == 0 { 0.0 } else { thresholds[i - 1] };
            let bracket_size = threshold - prev_threshold;
            let taxable_in_bracket = if remaining_income > bracket_size {
                bracket_size
            } else {
                remaining_income
            };
            
            let bracket_tax = taxable_in_bracket * rates[i];
            tax += bracket_tax;
            remaining_income -= taxable_in_bracket;
            
            explanation_parts.push(format!(
                "Bracket {} ({:.0}-{:.0}): {:.2} × {:.1}% = {:.2}", 
                i + 1, prev_threshold, threshold, taxable_in_bracket, rates[i] * 100.0, bracket_tax
            ));
        }
        
        // Apply highest bracket rate to remaining income
        if remaining_income > 0.0 {
            let highest_rate = rates[rates.len() - 1];
            let highest_bracket_tax = remaining_income * highest_rate;
            tax += highest_bracket_tax;
            
            let prev_threshold = if thresholds.is_empty() { 0.0 } else { thresholds[thresholds.len() - 1] };
            explanation_parts.push(format!(
                "Highest bracket ({:.0}+): {:.2} × {:.1}% = {:.2}", 
                prev_threshold, remaining_income, highest_rate * 100.0, highest_bracket_tax
            ));
        }
        
        explanation_parts.push(format!("Subtotal tax: {:.2}", tax));
        
        // Apply surcharge if tax exceeds threshold
        if tax > surcharge_threshold {
            let surcharge = tax * surcharge_rate;
            tax += surcharge;
            explanation_parts.push(format!(
                "Surcharge applied (tax {:.2} > {:.2}): {:.2} × {:.1}% = {:.2}", 
                tax - surcharge, surcharge_threshold, tax - surcharge, surcharge_rate * 100.0, surcharge
            ));
            explanation_parts.push(format!("Final tax with surcharge: {:.2}", tax));
        } else {
            explanation_parts.push(format!("No surcharge (tax {:.2} ≤ {:.2})", tax, surcharge_threshold));
        }
        
        if surcharge_rate > 0.05 {
            warnings.push(format!("High surcharge rate: {:.1}%", surcharge_rate * 100.0));
        }
        
        CalcTaxResponse {
            tax,
            explanation: explanation_parts.join(". "),
            errors,
            warnings,
        }
    }

    /// Check if voting proposal passes
    fn check_voting_internal(
        eligible_voters: i32,
        turnout: i32,
        yes_votes: i32,
        proposal_type: &str,
    ) -> CheckVotingResponse {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let mut explanation_parts = Vec::new();
        
        // Validation
        if eligible_voters <= 0 {
            errors.push("Eligible voters must be positive".to_string());
        }
        if turnout < 0 {
            errors.push("Turnout cannot be negative".to_string());
        }
        if yes_votes < 0 {
            errors.push("Yes votes cannot be negative".to_string());
        }
        if turnout > eligible_voters {
            errors.push("Turnout cannot exceed eligible voters".to_string());
        }
        if yes_votes > turnout {
            errors.push("Yes votes cannot exceed turnout".to_string());
        }
        if !matches!(proposal_type, "general" | "amendment") {
            errors.push(format!("Invalid proposal type '{}' (must be 'general' or 'amendment')", proposal_type));
        }
        
        if !errors.is_empty() {
            return CheckVotingResponse {
                passes: false,
                explanation: "Voting check failed due to invalid inputs".to_string(),
                errors,
                warnings,
            };
        }
        
        // Check minimum turnout (60%)
        let turnout_percentage = turnout as f64 / eligible_voters as f64;
        explanation_parts.push(format!(
            "Turnout: {} out of {} eligible voters ({:.1}%)", 
            turnout, eligible_voters, turnout_percentage * 100.0
        ));
        
        if turnout_percentage < 0.60 {
            explanation_parts.push("Turnout requirement: ≥60% - FAILED".to_string());
            explanation_parts.push("Proposal fails due to insufficient turnout".to_string());
            
            return CheckVotingResponse {
                passes: false,
                explanation: explanation_parts.join(". "),
                errors,
                warnings,
            };
        } else {
            explanation_parts.push("Turnout requirement: ≥60% - PASSED".to_string());
        }
        
        // Check voting threshold based on proposal type
        let yes_percentage = yes_votes as f64 / turnout as f64;
        explanation_parts.push(format!(
            "Yes votes: {} out of {} ({:.1}%)", 
            yes_votes, turnout, yes_percentage * 100.0
        ));
        
        let passes = match proposal_type {
            "general" => {
                let required = 50.0;
                explanation_parts.push(format!("General proposal requirement: >{}%", required));
                let passes = yes_percentage > 0.50;
                explanation_parts.push(format!(
                    "Vote threshold: {:.1}% > {}% - {}", 
                    yes_percentage * 100.0, required, if passes { "PASSED" } else { "FAILED" }
                ));
                passes
            },
            "amendment" => {
                let required = 66.7;
                explanation_parts.push(format!("Amendment requirement: ≥{:.1}%", required));
                let passes = yes_percentage >= 2.0 / 3.0;
                explanation_parts.push(format!(
                    "Vote threshold: {:.1}% ≥ {:.1}% - {}", 
                    yes_percentage * 100.0, required, if passes { "PASSED" } else { "FAILED" }
                ));
                passes
            },
            _ => false,
        };
        
        explanation_parts.push(format!("Final result: Proposal {}", if passes { "PASSES" } else { "FAILS" }));
        
        if turnout_percentage < 0.70 {
            warnings.push("Low turnout (below 70%)".to_string());
        }
        if turnout > 0 && yes_votes == 0 {
            warnings.push("No yes votes recorded".to_string());
        }
        
        CheckVotingResponse {
            passes,
            explanation: explanation_parts.join(". "),
            errors,
            warnings,
        }
    }

    /// Distribute cash in waterfall structure
    fn distribute_waterfall_internal(
        cash_available: f64,
        senior_debt: f64,
        junior_debt: f64,
    ) -> DistributeWaterfallResponse {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let mut explanation_parts = Vec::new();
        
        // Validation
        if cash_available < 0.0 {
            errors.push("Cash available cannot be negative".to_string());
        }
        if senior_debt < 0.0 {
            errors.push("Senior debt cannot be negative".to_string());
        }
        if junior_debt < 0.0 {
            errors.push("Junior debt cannot be negative".to_string());
        }
        
        if !errors.is_empty() {
            return DistributeWaterfallResponse {
                distribution: DistributeWaterfallResult { senior: 0.0, junior: 0.0, equity: 0.0 },
                explanation: "Waterfall distribution failed due to invalid inputs".to_string(),
                errors,
                warnings,
            };
        }
        
        let mut remaining = cash_available;
        explanation_parts.push(format!("Starting cash: {:.2}", cash_available));
        
        // Pay senior debt first
        let senior_payment = remaining.min(senior_debt);
        remaining -= senior_payment;
        
        if senior_debt > 0.0 {
            if senior_payment == senior_debt {
                explanation_parts.push(format!("Senior debt: {:.2} fully paid", senior_debt));
            } else {
                explanation_parts.push(format!("Senior debt: {:.2} partially paid ({:.2} of {:.2})", senior_payment, senior_payment, senior_debt));
                warnings.push(format!("Senior debt underpaid by {:.2}", senior_debt - senior_payment));
            }
        } else {
            explanation_parts.push("No senior debt to pay".to_string());
        }
        
        explanation_parts.push(format!("Remaining after senior: {:.2}", remaining));
        
        // Pay junior debt second
        let junior_payment = remaining.min(junior_debt);
        remaining -= junior_payment;
        
        if junior_debt > 0.0 {
            if junior_payment == junior_debt {
                explanation_parts.push(format!("Junior debt: {:.2} fully paid", junior_debt));
            } else if junior_payment > 0.0 {
                explanation_parts.push(format!("Junior debt: {:.2} partially paid ({:.2} of {:.2})", junior_payment, junior_payment, junior_debt));
                warnings.push(format!("Junior debt underpaid by {:.2}", junior_debt - junior_payment));
            } else {
                explanation_parts.push("Junior debt: no funds available".to_string());
                warnings.push(format!("Junior debt unpaid ({:.2})", junior_debt));
            }
        } else {
            explanation_parts.push("No junior debt to pay".to_string());
        }
        
        explanation_parts.push(format!("Remaining for equity: {:.2}", remaining));
        
        // Remainder goes to equity
        let equity_payment = remaining;
        
        if equity_payment > 0.0 {
            explanation_parts.push(format!("Equity distribution: {:.2}", equity_payment));
        } else {
            explanation_parts.push("No funds available for equity".to_string());
        }
        
        let total_debt = senior_debt + junior_debt;
        if cash_available < total_debt {
            warnings.push(format!("Insufficient cash: {:.2} available vs {:.2} total debt", cash_available, total_debt));
        }
        
        DistributeWaterfallResponse {
            distribution: DistributeWaterfallResult {
                senior: senior_payment,
                junior: junior_payment,
                equity: equity_payment,
            },
            explanation: explanation_parts.join(". "),
            errors,
            warnings,
        }
    }

    /// Check housing grant eligibility
    fn check_housing_grant_internal(
        ami: f64,
        household_size: i32,
        income: f64,
        has_other_subsidy: bool,
    ) -> CheckHousingGrantResponse {
        let mut errors = Vec::new();
        let mut additional_requirements = Vec::new();
        let mut explanation_parts = Vec::new();
        
        // Validation
        if ami <= 0.0 {
            errors.push("Area Median Income (AMI) must be positive".to_string());
        }
        if household_size <= 0 {
            errors.push("Household size must be positive".to_string());
        }
        if income < 0.0 {
            errors.push("Income cannot be negative".to_string());
        }
        
        if !errors.is_empty() {
            return CheckHousingGrantResponse {
                eligible: false,
                explanation: "Housing grant eligibility check failed due to invalid inputs".to_string(),
                errors,
                additional_requirements,
            };
        }
        
        explanation_parts.push(format!("Area Median Income (AMI): {:.2}", ami));
        explanation_parts.push(format!("Household size: {}", household_size));
        explanation_parts.push(format!("Household income: {:.2}", income));
        explanation_parts.push(format!("Has other subsidy: {}", if has_other_subsidy { "Yes" } else { "No" }));
        
        // Check subsidy requirement first
        if has_other_subsidy {
            explanation_parts.push("Subsidy check: FAILED (already has another subsidy)".to_string());
            explanation_parts.push("Result: NOT ELIGIBLE".to_string());
            
            additional_requirements.push("Must not have any other housing subsidies or assistance".to_string());
            
            return CheckHousingGrantResponse {
                eligible: false,
                explanation: explanation_parts.join(". "),
                errors,
                additional_requirements,
            };
        } else {
            explanation_parts.push("Subsidy check: PASSED (no other subsidies)".to_string());
        }
        
        // Calculate threshold
        let base_threshold = 0.60 * ami;
        explanation_parts.push(format!("Base income threshold: 60% of AMI = {:.2}", base_threshold));
        
        let threshold = if household_size > 4 {
            let adjusted_threshold = base_threshold * 1.10;
            explanation_parts.push(format!(
                "Household size adjustment: {} > 4, threshold increased by 10% to {:.2}", 
                household_size, adjusted_threshold
            ));
            adjusted_threshold
        } else {
            explanation_parts.push(format!("No household size adjustment needed ({} ≤ 4)", household_size));
            base_threshold
        };
        
        // Check income eligibility
        let eligible = income <= threshold;
        explanation_parts.push(format!(
            "Income eligibility: {:.2} {} {:.2} - {}", 
            income, 
            if eligible { "≤" } else { ">" }, 
            threshold,
            if eligible { "PASSED" } else { "FAILED" }
        ));
        
        explanation_parts.push(format!("Final result: {}", if eligible { "ELIGIBLE" } else { "NOT ELIGIBLE" }));
        
        // Add additional requirements
        additional_requirements.push("Must provide proof of income documentation".to_string());
        additional_requirements.push("Must be a first-time homebuyer or meet other program criteria".to_string());
        if household_size > 4 {
            additional_requirements.push("Large household size may require additional documentation".to_string());
        }
        if income > threshold * 0.9 {
            additional_requirements.push("Income is close to threshold - verify all deductions are included".to_string());
        }
        
        CheckHousingGrantResponse {
            eligible,
            explanation: explanation_parts.join(". "),
            errors,
            additional_requirements,
        }
    }
}

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

    /// Calculate progressive tax with surcharge
    /// Logic: apply progressive brackets defined by thresholds and rates. If total tax > surcharge_threshold, add surcharge = tax × surcharge_rate
    #[tool(description = "Calculate progressive tax with surcharge. Returns structured response with tax amount, detailed explanation of bracket calculations and surcharge application, errors for invalid inputs, and warnings. Logic: apply progressive brackets defined by thresholds and rates. If total tax > surcharge_threshold, add surcharge = tax × surcharge_rate. Tax brackets, rates, and surcharge values are configured via environment variables. Example: '40000' income → uses configured tax brackets")]
    pub async fn calc_tax(
        &self,
        Parameters(params): Parameters<CalcTaxParams>
    ) -> Result<CallToolResult, McpError> {
        let _timer = RequestTimer::new();
        increment_requests();

        // Parse string parameter
        let income = match parse_f64_from_string(&params.income) {
            Ok(value) => value,
            Err(parse_error) => {
                increment_errors();
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Invalid income parameter: {}", parse_error
                ))]));
            }
        };

        let result = Self::calc_tax_internal(
            income,
            CONFIG.default_thresholds.clone(),
            CONFIG.default_rates.clone(),
            CONFIG.default_surcharge_threshold,
            CONFIG.default_surcharge_rate,
        );

        if !result.errors.is_empty() {
            increment_errors();
            Ok(CallToolResult::error(vec![Content::text(format!(
                "Calculation errors: {}", result.errors.join(", ")
            ))]))
        } else {
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
    }

    /// Check voting proposal eligibility
    /// Logic: turnout must be ≥60% of eligible. Then check: If proposal_type = "general" → yes_votes / turnout > 0.50. If proposal_type = "amendment" → yes_votes / turnout ≥ 2/3
    #[tool(description = "Check voting proposal eligibility. Returns structured response with pass/fail result, detailed explanation of turnout and voting threshold checks, validation errors, and warnings. Logic: turnout must be ≥60% of eligible. Then check: If proposal_type = 'general' → yes_votes / turnout > 0.50. If proposal_type = 'amendment' → yes_votes / turnout ≥ 2/3. Example: '100' eligible, turnout = '70', yes_votes = '55', proposal_type = 'amendment' → turnout = 70%, yes% = 78.6%, passes")]
    pub async fn check_voting(
        &self,
        Parameters(params): Parameters<CheckVotingParams>
    ) -> Result<CallToolResult, McpError> {
        let _timer = RequestTimer::new();
        increment_requests();

        // Parse string parameters
        let eligible_voters = match parse_i32_from_string(&params.eligible_voters) {
            Ok(value) => value,
            Err(parse_error) => {
                increment_errors();
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Invalid eligible_voters parameter: {}", parse_error
                ))]));
            }
        };

        let turnout = match parse_i32_from_string(&params.turnout) {
            Ok(value) => value,
            Err(parse_error) => {
                increment_errors();
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Invalid turnout parameter: {}", parse_error
                ))]));
            }
        };

        let yes_votes = match parse_i32_from_string(&params.yes_votes) {
            Ok(value) => value,
            Err(parse_error) => {
                increment_errors();
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Invalid yes_votes parameter: {}", parse_error
                ))]));
            }
        };

        let result = Self::check_voting_internal(
            eligible_voters,
            turnout,
            yes_votes,
            &params.proposal_type,
        );

        if !result.errors.is_empty() {
            increment_errors();
            Ok(CallToolResult::error(vec![Content::text(format!(
                "Validation errors: {}", result.errors.join(", ")
            ))]))
        } else {
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
    }

    /// Distribute cash in waterfall structure
    /// Logic: Pay senior first (up to senior_debt). Then junior (up to junior_debt). Any remainder goes to equity
    #[tool(description = "Distribute cash in waterfall structure. Returns structured response with distribution amounts, detailed explanation of waterfall payments, validation errors, and warnings about underpayments. Logic: Pay senior first (up to senior_debt). Then junior (up to junior_debt). Any remainder goes to equity. Example: cash = '15000000', senior = '8000000', junior = '10000000' → {senior: 8M, junior: 7M, equity: 0}")]
    pub async fn distribute_waterfall(
        &self,
        Parameters(params): Parameters<DistributeWaterfallParams>
    ) -> Result<CallToolResult, McpError> {
        let _timer = RequestTimer::new();
        increment_requests();

        // Parse string parameters
        let cash_available = match parse_f64_from_string(&params.cash_available) {
            Ok(value) => value,
            Err(parse_error) => {
                increment_errors();
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Invalid cash_available parameter: {}", parse_error
                ))]));
            }
        };

        let senior_debt = match parse_f64_from_string(&params.senior_debt) {
            Ok(value) => value,
            Err(parse_error) => {
                increment_errors();
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Invalid senior_debt parameter: {}", parse_error
                ))]));
            }
        };

        let junior_debt = match parse_f64_from_string(&params.junior_debt) {
            Ok(value) => value,
            Err(parse_error) => {
                increment_errors();
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Invalid junior_debt parameter: {}", parse_error
                ))]));
            }
        };

        let result = Self::distribute_waterfall_internal(
            cash_available,
            senior_debt,
            junior_debt,
        );

        if !result.errors.is_empty() {
            increment_errors();
            Ok(CallToolResult::error(vec![Content::text(format!(
                "Validation errors: {}", result.errors.join(", ")
            ))]))
        } else {
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
    }

    /// Check housing grant eligibility
    /// Logic: Base threshold = 0.60 × AMI. If household_size > 4, threshold = threshold × 1.10. Must satisfy income ≤ threshold. Must not have another subsidy
    #[tool(description = "Check housing grant eligibility. Returns structured response with eligibility result, detailed explanation of threshold calculations and checks, validation errors, and additional requirements. Logic: Base threshold = 0.60 × AMI. If household_size > 4, threshold = threshold × 1.10. Must satisfy income ≤ threshold. Must not have another subsidy. Example A: AMI = '50000', household_size = '5', income = '32000', has_other_subsidy = 'false' → eligible. Example B: same AMI & size, income = '34000' → not eligible. Example C: income = '32000' but has_other_subsidy = 'true' → not eligible")]
    pub async fn check_housing_grant(
        &self,
        Parameters(params): Parameters<CheckHousingGrantParams>
    ) -> Result<CallToolResult, McpError> {
        let _timer = RequestTimer::new();
        increment_requests();

        // Parse string parameters
        let ami = match parse_f64_from_string(&params.ami) {
            Ok(value) => value,
            Err(parse_error) => {
                increment_errors();
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Invalid ami parameter: {}", parse_error
                ))]));
            }
        };

        let household_size = match parse_i32_from_string(&params.household_size) {
            Ok(value) => value,
            Err(parse_error) => {
                increment_errors();
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Invalid household_size parameter: {}", parse_error
                ))]));
            }
        };

        let income = match parse_f64_from_string(&params.income) {
            Ok(value) => value,
            Err(parse_error) => {
                increment_errors();
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Invalid income parameter: {}", parse_error
                ))]));
            }
        };

        let has_other_subsidy = match parse_bool_from_string(&params.has_other_subsidy) {
            Ok(value) => value,
            Err(parse_error) => {
                increment_errors();
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Invalid has_other_subsidy parameter: {}", parse_error
                ))]));
            }
        };

        let result = Self::check_housing_grant_internal(
            ami,
            household_size,
            income,
            has_other_subsidy,
        );

        if !result.errors.is_empty() {
            increment_errors();
            Ok(CallToolResult::error(vec![Content::text(format!(
                "Validation errors: {}", result.errors.join(", ")
            ))]))
        } else {
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
    }
}

#[tool_handler]
impl ServerHandler for CompatibilityEngine {
    fn get_info(&self) -> ServerInfo {
        // Read basic information from .env file (replaced by sync script during release)
        let name = "compatibility-engine-mcp-rs".to_string();
        let version = "1.3.3".to_string();
        let title = "Compatibility Engine MCP Server".to_string();
        let website_url = "https://github.com/alpha-hack-program/compatibility-engine-mcp-rs.git".to_string();

        ServerInfo {
            instructions: Some(
                "Compatibility Engine providing five calculation and eligibility functions:\
                 \n\n1. calc_penalty - Calculate penalty with cap and interest\
                 \n2. calc_tax - Calculate progressive tax with surcharge\
                 \n3. check_voting - Check voting proposal eligibility\
                 \n4. distribute_waterfall - Distribute cash in waterfall structure\
                 \n5. check_housing_grant - Check housing grant eligibility\
                 \n\nAll functions are strongly typed and provide explicit calculations.".into()
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
    async fn test_calc_tax() {
        let engine = CompatibilityEngine::new();
        let params = CalcTaxParams {
            income: "40000".to_string(),
        };
        
        let result = engine.calc_tax(Parameters(params)).await;
        assert!(result.is_ok());
        
        let call_result = result.unwrap();
        let content = call_result.content;
        let json_text = content[0].raw.as_text().unwrap().text.as_str();
        let response: CalcTaxResponse = serde_json::from_str(json_text).unwrap();
        
        // Expected: 10000 * 0.10 + 30000 * 0.20 = 1000 + 6000 = 7000
        // Surcharge: 7000 > 5000 (surcharge_threshold), so 7000 + (7000 * 0.02) = 7,140
        assert_eq!(response.tax, 7140.0);
        assert!(response.errors.is_empty());
        assert!(response.explanation.contains("Bracket 1"));
        assert!(response.explanation.contains("Surcharge applied"));
    }

    #[tokio::test]
    async fn test_check_voting_amendment_passes() {
        let engine = CompatibilityEngine::new();
        let params = CheckVotingParams {
            eligible_voters: "100".to_string(),
            turnout: "70".to_string(),
            yes_votes: "55".to_string(),
            proposal_type: "amendment".to_string(),
        };
        
        let result = engine.check_voting(Parameters(params)).await;
        assert!(result.is_ok());
        
        let call_result = result.unwrap();
        let content = call_result.content;
        let json_text = content[0].raw.as_text().unwrap().text.as_str();
        let response: CheckVotingResponse = serde_json::from_str(json_text).unwrap();
        
        // Expected: turnout = 70%, yes% = 55/70 = 78.6% ≥ 66.67%, passes
        assert_eq!(response.passes, true);
        assert!(response.errors.is_empty());
        assert!(response.explanation.contains("70.0%"));
        assert!(response.explanation.contains("PASSED"));
    }

    #[tokio::test]
    async fn test_distribute_waterfall() {
        let engine = CompatibilityEngine::new();
        let params = DistributeWaterfallParams {
            cash_available: "15000000".to_string(),
            senior_debt: "8000000".to_string(),
            junior_debt: "10000000".to_string(),
        };
        
        let result = engine.distribute_waterfall(Parameters(params)).await;
        assert!(result.is_ok());
        
        let call_result = result.unwrap();
        let content = call_result.content;
        let json_text = content[0].raw.as_text().unwrap().text.as_str();
        let response: DistributeWaterfallResponse = serde_json::from_str(json_text).unwrap();
        
        // Expected: senior = 8M, junior = 7M, equity = 0
        assert_eq!(response.distribution.senior, 8_000_000.0);
        assert_eq!(response.distribution.junior, 7_000_000.0);
        assert_eq!(response.distribution.equity, 0.0);
        assert!(response.errors.is_empty());
        assert!(response.explanation.contains("Senior debt: 8000000.00 fully paid"));
        assert!(response.explanation.contains("Junior debt: 7000000.00 partially paid"));
    }

    #[tokio::test]
    async fn test_check_housing_grant_eligible() {
        let engine = CompatibilityEngine::new();
        let params = CheckHousingGrantParams {
            ami: "50000".to_string(),
            household_size: "5".to_string(),
            income: "32000".to_string(),
            has_other_subsidy: "false".to_string(),
        };
        
        let result = engine.check_housing_grant(Parameters(params)).await;
        assert!(result.is_ok());
        
        let call_result = result.unwrap();
        let content = call_result.content;
        let json_text = content[0].raw.as_text().unwrap().text.as_str();
        let response: CheckHousingGrantResponse = serde_json::from_str(json_text).unwrap();
        
        // Expected: threshold = 0.60 * 50000 * 1.10 = 33000, income 32000 ≤ 33000, eligible
        assert_eq!(response.eligible, true);
        assert!(response.errors.is_empty());
        assert!(response.explanation.contains("5 > 4, threshold increased by 10%"));
        assert!(response.explanation.contains("ELIGIBLE"));
    }

    #[tokio::test]
    async fn test_check_housing_grant_not_eligible_income() {
        let engine = CompatibilityEngine::new();
        let params = CheckHousingGrantParams {
            ami: "50000".to_string(),
            household_size: "5".to_string(),
            income: "34000".to_string(),
            has_other_subsidy: "false".to_string(),
        };
        
        let result = engine.check_housing_grant(Parameters(params)).await;
        assert!(result.is_ok());
        
        let call_result = result.unwrap();
        let content = call_result.content;
        let json_text = content[0].raw.as_text().unwrap().text.as_str();
        let response: CheckHousingGrantResponse = serde_json::from_str(json_text).unwrap();
        
        // Expected: threshold = 33000, income 34000 > 33000, not eligible
        assert_eq!(response.eligible, false);
        assert!(response.errors.is_empty());
        assert!(response.explanation.contains("NOT ELIGIBLE"));
    }

    #[tokio::test]
    async fn test_check_housing_grant_not_eligible_subsidy() {
        let engine = CompatibilityEngine::new();
        let params = CheckHousingGrantParams {
            ami: "50000".to_string(),
            household_size: "5".to_string(),
            income: "32000".to_string(),
            has_other_subsidy: "true".to_string(),
        };
        
        let result = engine.check_housing_grant(Parameters(params)).await;
        assert!(result.is_ok());
        
        let call_result = result.unwrap();
        let content = call_result.content;
        let json_text = content[0].raw.as_text().unwrap().text.as_str();
        let response: CheckHousingGrantResponse = serde_json::from_str(json_text).unwrap();
        
        // Expected: has other subsidy, not eligible
        assert_eq!(response.eligible, false);
        assert!(response.errors.is_empty());
        assert!(response.explanation.contains("already has another subsidy"));
        assert!(!response.additional_requirements.is_empty());
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
    async fn test_calc_tax_invalid_brackets() {
        // This test is no longer relevant since we use fixed configuration
        // but let's keep it to test that the default configuration is valid
        let engine = CompatibilityEngine::new();
        let params = CalcTaxParams {
            income: "40000".to_string(),
        };
        
        let result = engine.calc_tax(Parameters(params)).await;
        assert!(result.is_ok());
        let call_result = result.unwrap();
        // Should succeed since we use valid default configuration
        assert!(!call_result.is_error.unwrap_or(false));
        let content = call_result.content;
        let json_text = content[0].raw.as_text().unwrap().text.as_str();
        let response: CalcTaxResponse = serde_json::from_str(json_text).unwrap();
        assert!(response.errors.is_empty());
    }

    #[tokio::test]
    async fn test_check_voting_invalid_proposal_type() {
        let engine = CompatibilityEngine::new();
        let params = CheckVotingParams {
            eligible_voters: "100".to_string(),
            turnout: "70".to_string(),
            yes_votes: "55".to_string(),
            proposal_type: "invalid_type".to_string(),
        };
        
        let result = engine.check_voting(Parameters(params)).await;
        assert!(result.is_ok());
        
        let call_result = result.unwrap();
        assert!(call_result.is_error.unwrap_or(false));
        let content = call_result.content;
        let error_text = content[0].raw.as_text().unwrap().text.as_str();
        assert!(error_text.contains("Invalid proposal type"));
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
    async fn test_calc_tax_with_surcharge() {
        let engine = CompatibilityEngine::new();
        let params = CalcTaxParams {
            income: "50000".to_string(),
        };
        
        let result = engine.calc_tax(Parameters(params)).await;
        assert!(result.is_ok());
        
        let call_result = result.unwrap();
        let content = call_result.content;
        let json_text = content[0].raw.as_text().unwrap().text.as_str();
        let response: CalcTaxResponse = serde_json::from_str(json_text).unwrap();
        
        // Uses configured defaults: thresholds=[10000], rates=[0.10,0.20]
        // surcharge_threshold=5000, surcharge_rate=0.02
        // Expected: 10000 * 0.10 + 40000 * 0.20 = 1000 + 8000 = 9000
        // Surcharge: 9000 > 5000, so 9000 + (9000 * 0.02) = 9000 + 180 = 9,180
        assert_eq!(response.tax, 9180.0);
        assert!(response.errors.is_empty());
    }

    #[tokio::test]
    async fn test_string_parsing_with_commas() {
        let engine = CompatibilityEngine::new();
        let params = CalcTaxParams {
            income: "40,000.00".to_string(), // Test comma-separated thousands
        };
        
        let result = engine.calc_tax(Parameters(params)).await;
        assert!(result.is_ok());
        
        let call_result = result.unwrap();
        assert!(!call_result.is_error.unwrap_or(false));
        let content = call_result.content;
        let json_text = content[0].raw.as_text().unwrap().text.as_str();
        let response: CalcTaxResponse = serde_json::from_str(json_text).unwrap();
        
        // Should parse as 40000.0 and give same result
        assert_eq!(response.tax, 7140.0);
        assert!(response.errors.is_empty());
    }

    #[tokio::test]
    async fn test_string_parsing_with_dollar_sign() {
        let engine = CompatibilityEngine::new();
        let params = DistributeWaterfallParams {
            cash_available: "$15,000,000".to_string(), // Test dollar sign and commas
            senior_debt: "$8000000".to_string(),
            junior_debt: "$10,000,000.00".to_string(),
        };
        
        let result = engine.distribute_waterfall(Parameters(params)).await;
        assert!(result.is_ok());
        
        let call_result = result.unwrap();
        assert!(!call_result.is_error.unwrap_or(false));
        let content = call_result.content;
        let json_text = content[0].raw.as_text().unwrap().text.as_str();
        let response: DistributeWaterfallResponse = serde_json::from_str(json_text).unwrap();
        
        // Should parse correctly and give expected result
        assert_eq!(response.distribution.senior, 8_000_000.0);
        assert_eq!(response.distribution.junior, 7_000_000.0);
        assert_eq!(response.distribution.equity, 0.0);
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
    async fn test_string_parsing_empty_string() {
        let engine = CompatibilityEngine::new();
        let params = CheckVotingParams {
            eligible_voters: "".to_string(), // Empty string
            turnout: "70".to_string(),
            yes_votes: "55".to_string(),
            proposal_type: "general".to_string(),
        };
        
        let result = engine.check_voting(Parameters(params)).await;
        assert!(result.is_ok());
        
        let call_result = result.unwrap();
        assert!(call_result.is_error.unwrap_or(false));
        let content = call_result.content;
        let error_text = content[0].raw.as_text().unwrap().text.as_str();
        assert!(error_text.contains("Invalid eligible_voters parameter"));
        assert!(error_text.contains("Empty string cannot be parsed"));
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
    async fn test_boolean_parsing_variations() {
        let engine = CompatibilityEngine::new();
        
        // Test various "true" representations
        for true_value in ["true", "TRUE", "True", "t", "T", "yes", "YES", "y", "Y", "1", "on", "ON"] {
            let params = CheckHousingGrantParams {
                ami: "50000".to_string(),
                household_size: "3".to_string(),
                income: "25000".to_string(), // Same qualifying income as false test
                has_other_subsidy: true_value.to_string(),
            };
            
            let result = engine.check_housing_grant(Parameters(params)).await;
            assert!(result.is_ok());
            
            let call_result = result.unwrap();
            assert!(!call_result.is_error.unwrap_or(false));
            let content = call_result.content;
            let json_text = content[0].raw.as_text().unwrap().text.as_str();
            let response: CheckHousingGrantResponse = serde_json::from_str(json_text).unwrap();
            
            // Should be ineligible due to having other subsidy (true)
            assert_eq!(response.eligible, false);
            assert!(response.explanation.contains("already has another subsidy"));
        }
        
        // Test various "false" representations
        for false_value in ["false", "FALSE", "False", "f", "F", "no", "NO", "n", "N", "0", "off", "OFF"] {
            let params = CheckHousingGrantParams {
                ami: "50000".to_string(),
                household_size: "3".to_string(),
                income: "25000".to_string(), // Set income below threshold (0.60 * 50000 = 30000)
                has_other_subsidy: false_value.to_string(),
            };
            
            let result = engine.check_housing_grant(Parameters(params)).await;
            assert!(result.is_ok());
            
            let call_result = result.unwrap();
            assert!(!call_result.is_error.unwrap_or(false));
            let content = call_result.content;
            let json_text = content[0].raw.as_text().unwrap().text.as_str();
            let response: CheckHousingGrantResponse = serde_json::from_str(json_text).unwrap();
            
            // Should be eligible (no other subsidy + income qualifies)
            assert_eq!(response.eligible, true);
        }
    }

    #[tokio::test]
    async fn test_boolean_parsing_invalid() {
        let engine = CompatibilityEngine::new();
        let params = CheckHousingGrantParams {
            ami: "50000".to_string(),
            household_size: "3".to_string(),
            income: "32000".to_string(),
            has_other_subsidy: "maybe".to_string(), // Invalid boolean
        };
        
        let result = engine.check_housing_grant(Parameters(params)).await;
        assert!(result.is_ok());
        
        let call_result = result.unwrap();
        assert!(call_result.is_error.unwrap_or(false));
        let content = call_result.content;
        let error_text = content[0].raw.as_text().unwrap().text.as_str();
        
        assert!(error_text.contains("Invalid has_other_subsidy parameter"));
        assert!(error_text.contains("Cannot parse 'maybe' as a boolean"));
    }

    #[tokio::test]
    async fn test_boolean_parsing_empty_string() {
        let engine = CompatibilityEngine::new();
        let params = CheckHousingGrantParams {
            ami: "50000".to_string(),
            household_size: "3".to_string(),
            income: "32000".to_string(),
            has_other_subsidy: "".to_string(), // Empty string
        };
        
        let result = engine.check_housing_grant(Parameters(params)).await;
        assert!(result.is_ok());
        
        let call_result = result.unwrap();
        assert!(call_result.is_error.unwrap_or(false));
        let content = call_result.content;
        let error_text = content[0].raw.as_text().unwrap().text.as_str();
        
        assert!(error_text.contains("Invalid has_other_subsidy parameter"));
        assert!(error_text.contains("Empty string cannot be parsed as boolean"));
    }

    #[tokio::test]
    async fn test_llm_generated_boolean_strings() {
        let engine = CompatibilityEngine::new();
        
        // Simulate the exact error scenario from the terminal log:
        // "has_other_subsidy": String("true") instead of boolean true
        let params = CheckHousingGrantParams {
            ami: "65000".to_string(),
            household_size: "7".to_string(),
            income: "40000".to_string(),
            has_other_subsidy: "true".to_string(), // This was causing the original error
        };
        
        let result = engine.check_housing_grant(Parameters(params)).await;
        assert!(result.is_ok());
        
        let call_result = result.unwrap();
        assert!(!call_result.is_error.unwrap_or(false)); // Should NOT be an error anymore
        let content = call_result.content;
        let json_text = content[0].raw.as_text().unwrap().text.as_str();
        let response: CheckHousingGrantResponse = serde_json::from_str(json_text).unwrap();
        
        // Should be ineligible due to having other subsidy
        assert_eq!(response.eligible, false);
        assert!(response.explanation.contains("already has another subsidy"));
    }

    #[tokio::test]
    async fn test_native_json_types() {
        // Test that we can deserialize native JSON types directly
        let json_data = r#"{
            "ami": 65000,
            "household_size": 7,
            "income": 40000,
            "has_other_subsidy": true
        }"#;
        
        let params: CheckHousingGrantParams = serde_json::from_str(json_data).unwrap();
        
        // Should have been converted to strings internally
        assert_eq!(params.ami, "65000");
        assert_eq!(params.household_size, "7");
        assert_eq!(params.income, "40000");
        assert_eq!(params.has_other_subsidy, "true");
        
        // Test that the engine can process these
        let engine = CompatibilityEngine::new();
        let result = engine.check_housing_grant(Parameters(params)).await;
        assert!(result.is_ok());
        
        let call_result = result.unwrap();
        assert!(!call_result.is_error.unwrap_or(false));
    }

    #[tokio::test]
    async fn test_mixed_types() {
        // Test mixing native types and strings
        let json_data = r#"{
            "ami": "65000",
            "household_size": 7,
            "income": 40000.5,
            "has_other_subsidy": "false"
        }"#;
        
        let params: CheckHousingGrantParams = serde_json::from_str(json_data).unwrap();
        
        assert_eq!(params.ami, "65000");
        assert_eq!(params.household_size, "7");
        assert_eq!(params.income, "40000.5");
        assert_eq!(params.has_other_subsidy, "false");
    }

    #[tokio::test]
    async fn test_all_parameter_types_with_numbers() {
        // Test CalcPenaltyParams with native number
        let json_penalty = r#"{"days_late": 12.5}"#;
        let penalty_params: CalcPenaltyParams = serde_json::from_str(json_penalty).unwrap();
        assert_eq!(penalty_params.days_late, "12.5");
        
        // Test CalcTaxParams with native number
        let json_tax = r#"{"income": 50000}"#;
        let tax_params: CalcTaxParams = serde_json::from_str(json_tax).unwrap();
        assert_eq!(tax_params.income, "50000");
        
        // Test CheckVotingParams with native numbers
        let json_voting = r#"{
            "eligible_voters": 100,
            "turnout": 75,
            "yes_votes": 60,
            "proposal_type": "amendment"
        }"#;
        let voting_params: CheckVotingParams = serde_json::from_str(json_voting).unwrap();
        assert_eq!(voting_params.eligible_voters, "100");
        assert_eq!(voting_params.turnout, "75");
        assert_eq!(voting_params.yes_votes, "60");
        
        // Test DistributeWaterfallParams with native numbers
        let json_waterfall = r#"{
            "cash_available": 15000000.0,
            "senior_debt": 8000000,
            "junior_debt": 10000000.5
        }"#;
        let waterfall_params: DistributeWaterfallParams = serde_json::from_str(json_waterfall).unwrap();
        assert_eq!(waterfall_params.cash_available, "15000000");
        assert_eq!(waterfall_params.senior_debt, "8000000");
        assert_eq!(waterfall_params.junior_debt, "10000000.5");
    }

    #[tokio::test]
    async fn test_float_to_int_conversion_error() {
        // Test that floats are rejected for integer fields
        let json_data = r#"{
            "eligible_voters": 100.5,
            "turnout": 75,
            "yes_votes": 60,
            "proposal_type": "amendment"
        }"#;
        
        let result = serde_json::from_str::<CheckVotingParams>(json_data);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Expected integer, got float"));
    }

    #[tokio::test]
    async fn test_end_to_end_with_native_types() {
        let engine = CompatibilityEngine::new();
        
        // Simulate the exact payload from the terminal log that was failing
        let json_data = r#"{
            "ami": 65000,
            "has_other_subsidy": true,
            "household_size": 7,
            "income": 40000
        }"#;
        
        let params: CheckHousingGrantParams = serde_json::from_str(json_data).unwrap();
        let result = engine.check_housing_grant(Parameters(params)).await;
        
        assert!(result.is_ok());
        let call_result = result.unwrap();
        assert!(!call_result.is_error.unwrap_or(false)); // Should NOT error anymore
        
        let content = call_result.content;
        let json_text = content[0].raw.as_text().unwrap().text.as_str();
        let response: CheckHousingGrantResponse = serde_json::from_str(json_text).unwrap();
        
        // Should be ineligible due to having subsidy
        assert_eq!(response.eligible, false);
    }

    #[test]
    fn test_exact_terminal_log_scenario() {
        // Test the exact JSON structure that was failing in the terminal log  
        // (excluding session_id which is not part of the parameter struct)
        let json_data = r#"{
            "ami": 65000,
            "has_other_subsidy": true,
            "household_size": 7,
            "income": 40000
        }"#;
        
        // This should now deserialize successfully
        let params: Result<CheckHousingGrantParams, _> = serde_json::from_str(json_data);
        assert!(params.is_ok());
        
        let params = params.unwrap();
        assert_eq!(params.ami, "65000");
        assert_eq!(params.has_other_subsidy, "true");
        assert_eq!(params.household_size, "7");
        assert_eq!(params.income, "40000");
    }

    #[test]
    fn test_scenario_2_from_terminal_log() {
        // Test the second failing scenario
        let json_data = r#"{
            "ami": 55000,
            "has_other_subsidy": false,
            "household_size": 2,
            "income": 32000
        }"#;
        
        let params: Result<CheckHousingGrantParams, _> = serde_json::from_str(json_data);
        assert!(params.is_ok());
        
        let params = params.unwrap();
        assert_eq!(params.ami, "55000");
        assert_eq!(params.has_other_subsidy, "false");
        assert_eq!(params.household_size, "2");
        assert_eq!(params.income, "32000");
    }
}
