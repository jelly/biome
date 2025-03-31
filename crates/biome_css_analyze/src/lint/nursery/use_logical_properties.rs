use biome_analyze::{
    context::RuleContext, declare_lint_rule, Ast, Rule, RuleDiagnostic, RuleSourceKind,
};
use biome_console::markup;
use biome_css_syntax::{CssGenericComponentValueList, CssGenericProperty};
use biome_rowan::{AstNode, TextRange};
use biome_string_case::StrLikeExtension;

declare_lint_rule! {
    /// Enforce the use of logical properties.
    ///
    /// Put context and details about the rule.
    /// As a starting point, you can take the description of the corresponding _ESLint_ rule (if any).
    ///
    /// Try to stay consistent with the descriptions of implemented rules.
    ///
    /// Add a link to the corresponding stylelint rule (if any):
    ///
    /// ## Examples
    ///
    /// ### Invalid
    ///
    /// ```css,expect_diagnostic
    /// p {
    ///   margin-left: 10px;
    /// }
    /// ```
    ///
    /// ### Valid
    ///
    /// ```css
    /// p {
    ///   margin-inline-start 1px;
    /// }
    /// ```
    ///
    pub UseLogicalProperties {
        version: "next",
        name: "useLogicalProperties",
        language: "css",
        recommended: false,
        source_kind: RuleSourceKind::Inspired,
    }
}

pub const PHYSICAL_PROPERTIES: [&str; 4] = ["top", "right", "bottom", "left"];
pub const LOGICAL_PROPERTIES: [&str; 4] = [
    "inset-block-start",
    "inset-inline-end",
    "inset-block-end",
    "inset-inline-start",
];

pub const PHYSICAL_VALUES: [&str; 2] = ["left", "right"];
pub const LOGICAL_VALUES: [&str; 2] = ["start", "end"];

pub struct UseLogicalPropertiesState {
    range: TextRange,
    name: String,
    logical_name: String,
}

impl Rule for UseLogicalProperties {
    type Query = Ast<CssGenericProperty>;
    type State = UseLogicalPropertiesState;
    type Signals = Option<Self::State>;
    type Options = ();

    fn run(ctx: &RuleContext<Self>) -> Option<Self::State> {
        let node = ctx.query();
        let property_name = node.name().ok()?.to_trimmed_string();
        let property_name_lower = property_name.to_ascii_lowercase_cow();

        if let Some(pos) = PHYSICAL_PROPERTIES
            .iter()
            .position(|&x| x == property_name_lower.trim())
        {
            return Some(UseLogicalPropertiesState {
                range: node.name().ok()?.range(),
                name: property_name,
                logical_name: LOGICAL_PROPERTIES[pos].to_string(),
            });
        }

        let properties = node.value();
        if let Some(pos) = is_phyiscal_value(&properties) {
            return Some(UseLogicalPropertiesState {
                range: properties.into_iter().next().unwrap().range(),
                name: PHYSICAL_VALUES[pos].to_string(),
                logical_name: LOGICAL_VALUES[pos].to_string(),
            });
        }

        None
    }

    fn diagnostic(_: &RuleContext<Self>, state: &Self::State) -> Option<RuleDiagnostic> {
        let span = state.range;
        let name = &state.name;
        Some(
            RuleDiagnostic::new(
                rule_category!(),
                span,
                markup! {
                    "Unexpected physical property: "<Emphasis>{ name }</Emphasis>" "
                },
            )
           .note(markup! {
                "To resolve this issue, replace the physical property with the equivalent logical property: "<Emphasis>{ state.logical_name }</Emphasis>" "
            })
        )
    }
}

fn is_phyiscal_value(properties: &CssGenericComponentValueList) -> Option<usize> {
    if properties.into_iter().len() != 1 {
        return None;
    }

    properties
        .into_iter()
        .position(|p| PHYSICAL_VALUES.contains(&p.to_trimmed_string().as_str()))
}
