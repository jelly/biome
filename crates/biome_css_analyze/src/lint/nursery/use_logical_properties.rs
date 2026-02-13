use crate::CssRuleAction;
use biome_analyze::{
    context::RuleContext, declare_lint_rule, Ast, FixKind, Rule, RuleDiagnostic, RuleSource,
};
use biome_console::markup;
use biome_css_factory::make;
use biome_css_syntax::{
    AnyCssDeclaration, AnyCssDeclarationName, AnyCssDeclarationOrRuleBlock,
    CssDeclarationOrRuleBlock, CssGenericProperty, CssSyntaxKind, CssSyntaxToken,
};
use biome_diagnostics::Severity;
use biome_rowan::{AstNode, AstNodeList, BatchMutationExt, SyntaxElement};
use biome_rule_options::use_logical_properties::UseLogicalPropertiesOptions;

declare_lint_rule! {
    /// Enforce the use of CSS logical properties and values over their physical counterparts.
    ///
    /// CSS logical properties and values use abstract terms like "block" and "inline" to describe direction,
    /// rather than physical terms like "top", "right", "bottom", and "left".
    /// This makes your CSS more adaptable to different writing modes and text directions (e.g., RTL languages).
    ///
    /// This rule promotes the following replacements:
    ///
    /// - Physical margin/padding properties → Logical equivalents (e.g., `margin-left` → `margin-inline-start`)
    /// - Physical inset properties → Logical equivalents (e.g., `left` → `inset-inline-start`)
    /// - Physical size properties → Logical equivalents (e.g., `width` → `inline-size`)
    /// - Physical border properties → Logical equivalents (e.g., `border-left` → `border-inline-start`)
    /// - Physical border-radius properties → Logical equivalents (e.g., `border-top-left-radius` → `border-start-start-radius`)
    ///
    /// When both properties of a pair exist (e.g., `margin-left` and `margin-right`), they will be combined
    /// into a shorthand logical property (e.g., `margin-inline: <right> <left>`).
    ///
    /// ## Examples
    ///
    /// ### Invalid
    ///
    /// ```css,expect_diagnostic
    /// .margin { margin-left: 10px; }
    /// ```
    ///
    /// ```css,expect_diagnostic
    /// .padding { padding-right: 10px; }
    /// ```
    ///
    /// ```css,expect_diagnostic
    /// .inset { top: 10px; }
    /// ```
    ///
    /// ```css,expect_diagnostic
    /// .size { width: 100px; }
    /// ```
    ///
    /// ```css,expect_diagnostic
    /// .border { border-left: 1px solid black; }
    /// ```
    ///
    /// ```css,expect_diagnostic
    /// .radius { border-top-left-radius: 4px; }
    /// ```
    ///
    /// ### Valid
    ///
    /// ```css
    /// .margin { margin-inline-start: 10px; }
    /// ```
    ///
    /// ```css
    /// .padding { padding-inline-end: 10px; }
    /// ```
    ///
    /// ```css
    /// .inset { inset-block-start: 10px; }
    /// ```
    ///
    /// ```css
    /// .size { inline-size: 100px; }
    /// ```
    ///
    /// ```css
    /// .border { border-inline-start: 1px solid black; }
    /// ```
    ///
    /// ```css
    /// .radius { border-start-start-radius: 4px; }
    /// ```
    ///
    /// ```css
    /// .shorthand { margin-inline: 10px 20px; }
    /// ```
    ///
    pub UseLogicalProperties {
        version: "next",
        name: "useLogicalProperties",
        language: "css",
        recommended: false,
        severity: Severity::Warning,
        sources: &[RuleSource::Stylelint("stylistic/use-logical-properties-and-values").inspired()],
        fix_kind: FixKind::Safe,
    }
}

#[derive(Debug)]
pub struct UseLogicalPropertiesState {
    property_name: String,
    logical_equivalent: String,
    paired_property: Option<PairedPropertyInfo>,
}

#[derive(Debug)]
struct PairedPropertyInfo {
    pair_name: String,
    shorthand_name: String,
    is_inline: bool, // true for inline (left/right), false for block (top/bottom)
}

