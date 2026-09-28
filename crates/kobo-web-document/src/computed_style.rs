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
    BoxSizing,
    MarginLeft,
    MarginRight,
    MarginTop,
    MarginBottom,
    Direction,
    FontSize,
    BackgroundColor,
    PaddingLeft,
    PaddingRight,
    PaddingTop,
    PaddingBottom,
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
    BoxSizing(BoxSizing),
    Margin(Margin),
    Direction(Direction),
    BackgroundColor(Option<u32>),
    Padding(u32),
    FontSize(u32),
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
pub enum BoxSizing {
    ContentBox,
    BorderBox,
}

/// Computed horizontal margin before resolving percentage against block width.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Margin {
    Auto,
    Px(i32),
    Percent(i32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    Ltr,
    Rtl,
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
    pub box_sizing: BoxSizing,
    pub margin_left: Margin,
    pub margin_right: Margin,
    pub margin_top: Margin,
    pub margin_bottom: Margin,
    pub direction: Direction,
    /// Computed CSS font-size in integer pixels. Relative sizes need an explicit parent.
    pub font_size: u32,
    /// Transparent unless explicitly painted; unlike foreground color, not inherited.
    pub background_color: Option<u32>,
    pub padding_left: u32,
    pub padding_right: u32,
    pub padding_top: u32,
    pub padding_bottom: u32,
}

impl Computed {
    pub const INITIAL: Self = Self {
        display: Display::Inline,
        color: 0x00_00_00,
        width: Length::Auto,
        height: Length::Auto,
        box_sizing: BoxSizing::ContentBox,
        margin_left: Margin::Px(0),
        margin_right: Margin::Px(0),
        margin_top: Margin::Px(0),
        margin_bottom: Margin::Px(0),
        direction: Direction::Ltr,
        font_size: 16,
        background_color: None,
        padding_left: 0,
        padding_right: 0,
        padding_top: 0,
        padding_bottom: 0,
    };

