//! Per-property cascade and inherited values for the future box tree.
//!
//! This is separate from the older reader's visibility approximation. A box
//! renderer must ask for computed values on each DOM element, not flatten
//! markup into paragraphs before applying CSS.

/// Properties the first box-tree phase understands. More properties belong
/// here as their parsing and used-value rules are implemented and tested.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Property {
    Display,
    Color,
    Width,
    Height,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Display {
    None,
    Block,
    Inline,
    InlineBlock,
    ListItem,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Value {
    Display(Display),
    /// Opaque sRGB. Painting on a monochrome panel converts this later.
    Color(u32),
    Width(Length),
    Height(Length),
    Inherit,
    Initial,
    Unset,
    Revert,
}

/// CSS width before resolving a percentage against the containing block.
/// Percent uses hundredths of one percent to avoid floats on the device.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Length {
    Auto,
    Px(u32),
    Percent(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Origin {
    UserAgent,
    User,
    Author,
}

/// A selector-matched declaration, retaining cascade metadata. Layers and
/// animations are not represented here yet and must not silently enter this
/// path until their precedence rules are implemented.
#[derive(Clone, Copy, Debug)]
pub struct Declaration {
    pub property: Property,
    pub value: Value,
    pub origin: Origin,
    pub important: bool,
    /// Selector specificity is lexicographic (ID, class, type), not decimal.
    pub specificity: (u16, u16, u16),
    pub source_order: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Computed {
    pub display: Display,
    pub color: u32,
    pub width: Length,
    pub height: Length,
}

impl Computed {
    pub const INITIAL: Self = Self {
        display: Display::Inline,
        color: 0x00_00_00,
        width: Length::Auto,
        height: Length::Auto,
    };

    #[must_use]
    pub fn cascade(parent: Option<Self>, declarations: &[Declaration]) -> Self {
        let initial = Self::INITIAL;
        let mut computed = Self {
            display: initial.display,
            color: parent.unwrap_or(initial).color,
            width: initial.width,
            height: initial.height,
        };
        for property in [
            Property::Display,
            Property::Color,
            Property::Width,
            Property::Height,
        ] {
            let chosen = declarations
                .iter()
                .filter(|decl| decl.property == property)
                .max_by_key(|decl| {
                    (
                        precedence(decl.origin, decl.important),
                        decl.specificity,
                        decl.source_order,
                    )
                });
            let fallback = match property {
                Property::Display => Value::Display(initial.display),
                Property::Color => Value::Color(parent.unwrap_or(initial).color),
                Property::Width => Value::Width(initial.width),
                Property::Height => Value::Height(initial.height),
            };
            let value = chosen.map_or(fallback, |decl| {
                resolve(
                    decl.value,
                    property,
                    parent,
                    initial,
                    decl.origin,
                    declarations,
                )
            });
            match value {
                Value::Display(display) if property == Property::Display => {
                    computed.display = display;
                }
                Value::Color(color) if property == Property::Color => computed.color = color,
                Value::Width(width) if property == Property::Width => computed.width = width,
                Value::Height(height) if property == Property::Height => computed.height = height,
                _ => unreachable!("resolved property value has the wrong type"),
            }
        }
        computed
    }
}

fn precedence(origin: Origin, important: bool) -> u8 {
    match (origin, important) {
        (Origin::UserAgent, false) => 0,
        (Origin::User, false) => 1,
        (Origin::Author, false) => 2,
        (Origin::Author, true) => 3,
        (Origin::User, true) => 4,
        (Origin::UserAgent, true) => 5,
    }
}

fn resolve(
    value: Value,
    property: Property,
    parent: Option<Computed>,
    initial: Computed,
    origin: Origin,
    declarations: &[Declaration],
) -> Value {
    let initial_value = match property {
        Property::Display => Value::Display(initial.display),
        Property::Color => Value::Color(initial.color),
        Property::Width => Value::Width(initial.width),
        Property::Height => Value::Height(initial.height),
    };
    let inherited = match property {
        Property::Display => Value::Display(parent.unwrap_or(initial).display),
        Property::Color => Value::Color(parent.unwrap_or(initial).color),
        Property::Width => Value::Width(parent.unwrap_or(initial).width),
        Property::Height => Value::Height(parent.unwrap_or(initial).height),
    };
    match value {
        Value::Inherit => inherited,
        Value::Initial => initial_value,
        Value::Unset => match property {
            Property::Display | Property::Width | Property::Height => initial_value,
            Property::Color => inherited,
        },
        Value::Revert => {
            // Revert crosses the *origin* boundary, even for important values.
            // For author declarations the next lower origin is user or UA;
            // user declarations fall back to UA. UA revert uses initial or
            // inherited value. A layer-specific revert-layer is distinct.
            let lower = declarations
                .iter()
                .filter(|decl| {
                    decl.property == property && origin_rank(decl.origin) < origin_rank(origin)
                })
                .max_by_key(|decl| {
                    (
                        precedence(decl.origin, decl.important),
                        decl.specificity,
                        decl.source_order,
                    )
                });
            lower.map_or_else(
                || {
                    if property == Property::Color {
                        inherited
                    } else {
                        initial_value
                    }
                },
                |decl| {
                    resolve(
                        decl.value,
                        property,
                        parent,
                        initial,
                        decl.origin,
                        declarations,
                    )
                },
            )
        }
        concrete => concrete,
    }
}

fn origin_rank(origin: Origin) -> u8 {
    match origin {
        Origin::UserAgent => 0,
        Origin::User => 1,
        Origin::Author => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declaration(property: Property, value: Value, origin: Origin, order: u32) -> Declaration {
        Declaration {
            property,
            value,
            origin,
            important: false,
            specificity: (0, 1, 0),
            source_order: order,
        }
    }

    #[test]
    fn display_is_not_inherited_but_color_is() {
        let parent = Computed {
            display: Display::Block,
            color: 0x12_34_56,
            width: Length::Px(42),
            height: Length::Auto,
        };
        assert_eq!(
            Computed::cascade(Some(parent), &[]),
            Computed {
                display: Display::Inline,
                color: parent.color,
                width: Length::Auto,
                height: Length::Auto
            }
        );
    }

    #[test]
    fn width_defaults_auto_and_inherits_only_when_requested() {
        let parent = Computed {
            width: Length::Percent(3750),
            ..Computed::INITIAL
        };
        assert_eq!(Computed::cascade(Some(parent), &[]).width, Length::Auto);
        for (value, expected) in [
            (Value::Inherit, parent.width),
            (Value::Initial, Length::Auto),
            (Value::Unset, Length::Auto),
            (Value::Width(Length::Px(180)), Length::Px(180)),
        ] {
            let got = Computed::cascade(
                Some(parent),
                &[declaration(Property::Width, value, Origin::Author, 0)],
            );
            assert_eq!(got.width, expected);
        }
    }

    #[test]
    fn height_is_not_inherited_without_a_declaration() {
        let parent = Computed {
            height: Length::Px(240),
            ..Computed::INITIAL
        };
        assert_eq!(Computed::cascade(Some(parent), &[]).height, Length::Auto);
        for (value, expected) in [
            (Value::Inherit, Length::Px(240)),
            (Value::Initial, Length::Auto),
            (Value::Unset, Length::Auto),
            (Value::Height(Length::Percent(5000)), Length::Percent(5000)),
        ] {
            assert_eq!(
                Computed::cascade(
                    Some(parent),
                    &[declaration(Property::Height, value, Origin::Author, 0)]
                )
                .height,
                expected
            );
        }
    }

    #[test]
    fn importance_origin_specificity_and_order_are_distinct() {
        let mut rules = vec![
            declaration(
                Property::Display,
                Value::Display(Display::None),
                Origin::Author,
                3,
            ),
            declaration(
                Property::Display,
                Value::Display(Display::Block),
                Origin::Author,
                4,
            ),
        ];
        assert_eq!(Computed::cascade(None, &rules).display, Display::Block);
        rules[0].specificity = (1, 0, 0);
        assert_eq!(Computed::cascade(None, &rules).display, Display::None);
        rules[1].important = true;
        assert_eq!(Computed::cascade(None, &rules).display, Display::Block);
        rules.push(Declaration {
            origin: Origin::User,
            important: true,
            ..rules[0]
        });
        assert_eq!(Computed::cascade(None, &rules).display, Display::None);
        rules.push(Declaration {
            origin: Origin::UserAgent,
            important: true,
            value: Value::Display(Display::ListItem),
            ..rules[0]
        });
        assert_eq!(Computed::cascade(None, &rules).display, Display::ListItem);
    }

    #[test]
    fn css_wide_keywords_resolve_per_property() {
        let parent = Computed {
            display: Display::Block,
            color: 0x22_33_44,
            width: Length::Px(42),
            height: Length::Auto,
        };
        for (keyword, expected_display, expected_color) in [
            (Value::Inherit, Display::Block, parent.color),
            (Value::Initial, Display::Inline, 0),
            (Value::Unset, Display::Inline, parent.color),
        ] {
            let declarations = [
                declaration(Property::Display, keyword, Origin::Author, 0),
                declaration(Property::Color, keyword, Origin::Author, 1),
            ];
            assert_eq!(
                Computed::cascade(Some(parent), &declarations),
                Computed {
                    display: expected_display,
                    color: expected_color,
                    width: Length::Auto,
                    height: Length::Auto
                }
            );
        }
    }

    #[test]
    fn revert_uses_lower_origin_not_earlier_same_origin() {
        let rules = [
            declaration(
                Property::Color,
                Value::Color(0x11_11_11),
                Origin::UserAgent,
                0,
            ),
            declaration(Property::Color, Value::Color(0x22_22_22), Origin::User, 1),
            declaration(Property::Color, Value::Color(0x33_33_33), Origin::Author, 2),
            declaration(Property::Color, Value::Revert, Origin::Author, 3),
        ];
        assert_eq!(Computed::cascade(None, &rules).color, 0x22_22_22);
    }
}