impl Rule for UseLogicalProperties {
    type Query = Ast<CssGenericProperty>;
    type State = UseLogicalPropertiesState;
    type Signals = Option<Self::State>;
    type Options = UseLogicalPropertiesOptions;

    fn run(ctx: &RuleContext<Self>) -> Self::Signals {
        let node = ctx.query();
        let name_node = node.name().ok()?;
        let property_name = name_node.syntax().text_trimmed().to_string();
        let property_name_lower = property_name.to_ascii_lowercase();

        // Map physical properties to their logical equivalents
        let logical_equivalent = get_logical_equivalent(&property_name_lower)?;

        // Check if this property has a pair that could be combined
        let paired_property = get_pair_info(&property_name_lower);

        Some(UseLogicalPropertiesState {
            property_name: property_name_lower,
            logical_equivalent: logical_equivalent.to_string(),
            paired_property,
        })
    }

    fn diagnostic(ctx: &RuleContext<Self>, state: &Self::State) -> Option<RuleDiagnostic> {
        Some(
            RuleDiagnostic::new(
                rule_category!(),
                ctx.query().range(),
                markup! {
                    "Use logical property \""<Emphasis>{state.logical_equivalent}</Emphasis>"\" instead of physical property \""<Emphasis>{state.property_name}</Emphasis>"\"."
                },
            )
            .note(markup! {
                "Logical properties improve internationalization and adapt to different writing modes."
            })
            .note(markup! {
                "See "<Hyperlink href="https://developer.mozilla.org/en-US/docs/Web/CSS/CSS_logical_properties_and_values">"MDN Web Docs on CSS Logical Properties"</Hyperlink>" for more information."
            }),
        )
    }

    fn action(ctx: &RuleContext<Self>, state: &Self::State) -> Option<CssRuleAction> {
        let node = ctx.query();
        let mut mutation = ctx.root().begin();

        // Try to find the paired property if it exists
        if let Some(ref pair_info) = state.paired_property {
            if let Some(paired_node) = find_paired_property(node, &pair_info.pair_name) {
                // Both properties exist - combine into shorthand
                return create_shorthand_action(
                    ctx,
                    &mut mutation,
                    node,
                    &paired_node,
                    &state.property_name,
                    &pair_info.pair_name,
                    &pair_info.shorthand_name,
                    pair_info.is_inline,
                );
            }
        }

        // No pair found - just replace with longhand
        let new_token = CssSyntaxToken::new_detached(
            CssSyntaxKind::IDENT,
            &state.logical_equivalent,
            [],
            [],
        );

        let new_identifier = make::css_identifier(new_token);
        let new_name = AnyCssDeclarationName::CssIdentifier(new_identifier);

        mutation.replace_node(node.name().ok()?, new_name);

        Some(CssRuleAction::new(
            ctx.metadata().action_category(ctx.category(), ctx.group()),
            ctx.metadata().applicability(),
            markup! { "Replace \""<Emphasis>{state.property_name}</Emphasis>"\" with \""<Emphasis>{state.logical_equivalent}</Emphasis>"\"" }
                .to_owned(),
            mutation,
        ))
    }
}

