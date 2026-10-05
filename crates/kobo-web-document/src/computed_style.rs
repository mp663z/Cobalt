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
    LineHeight,
    BackgroundColor,
    PaddingLeft,
    PaddingRight,
    PaddingTop,
    PaddingBottom,
    BorderTopWidth,
    BorderRightWidth,
    BorderBottomWidth,
    BorderLeftWidth,
    BorderTopStyle,
    BorderRightStyle,
    BorderBottomStyle,
    BorderLeftStyle,
    BorderTopColor,
    BorderRightColor,
    BorderBottomColor,
    BorderLeftColor,
    BackgroundClip,
    BackgroundLayers,
}

impl Property {
    /// Properties kept together in [`Decor`]: none of them is inherited.
    const DECOR: [Self; 14] = [
        Self::BorderTopWidth,
        Self::BorderRightWidth,
        Self::BorderBottomWidth,
        Self::BorderLeftWidth,
        Self::BorderTopStyle,
        Self::BorderRightStyle,
        Self::BorderBottomStyle,
        Self::BorderLeftStyle,
        Self::BorderTopColor,
        Self::BorderRightColor,
        Self::BorderBottomColor,
        Self::BorderLeftColor,
        Self::BackgroundClip,
        Self::BackgroundLayers,
    ];
}

/// Which box edge the background colour is clipped to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Clip {
    Border,
    Padding,
    Content,
}

/// Computed borders and background layer clipping. Sides are top, right,
/// bottom, left. A border whose style is `none` has no width when used.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Decor {
    pub border_width: [u32; 4],
    pub border_visible: [bool; 4],
    /// Whether each side's border colour is `transparent`. Any other colour
    /// would need painting that does not exist yet.
    pub border_clear: [bool; 4],
    /// Number of background layers: the length of the `background-image` list.
    pub layers: u8,
    /// `background-clip` list; it repeats to cover every layer.
    pub clips: [Clip; 4],
    pub clip_len: u8,
}

impl Decor {
    pub const INITIAL: Self = Self {
        border_width: [3; 4],
        border_visible: [false; 4],
        border_clear: [false; 4],
        layers: 1,
        clips: [Clip::Border; 4],
        clip_len: 1,
    };

    /// Whether any border would be drawn in a colour other than transparent.
    #[must_use]
    pub fn has_painted_border(self) -> bool {
        (0..4).any(|side| {
            self.border_visible[side] && self.border_width[side] > 0 && !self.border_clear[side]
        })
    }

    /// Used border widths, with `none` styles collapsed to zero.
    #[must_use]
    pub fn used_border(self) -> [u32; 4] {
        let mut used = [0; 4];
        for (side, slot) in used.iter_mut().enumerate() {
            if self.border_visible[side] {
                *slot = self.border_width[side];
            }
        }
        used
    }

    /// The clip of the bottom-most layer, which is the one that bounds the
    /// background colour (CSS Backgrounds 3, section 3.2).
    #[must_use]
    pub fn colour_clip(self) -> Clip {
        let layers = usize::from(self.layers.max(1));
        let len = usize::from(self.clip_len.max(1));
        self.clips[(layers - 1) % len]
    }

    fn get(self, property: Property) -> Value {
        match property {
            Property::BorderTopWidth => Value::Padding(self.border_width[0]),
            Property::BorderRightWidth => Value::Padding(self.border_width[1]),
            Property::BorderBottomWidth => Value::Padding(self.border_width[2]),
            Property::BorderLeftWidth => Value::Padding(self.border_width[3]),
            Property::BorderTopStyle => Value::Flag(self.border_visible[0]),
            Property::BorderRightStyle => Value::Flag(self.border_visible[1]),
            Property::BorderBottomStyle => Value::Flag(self.border_visible[2]),
            Property::BorderLeftStyle => Value::Flag(self.border_visible[3]),
            Property::BorderTopColor => Value::Flag(self.border_clear[0]),
            Property::BorderRightColor => Value::Flag(self.border_clear[1]),
            Property::BorderBottomColor => Value::Flag(self.border_clear[2]),
            Property::BorderLeftColor => Value::Flag(self.border_clear[3]),
            Property::BackgroundClip => Value::BackgroundClips {
                clips: self.clips,
                len: self.clip_len,
            },
            _ => Value::BackgroundLayers(self.layers),
        }
    }

