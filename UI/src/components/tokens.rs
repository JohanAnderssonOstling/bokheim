use serde::Deserialize;
use std::sync::LazyLock;

#[derive(Clone, Copy, Deserialize)]
pub struct SpacingTokens {
    pub xs: i32,
    pub sm: i32,
    pub md: i32,
    pub lg: i32,
    pub xl: i32,
}

#[derive(Clone, Copy, Deserialize)]
pub struct RadiusTokens {
    pub sm: i32,
    pub md: i32,
    pub lg: i32,
    pub xl: i32,
}

#[derive(Clone, Copy, Deserialize)]
pub struct SizeTokens {
    pub control_sm: i32,
    pub control_md: i32,
    pub nav_rail_width: i32,
    pub nav_item_height: i32,
    pub book_cover_width: i32,
    pub book_cover_height: i32,
}

#[derive(Clone, Copy, Deserialize)]
pub struct TypographyTokens {
    pub text_xs: i32,
    pub text_sm: i32,
    pub text_md: i32,
    pub text_lg: i32,
    pub title_md: i32,
    pub title_lg: i32,
}

#[derive(Clone, Copy, Deserialize)]
pub struct DesignTokens {
    pub spacing: SpacingTokens,
    pub radius: RadiusTokens,
    pub size: SizeTokens,
    pub typography: TypographyTokens,
}

#[derive(Deserialize)]
struct DesignContract {
    spacing: SpacingTokens,
    radius: RadiusTokens,
    size: SizeTokens,
    typography: TypographyTokens,
}

pub static TOKENS: LazyLock<DesignTokens> = LazyLock::new(|| {
    let contract: DesignContract = serde_json::from_str(include_str!("../../../FrontendContract/design_tokens.json")).expect("shared design tokens must be valid");
    DesignTokens { spacing: contract.spacing, radius: contract.radius, size: contract.size, typography: contract.typography }
});