/// Find the paired property in the same declaration block
fn find_paired_property(
    node: &CssGenericProperty,
    paired_property_name: &str,
) -> Option<CssGenericProperty> {
    let declaration_block: CssDeclarationOrRuleBlock = node
        .syntax()
        .ancestors()
        .find_map(|ancestor| {
            AnyCssDeclarationOrRuleBlock::cast_ref(&ancestor).and_then(|block| {
                block.as_css_declaration_or_rule_block().cloned()
            })
        })?;

    let items = declaration_block.items();

    for item in items.iter() {
        // Check if this is a declaration with semicolon (not a rule)
        if let Some(decl_with_semi) = item.as_css_declaration_with_semicolon() {
            if let Ok(css_declaration) = decl_with_semi.declaration() {
                if let Ok(property) = css_declaration.property() {
                    if let Some(generic_prop) = property.as_css_generic_property() {
                        if let Ok(name) = generic_prop.name() {
                            let prop_name = name.syntax().text_trimmed().to_string();
                            if prop_name.to_ascii_lowercase() == paired_property_name {
                                return Some(generic_prop.clone());
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

/// Create a shorthand property action that combines two properties
fn create_shorthand_action(
    ctx: &RuleContext<UseLogicalProperties>,
    mutation: &mut biome_rowan::BatchMutation<biome_css_syntax::CssLanguage>,
    first_prop: &CssGenericProperty,
    second_prop: &CssGenericProperty,
    first_prop_name: &str,
    second_prop_name: &str,
    shorthand_name: &str,
    is_inline: bool,
) -> Option<CssRuleAction> {
    // Extract values from both properties
    let first_value = first_prop.value();
    let second_value = second_prop.value();

    // Determine the correct order for the shorthand values
    // For inline: <right> <left>
    // For block: <top> <bottom>
    let (first_val, second_val) = if is_inline {
        // Inline properties: right comes before left in the shorthand
        if first_prop_name.contains("left") {
            (&second_value, &first_value)
        } else {
            (&first_value, &second_value)
        }
    } else {
        // Block properties: top comes before bottom in the shorthand
        if first_prop_name.contains("top") {
            (&first_value, &second_value)
        } else {
            (&second_value, &first_value)
        }
    };

    // Build a new value list by combining both values with a space separator
    let mut combined_elements: Vec<Option<SyntaxElement<biome_css_syntax::CssLanguage>>> = Vec::new();

    // Add all elements from the first value
    for element in first_val.iter() {
        combined_elements.push(Some(SyntaxElement::Node(element.syntax().clone())));
    }

    // Add a whitespace separator
    let whitespace = CssSyntaxToken::new_detached(
        CssSyntaxKind::WHITESPACE,
        " ",
        [],
        [],
    );
    combined_elements.push(Some(SyntaxElement::Token(whitespace)));

    // Add all elements from the second value
    for element in second_val.iter() {
        combined_elements.push(Some(SyntaxElement::Node(element.syntax().clone())));
    }

    // Create a new value list node
    use biome_rowan::SyntaxNode;
    let new_value_node = SyntaxNode::new_detached(
        CssSyntaxKind::CSS_GENERIC_COMPONENT_VALUE_LIST,
        combined_elements,
    );
    let new_value = biome_css_syntax::CssGenericComponentValueList::unwrap_cast(new_value_node);

    // Create the new shorthand property name token
    let name_token = CssSyntaxToken::new_detached(CssSyntaxKind::IDENT, shorthand_name, [], []);
    let new_name = make::css_identifier(name_token);
    let new_name = AnyCssDeclarationName::CssIdentifier(new_name);

    // Replace the first property's name and value
    mutation.replace_node(first_prop.name().ok()?, new_name);
    mutation.replace_node(first_value, new_value);

    // Remove the second property entirely
    if let Some(second_decl_with_semi) = second_prop.syntax().ancestors().find_map(|ancestor| {
        AnyCssDeclaration::cast_ref(&ancestor)
            .and_then(|decl| decl.as_css_declaration_with_semicolon().cloned())
    }) {
        mutation.remove_node(second_decl_with_semi);
    }

    Some(CssRuleAction::new(
        ctx.metadata().action_category(ctx.category(), ctx.group()),
        ctx.metadata().applicability(),
        markup! {
            "Combine \""<Emphasis>{first_prop_name}</Emphasis>"\" and \""<Emphasis>{second_prop_name}</Emphasis>"\" into \""<Emphasis>{shorthand_name}</Emphasis>"\""
        }
        .to_owned(),
        mutation.clone(),
    ))
}

/// Get the logical equivalent for a physical property
fn get_logical_equivalent(property: &str) -> Option<&'static str> {
    match property {
        // Margin properties
        "margin-left" => Some("margin-inline-start"),
        "margin-right" => Some("margin-inline-end"),
        "margin-top" => Some("margin-block-start"),
        "margin-bottom" => Some("margin-block-end"),

        // Padding properties
        "padding-left" => Some("padding-inline-start"),
        "padding-right" => Some("padding-inline-end"),
        "padding-top" => Some("padding-block-start"),
        "padding-bottom" => Some("padding-block-end"),

        // Inset properties
        "left" => Some("inset-inline-start"),
        "right" => Some("inset-inline-end"),
        "top" => Some("inset-block-start"),
        "bottom" => Some("inset-block-end"),

        // Size properties
        "width" => Some("inline-size"),
        "min-width" => Some("min-inline-size"),
        "max-width" => Some("max-inline-size"),
        "height" => Some("block-size"),
        "min-height" => Some("min-block-size"),
        "max-height" => Some("max-block-size"),

        // Border properties
        "border-left" => Some("border-inline-start"),
        "border-right" => Some("border-inline-end"),
        "border-top" => Some("border-block-start"),
        "border-bottom" => Some("border-block-end"),

        // Border width
        "border-left-width" => Some("border-inline-start-width"),
        "border-right-width" => Some("border-inline-end-width"),
        "border-top-width" => Some("border-block-start-width"),
        "border-bottom-width" => Some("border-block-end-width"),

        // Border style
        "border-left-style" => Some("border-inline-start-style"),
        "border-right-style" => Some("border-inline-end-style"),
        "border-top-style" => Some("border-block-start-style"),
        "border-bottom-style" => Some("border-block-end-style"),

        // Border color
        "border-left-color" => Some("border-inline-start-color"),
        "border-right-color" => Some("border-inline-end-color"),
        "border-top-color" => Some("border-block-start-color"),
        "border-bottom-color" => Some("border-block-end-color"),

        // Border radius
        "border-top-left-radius" => Some("border-start-start-radius"),
        "border-top-right-radius" => Some("border-start-end-radius"),
        "border-bottom-left-radius" => Some("border-end-start-radius"),
        "border-bottom-right-radius" => Some("border-end-end-radius"),

        _ => None,
    }
}

/// Get pairing information for properties that can be combined into shorthands
fn get_pair_info(property: &str) -> Option<PairedPropertyInfo> {
    match property {
        // Margin inline pairs
        "margin-left" | "margin-right" => Some(PairedPropertyInfo {
            pair_name: if property == "margin-left" {
                "margin-right"
            } else {
                "margin-left"
            }
            .to_string(),
            shorthand_name: "margin-inline".to_string(),
            is_inline: true,
        }),

        // Margin block pairs
        "margin-top" | "margin-bottom" => Some(PairedPropertyInfo {
            pair_name: if property == "margin-top" {
                "margin-bottom"
            } else {
                "margin-top"
            }
            .to_string(),
            shorthand_name: "margin-block".to_string(),
            is_inline: false,
        }),

        // Padding inline pairs
        "padding-left" | "padding-right" => Some(PairedPropertyInfo {
            pair_name: if property == "padding-left" {
                "padding-right"
            } else {
                "padding-left"
            }
            .to_string(),
            shorthand_name: "padding-inline".to_string(),
            is_inline: true,
        }),

        // Padding block pairs
        "padding-top" | "padding-bottom" => Some(PairedPropertyInfo {
            pair_name: if property == "padding-top" {
                "padding-bottom"
            } else {
                "padding-top"
            }
            .to_string(),
            shorthand_name: "padding-block".to_string(),
            is_inline: false,
        }),

        // Inset inline pairs
        "left" | "right" => Some(PairedPropertyInfo {
            pair_name: if property == "left" {
                "right"
            } else {
                "left"
            }
            .to_string(),
            shorthand_name: "inset-inline".to_string(),
            is_inline: true,
        }),

        // Inset block pairs
        "top" | "bottom" => Some(PairedPropertyInfo {
            pair_name: if property == "top" {
                "bottom"
            } else {
                "top"
            }
            .to_string(),
            shorthand_name: "inset-block".to_string(),
            is_inline: false,
        }),

        _ => None,
    }
}