    #[must_use]
    #[allow(clippy::too_many_lines)] // Per-property cascade over a bounded declaration list.
    pub fn cascade(parent: Option<Self>, declarations: &[Declaration]) -> Self {
        let initial = Self::INITIAL;
        let mut computed = Self {
            display: initial.display,
            color: parent.unwrap_or(initial).color,
            width: initial.width,
            height: initial.height,
            box_sizing: initial.box_sizing,
            margin_left: initial.margin_left,
            margin_right: initial.margin_right,
            margin_top: initial.margin_top,
            margin_bottom: initial.margin_bottom,
            direction: parent.unwrap_or(initial).direction,
            font_size: parent.unwrap_or(initial).font_size,
            background_color: initial.background_color,
            padding_left: initial.padding_left,
            padding_right: initial.padding_right,
            padding_top: initial.padding_top,
            padding_bottom: initial.padding_bottom,
        };
        for property in [
            Property::Display,
            Property::Color,
            Property::Width,
            Property::Height,
            Property::BoxSizing,
            Property::MarginLeft,
            Property::MarginRight,
            Property::MarginTop,
            Property::MarginBottom,
            Property::Direction,
            Property::FontSize,
            Property::BackgroundColor,
            Property::PaddingLeft,
            Property::PaddingRight,
            Property::PaddingTop,
            Property::PaddingBottom,
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
                Property::BoxSizing => Value::BoxSizing(initial.box_sizing),
                Property::MarginLeft => Value::Margin(initial.margin_left),
                Property::MarginRight => Value::Margin(initial.margin_right),
                Property::MarginTop => Value::Margin(initial.margin_top),
                Property::MarginBottom => Value::Margin(initial.margin_bottom),
                Property::Direction => Value::Direction(parent.unwrap_or(initial).direction),
                Property::FontSize => Value::FontSize(parent.unwrap_or(initial).font_size),
                Property::BackgroundColor => Value::BackgroundColor(initial.background_color),
                Property::PaddingLeft => Value::Padding(initial.padding_left),
                Property::PaddingRight => Value::Padding(initial.padding_right),
                Property::PaddingTop => Value::Padding(initial.padding_top),
                Property::PaddingBottom => Value::Padding(initial.padding_bottom),
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
                Value::BoxSizing(sizing) if property == Property::BoxSizing => {
                    computed.box_sizing = sizing;
                }
                Value::Margin(margin) if property == Property::MarginLeft => {
                    computed.margin_left = margin;
                }
                Value::Margin(margin) if property == Property::MarginRight => {
                    computed.margin_right = margin;
                }
                Value::Margin(margin) if property == Property::MarginTop => {
                    computed.margin_top = margin;
                }
                Value::Margin(margin) if property == Property::MarginBottom => {
                    computed.margin_bottom = margin;
                }
                Value::Direction(direction) if property == Property::Direction => {
                    computed.direction = direction;
                }
                Value::FontSize(px) if property == Property::FontSize => computed.font_size = px,
                Value::BackgroundColor(color) if property == Property::BackgroundColor => {
                    computed.background_color = color;
                }
                Value::Padding(px) if property == Property::PaddingLeft => {
                    computed.padding_left = px;
                }
                Value::Padding(px) if property == Property::PaddingRight => {
                    computed.padding_right = px;
                }
                Value::Padding(px) if property == Property::PaddingTop => computed.padding_top = px,
                Value::Padding(px) if property == Property::PaddingBottom => {
                    computed.padding_bottom = px;
                }
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
        Property::BoxSizing => Value::BoxSizing(initial.box_sizing),
        Property::MarginLeft => Value::Margin(initial.margin_left),
        Property::MarginRight => Value::Margin(initial.margin_right),
        Property::MarginTop => Value::Margin(initial.margin_top),
        Property::MarginBottom => Value::Margin(initial.margin_bottom),
        Property::Direction => Value::Direction(initial.direction),
        Property::FontSize => Value::FontSize(initial.font_size),
        Property::BackgroundColor => Value::BackgroundColor(initial.background_color),
        Property::PaddingLeft => Value::Padding(initial.padding_left),
        Property::PaddingRight => Value::Padding(initial.padding_right),
        Property::PaddingTop => Value::Padding(initial.padding_top),
        Property::PaddingBottom => Value::Padding(initial.padding_bottom),
    };
    let inherited = match property {
        Property::Display => Value::Display(parent.unwrap_or(initial).display),
        Property::Color => Value::Color(parent.unwrap_or(initial).color),
        Property::Width => Value::Width(parent.unwrap_or(initial).width),
        Property::Height => Value::Height(parent.unwrap_or(initial).height),
        Property::BoxSizing => Value::BoxSizing(parent.unwrap_or(initial).box_sizing),
        Property::MarginLeft => Value::Margin(parent.unwrap_or(initial).margin_left),
        Property::MarginRight => Value::Margin(parent.unwrap_or(initial).margin_right),
        Property::MarginTop => Value::Margin(parent.unwrap_or(initial).margin_top),
        Property::MarginBottom => Value::Margin(parent.unwrap_or(initial).margin_bottom),
        Property::Direction => Value::Direction(parent.unwrap_or(initial).direction),
        Property::FontSize => Value::FontSize(parent.unwrap_or(initial).font_size),
        Property::BackgroundColor => {
            Value::BackgroundColor(parent.unwrap_or(initial).background_color)
        }
        Property::PaddingLeft => Value::Padding(parent.unwrap_or(initial).padding_left),
        Property::PaddingRight => Value::Padding(parent.unwrap_or(initial).padding_right),
        Property::PaddingTop => Value::Padding(parent.unwrap_or(initial).padding_top),
        Property::PaddingBottom => Value::Padding(parent.unwrap_or(initial).padding_bottom),
    };
    match value {
        Value::Inherit => inherited,
        Value::Initial => initial_value,
        Value::Unset => match property {
            Property::Display
            | Property::Width
            | Property::Height
            | Property::BoxSizing
            | Property::MarginLeft
            | Property::MarginRight
            | Property::MarginTop
            | Property::MarginBottom
            | Property::BackgroundColor
            | Property::PaddingLeft
            | Property::PaddingRight
            | Property::PaddingTop
            | Property::PaddingBottom => initial_value,
            Property::Color | Property::Direction | Property::FontSize => inherited,
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
                    if matches!(
                        property,
                        Property::Color | Property::Direction | Property::FontSize
                    ) {
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
            box_sizing: BoxSizing::ContentBox,
            margin_left: Margin::Px(0),
            margin_right: Margin::Px(0),
            margin_top: Margin::Px(0),
            margin_bottom: Margin::Px(0),
            direction: Direction::Ltr,
            font_size: 16,
            background_color: None,
            padding_left: 0,
            padding_right: 0,
            padding_top: 0,
            padding_bottom: 0,
        };
        assert_eq!(
            Computed::cascade(Some(parent), &[]),
            Computed {
                display: Display::Inline,
                color: parent.color,
                width: Length::Auto,
                height: Length::Auto,
                box_sizing: BoxSizing::ContentBox,
                margin_left: Margin::Px(0),
                margin_right: Margin::Px(0),
                margin_top: Margin::Px(0),
                margin_bottom: Margin::Px(0),
                direction: Direction::Ltr,
                font_size: 16,
                background_color: None,
                padding_left: 0,
                padding_right: 0,
                padding_top: 0,
                padding_bottom: 0
            }
        );
    }

    #[test]
    fn font_size_inherits_and_css_wide_keywords_keep_their_scope() {
        let parent = Computed {
            font_size: 24,
            ..Computed::INITIAL
        };
        assert_eq!(Computed::cascade(Some(parent), &[]).font_size, 24);
        for (value, expected) in [
            (Value::Inherit, 24),
            (Value::Unset, 24),
            (Value::Initial, 16),
            (Value::FontSize(19), 19),
        ] {
            let declaration = declaration(Property::FontSize, value, Origin::Author, 0);
            assert_eq!(
                Computed::cascade(Some(parent), &[declaration]).font_size,
                expected
            );
        }
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
            box_sizing: BoxSizing::ContentBox,
            margin_left: Margin::Px(0),
            margin_right: Margin::Px(0),
            margin_top: Margin::Px(0),
            margin_bottom: Margin::Px(0),
            direction: Direction::Ltr,
            font_size: 16,
            background_color: None,
            padding_left: 0,
            padding_right: 0,
            padding_top: 0,
            padding_bottom: 0,
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
                    height: Length::Auto,
                    box_sizing: BoxSizing::ContentBox,
                    margin_left: Margin::Px(0),
                    margin_right: Margin::Px(0),
                    margin_top: Margin::Px(0),
                    margin_bottom: Margin::Px(0),
                    direction: Direction::Ltr,
                    font_size: 16,
                    background_color: None,
                    padding_left: 0,
                    padding_right: 0,
                    padding_top: 0,
                    padding_bottom: 0
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