    fn set(&mut self, property: Property, value: Value) {
        match (property, value) {
            (Property::BorderTopWidth, Value::Padding(px)) => self.border_width[0] = px,
            (Property::BorderRightWidth, Value::Padding(px)) => self.border_width[1] = px,
            (Property::BorderBottomWidth, Value::Padding(px)) => self.border_width[2] = px,
            (Property::BorderLeftWidth, Value::Padding(px)) => self.border_width[3] = px,
            (Property::BorderTopStyle, Value::Flag(v)) => self.border_visible[0] = v,
            (Property::BorderRightStyle, Value::Flag(v)) => self.border_visible[1] = v,
            (Property::BorderBottomStyle, Value::Flag(v)) => self.border_visible[2] = v,
            (Property::BorderLeftStyle, Value::Flag(v)) => self.border_visible[3] = v,
            (Property::BorderTopColor, Value::Flag(v)) => self.border_clear[0] = v,
            (Property::BorderRightColor, Value::Flag(v)) => self.border_clear[1] = v,
            (Property::BorderBottomColor, Value::Flag(v)) => self.border_clear[2] = v,
            (Property::BorderLeftColor, Value::Flag(v)) => self.border_clear[3] = v,
            (Property::BackgroundClip, Value::BackgroundClips { clips, len }) => {
                self.clips = clips;
                self.clip_len = len;
            }
            (Property::BackgroundLayers, Value::BackgroundLayers(n)) => self.layers = n,
            _ => unreachable!("resolved decor value has the wrong type"),
        }
    }
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
    BackgroundCurrentColor,
    Padding(u32),
    Flag(bool),
    BackgroundClips {
        clips: [Clip; 4],
        len: u8,
    },
    BackgroundLayers(u8),
    FontSize(u32),
    FontSizePercent(u32),
    LineHeight(Option<u32>),
    /// Unitless number in hundredths, inherited before multiplying font size.
    LineHeightNumber(u32),
    /// Percentage of this element's font size, computed before inheritance.
    LineHeightPercent(u32),
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
    /// Normal uses font metrics; explicit pixel line-height is inherited.
    pub line_height: Option<u32>,
    pub line_height_number: Option<u32>,
    /// Transparent unless explicitly painted; unlike foreground color, not inherited.
    pub background_color: Option<u32>,
    /// Keep the computed keyword until this element's used color is known.
    pub background_current_color: bool,
    pub padding_left: u32,
    pub padding_right: u32,
    pub padding_top: u32,
    pub padding_bottom: u32,
    pub decor: Decor,
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
        line_height: None,
        line_height_number: None,
        background_color: None,
        background_current_color: false,
        padding_left: 0,
        padding_right: 0,
        padding_top: 0,
        padding_bottom: 0,
        decor: Decor::INITIAL,
    };

    fn computed_line_height(self) -> Value {
        self.line_height_number
            .map_or(Value::LineHeight(self.line_height), Value::LineHeightNumber)
    }

    /// Exact used line-height. Fractional results need subpixel geometry.
    #[must_use]
    pub fn used_line_height(self, natural: u32) -> Option<u32> {
        if let Some(number) = self.line_height_number {
            let product = self.font_size.checked_mul(number)?;
            if product % 100 != 0 {
                return None;
            }
            Some(product / 100)
        } else {
            Some(self.line_height.unwrap_or(natural))
        }
    }

    /// Resolve computed currentColor for painting on this element only.
    #[must_use]
    pub fn used_background_color(self) -> Option<u32> {
        if self.background_current_color {
            Some(self.color)
        } else {
            self.background_color
        }
    }

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
            line_height: parent.unwrap_or(initial).line_height,
            line_height_number: parent.unwrap_or(initial).line_height_number,
            background_color: initial.background_color,
            background_current_color: false,
            padding_left: initial.padding_left,
            padding_right: initial.padding_right,
            padding_top: initial.padding_top,
            padding_bottom: initial.padding_bottom,
            decor: initial.decor,
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
            Property::LineHeight,
            Property::BackgroundColor,
            Property::PaddingLeft,
            Property::PaddingRight,
            Property::PaddingTop,
            Property::PaddingBottom,
        ]
        .into_iter()
        .chain(Property::DECOR)
        {
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
                Property::LineHeight => parent.unwrap_or(initial).computed_line_height(),
                Property::BackgroundColor => Value::BackgroundColor(initial.background_color),
                Property::PaddingLeft => Value::Padding(initial.padding_left),
                Property::PaddingRight => Value::Padding(initial.padding_right),
                Property::PaddingTop => Value::Padding(initial.padding_top),
                Property::PaddingBottom => Value::Padding(initial.padding_bottom),
                decor => initial.decor.get(decor),
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
                Value::FontSizePercent(percent) if property == Property::FontSize => {
                    let product = parent.unwrap_or(initial).font_size.checked_mul(percent);
                    computed.font_size = product
                        .filter(|p| p % 10_000 == 0)
                        .map_or(0, |p| p / 10_000);
                }
                Value::LineHeightPercent(percent) if property == Property::LineHeight => {
                    let product = computed.font_size.checked_mul(percent);
                    // Zero signals unrepresentable subpixel geometry to the style
                    // tree and used-height gate, never a fallback to normal.
                    computed.line_height = Some(
                        product
                            .filter(|p| p % 10_000 == 0)
                            .map_or(0, |p| p / 10_000),
                    );
                    computed.line_height_number = None;
                }
                Value::LineHeightNumber(number) if property == Property::LineHeight => {
                    computed.line_height = None;
                    computed.line_height_number = Some(number);
                }
                Value::LineHeight(px) if property == Property::LineHeight => {
                    computed.line_height = px;
                    computed.line_height_number = None;
                }
                Value::BackgroundCurrentColor if property == Property::BackgroundColor => {
                    computed.background_color = None;
                    computed.background_current_color = true;
                }
                Value::BackgroundColor(color) if property == Property::BackgroundColor => {
                    computed.background_color = color;
                    computed.background_current_color = false;
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
                value if Property::DECOR.contains(&property) => computed.decor.set(property, value),
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

#[allow(clippy::too_many_lines)] // Explicit per-property initial/inherited values and origin rollback.
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
        Property::LineHeight => Value::LineHeight(initial.line_height),
        Property::BackgroundColor => Value::BackgroundColor(initial.background_color),
        Property::PaddingLeft => Value::Padding(initial.padding_left),
        Property::PaddingRight => Value::Padding(initial.padding_right),
        Property::PaddingTop => Value::Padding(initial.padding_top),
        Property::PaddingBottom => Value::Padding(initial.padding_bottom),
        decor => initial.decor.get(decor),
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
        Property::LineHeight => parent.unwrap_or(initial).computed_line_height(),
        Property::BackgroundColor => {
            if parent.unwrap_or(initial).background_current_color {
                Value::BackgroundCurrentColor
            } else {
                Value::BackgroundColor(parent.unwrap_or(initial).background_color)
            }
        }
        Property::PaddingLeft => Value::Padding(parent.unwrap_or(initial).padding_left),
        Property::PaddingRight => Value::Padding(parent.unwrap_or(initial).padding_right),
        Property::PaddingTop => Value::Padding(parent.unwrap_or(initial).padding_top),
        Property::PaddingBottom => Value::Padding(parent.unwrap_or(initial).padding_bottom),
        decor => parent.unwrap_or(initial).decor.get(decor),
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
            Property::Color | Property::Direction | Property::FontSize | Property::LineHeight => {
                inherited
            }
            _ => initial_value,
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
                        Property::Color
                            | Property::Direction
                            | Property::FontSize
                            | Property::LineHeight
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
            line_height: None,
            line_height_number: None,
            background_color: None,
            background_current_color: false,
            padding_left: 0,
            padding_right: 0,
            padding_top: 0,
            padding_bottom: 0,
            decor: Decor::INITIAL,
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
                line_height: None,
                line_height_number: None,
                background_color: None,
                background_current_color: false,
                padding_left: 0,
                padding_right: 0,
                padding_top: 0,
                padding_bottom: 0,
                decor: Decor::INITIAL,
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
            line_height: None,
            line_height_number: None,
            background_color: None,
            background_current_color: false,
            padding_left: 0,
            padding_right: 0,
            padding_top: 0,
            padding_bottom: 0,
            decor: Decor::INITIAL,
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
                    line_height: None,
                    line_height_number: None,
                    background_color: None,
                    background_current_color: false,
                    padding_left: 0,
                    padding_right: 0,
                    padding_top: 0,
                    padding_bottom: 0,
                    decor: Decor::INITIAL,
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
