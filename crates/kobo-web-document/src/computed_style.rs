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
    Position,
    Top,
    Right,
    Bottom,
    Left,
    Color,
    Width,
    Height,
    BoxSizing,
    FlexGrow,
    FlexShrink,
    FlexBasis,
    MinWidth,
    MaxWidth,
    JustifyContent,
    AlignItems,
    MarginLeft,
    MarginRight,
    MarginTop,
    MarginBottom,
    Direction,
    TextAlign,
    /// EXPERIMENTAL: `text-decoration` underline|none only; not inherited.
    TextDecoration,
    FontFamily,
    FontSize,
    FontWeight,
    FontStyle,
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

/// Retained line style. Non-solid styles need separately proven geometry;
/// parsing one does not mean every border shape can be painted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BorderStyle {
    None,
    Solid,
    Dashed,
    Dotted,
}

/// Computed borders and background layer clipping. Sides are top, right,
/// bottom, left. A border whose style is `none` has no width when used.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Decor {
    pub border_width: [u32; 4],
    pub border_style: [BorderStyle; 4],
    pub border_color: [BorderColor; 4],
    /// Number of background layers: the length of the `background-image` list.
    pub layers: u8,
    /// `background-clip` list; it repeats to cover every layer.
    pub clips: [Clip; 4],
    pub clip_len: u8,
}

impl Decor {
    pub const INITIAL: Self = Self {
        border_width: [3; 4],
        border_style: [BorderStyle::None; 4],
        border_color: [BorderColor::CurrentColor; 4],
        layers: 1,
        clips: [Clip::Border; 4],
        clip_len: 1,
    };

    /// Whether any border would be drawn in a colour other than transparent.
    #[must_use]
    pub fn has_painted_border(self) -> bool {
        (0..4).any(|side| {
            self.border_style[side] != BorderStyle::None
                && self.border_width[side] > 0
                && self.border_color[side] != BorderColor::Transparent
        })
    }

    /// Used border widths, with `none` styles collapsed to zero.
    #[must_use]
    pub fn used_border(self) -> [u32; 4] {
        let mut used = [0; 4];
        for (side, slot) in used.iter_mut().enumerate() {
            if self.border_style[side] != BorderStyle::None {
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
            Property::BorderTopStyle => Value::BorderStyle(self.border_style[0]),
            Property::BorderRightStyle => Value::BorderStyle(self.border_style[1]),
            Property::BorderBottomStyle => Value::BorderStyle(self.border_style[2]),
            Property::BorderLeftStyle => Value::BorderStyle(self.border_style[3]),
            Property::BorderTopColor => Value::BorderColor(self.border_color[0]),
            Property::BorderRightColor => Value::BorderColor(self.border_color[1]),
            Property::BorderBottomColor => Value::BorderColor(self.border_color[2]),
            Property::BorderLeftColor => Value::BorderColor(self.border_color[3]),
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
            (Property::BorderTopStyle, Value::BorderStyle(v)) => self.border_style[0] = v,
            (Property::BorderRightStyle, Value::BorderStyle(v)) => self.border_style[1] = v,
            (Property::BorderBottomStyle, Value::BorderStyle(v)) => self.border_style[2] = v,
            (Property::BorderLeftStyle, Value::BorderStyle(v)) => self.border_style[3] = v,
            (Property::BorderTopColor, Value::BorderColor(v)) => self.border_color[0] = v,
            (Property::BorderRightColor, Value::BorderColor(v)) => self.border_color[1] = v,
            (Property::BorderBottomColor, Value::BorderColor(v)) => self.border_color[2] = v,
            (Property::BorderLeftColor, Value::BorderColor(v)) => self.border_color[3] = v,
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
pub enum BorderColor {
    Transparent,
    CurrentColor,
    Opaque(u32),
}

/// Bounded absolute leaf layout uses the initial containing block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Position {
    Static,
    Absolute,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Positioned {
    pub position: Position,
    pub offsets: [Margin; 4],
}
impl Positioned {
    pub const INITIAL: Self = Self {
        position: Position::Static,
        offsets: [Margin::Auto; 4],
    };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Display {
    None,
    Block,
    Inline,
    InlineBlock,
    ListItem,
    /// Scratch flex-row slice: block-level flex container.
    Flex,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Value {
    Display(Display),
    Position(Position),
    Offset(Margin),
    /// Opaque sRGB. Painting on a monochrome panel converts this later.
    Color(u32),
    Width(Length),
    Height(Length),
    BoxSizing(BoxSizing),
    /// Scratch flex slice: non-negative integer flex-grow only.
    FlexGrow(u32),
    FlexShrink(u32),
    /// Scratch: auto or px only (percent/content/keywords refuse).
    FlexBasis(Length),
    MinWidth(Length),
    MaxWidth(Length),
    JustifyContent(JustifyContent),
    AlignItems(AlignItems),
    Margin(Margin),
    /// MEASUREMENT-ONLY: signed `rem` margin in ten-thousandths of a rem. Rewritten to `Margin(Px)` by
    /// the style tree against the root font size; unresolved it computes font size 0 (refused).
    MarginRem(i32),
    Direction(Direction),
    TextAlign(TextAlign),
    TextDecoration(TextDecoration),
    BackgroundColor(Option<u32>),
    BorderColor(BorderColor),
    BorderStyle(BorderStyle),
    BackgroundCurrentColor,
    Padding(u32),
    Flag(bool),
    BackgroundClips {
        clips: [Clip; 4],
        len: u8,
    },
    BackgroundLayers(u8),
    FontFamily(Option<GenericFamily>),
    FontSize(u32),
    FontSizePercent(u32),
    /// `rem` font size in ten-thousandths. Rewritten to `FontSize(px)` by the style
    /// tree against the root computed font size before cascade; if it ever reaches
    /// the cascade it computes to 0, which the style tree refuses.
    FontSizeRem(u32),
    /// Absolute weight 100..=900 in steps of 100; 400 is normal, 700 bold.
    FontWeight(u16),
    /// `bolder`/`lighter` resolve against the inherited weight at cascade.
    FontWeightRelative {
        bolder: bool,
    },
    FontStyle(FontStyle),
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

/// Retention only. No generic is bound to a font provider yet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GenericFamily {
    Serif,
    SansSerif,
    Monospace,
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

/// MEASUREMENT-ONLY padding-em carrier: high bit set, low bits are ten-thousandths of an em.
pub const PADDING_EM_FLAG: u32 = 1 << 31;
/// Inexact em padding: never a real px value; the style tree refuses it (ComputedValue).
/// MEASUREMENT-ONLY padding-rem carrier (bit 30): ten-thousandths of a rem, resolved by the style tree.
pub const PADDING_REM_FLAG: u32 = 1 << 30;
pub const PADDING_UNREPRESENTABLE: u32 = u32::MAX;

/// Computed horizontal margin before resolving percentage against block width.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Margin {
    Auto,
    Px(i32),
    Percent(i32),
    /// Hundredths of one em, resolved against the element's own computed
    /// font size at the end of the cascade. Only exact integer pixels
    /// resolve; a subpixel or overflowing product stays `Em` so geometry
    /// passes refuse it instead of rounding.
    Em(i32),
}

/// Scratch flex slice: main-axis distribution of free space.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JustifyContent {
    FlexStart,
    FlexEnd,
    Center,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

/// Scratch flex slice: cross-axis alignment (baseline refused).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlignItems {
    Stretch,
    FlexStart,
    FlexEnd,
    Center,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    Ltr,
    Rtl,
}

/// Direction-relative initial value is preserved for inheritance/initial.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextAlign {
    Initial,
    Left,
    Right,
    Center,
}

/// EXPERIMENTAL: only `none` and `underline` exist; every other text-decoration form is refused by the parser.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextDecoration {
    None,
    Underline,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FontStyle {
    Normal,
    Italic,
    Oblique,
}

/// CSS 2.1 §15.6 bolder/lighter table on hundred-step weights.
#[must_use]
pub const fn relative_weight(weight: u16, bolder: bool) -> u16 {
    if bolder {
        match weight {
            0..=300 => 400,
            400..=500 => 700,
            _ => 900,
        }
    } else {
        match weight {
            0..=500 => 100,
            600..=700 => 400,
            _ => 700,
        }
    }
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
    pub positioned: Positioned,
    pub color: u32,
    pub width: Length,
    pub height: Length,
    pub box_sizing: BoxSizing,
    pub flex_grow: u32,
    pub flex_shrink: u32,
    pub flex_basis: Length,
    pub min_width: Length,
    pub max_width: Length,
    pub justify_content: JustifyContent,
    pub align_items: AlignItems,
    pub margin_left: Margin,
    pub margin_right: Margin,
    pub margin_top: Margin,
    pub margin_bottom: Margin,
    pub direction: Direction,
    pub text_align: TextAlign,
    /// Not inherited; a decorating box paints its own and descendant text (restricted paths only).
    pub text_decoration: TextDecoration,
    /// Computed CSS font-size in integer pixels. Relative sizes need an explicit parent.
    pub font_size: u32,
    /// Explicit generic family, retained but not admitted for rendering.
    /// None is the existing unresolved default; it does not bind a UA face.
    pub font_family: Option<GenericFamily>,
    /// Computed weight 100..=900; painters that lack a matching face refuse.
    pub font_weight: u16,
    /// Italic/oblique text is computed but refused by the restricted painter.
    pub font_style: FontStyle,
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
        positioned: Positioned::INITIAL,
        color: 0x00_00_00,
        width: Length::Auto,
        height: Length::Auto,
        box_sizing: BoxSizing::ContentBox,
        flex_grow: 0,
        flex_shrink: 1,
        flex_basis: Length::Auto,
        min_width: Length::Auto,
        max_width: Length::Auto,
        justify_content: JustifyContent::FlexStart,
        align_items: AlignItems::Stretch,
        margin_left: Margin::Px(0),
        margin_right: Margin::Px(0),
        margin_top: Margin::Px(0),
        margin_bottom: Margin::Px(0),
        direction: Direction::Ltr,
        text_align: TextAlign::Initial,
        text_decoration: TextDecoration::None,
        font_size: 16,
        font_family: None,
        font_weight: 400,
        font_style: FontStyle::Normal,
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
            positioned: initial.positioned,
            color: parent.unwrap_or(initial).color,
            width: initial.width,
            height: initial.height,
            box_sizing: initial.box_sizing,
            flex_grow: 0,
            flex_shrink: 1,
            flex_basis: Length::Auto,
            min_width: Length::Auto,
            max_width: Length::Auto,
            justify_content: JustifyContent::FlexStart,
            align_items: AlignItems::Stretch,
            margin_left: initial.margin_left,
            margin_right: initial.margin_right,
            margin_top: initial.margin_top,
            margin_bottom: initial.margin_bottom,
            direction: parent.unwrap_or(initial).direction,
            text_align: parent.unwrap_or(initial).text_align,
            text_decoration: TextDecoration::None,
            font_size: parent.unwrap_or(initial).font_size,
            font_family: parent.unwrap_or(initial).font_family,
            font_weight: parent.unwrap_or(initial).font_weight,
            font_style: parent.unwrap_or(initial).font_style,
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
            Property::Position,
            Property::Top,
            Property::Right,
            Property::Bottom,
            Property::Left,
            Property::Color,
            Property::Width,
            Property::Height,
            Property::BoxSizing,
            Property::FlexGrow,
            Property::FlexShrink,
            Property::FlexBasis,
            Property::MinWidth,
            Property::MaxWidth,
            Property::JustifyContent,
            Property::AlignItems,
            Property::MarginLeft,
            Property::MarginRight,
            Property::MarginTop,
            Property::MarginBottom,
            Property::Direction,
            Property::TextAlign,
            Property::TextDecoration,
            Property::FontFamily,
            Property::FontSize,
            Property::FontWeight,
            Property::FontStyle,
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
                Property::Position => Value::Position(initial.positioned.position),
                Property::Top => Value::Offset(initial.positioned.offsets[0]),
                Property::Right => Value::Offset(initial.positioned.offsets[1]),
                Property::Bottom => Value::Offset(initial.positioned.offsets[2]),
                Property::Left => Value::Offset(initial.positioned.offsets[3]),
                Property::Color => Value::Color(parent.unwrap_or(initial).color),
                Property::Width => Value::Width(initial.width),
                Property::Height => Value::Height(initial.height),
                Property::BoxSizing => Value::BoxSizing(initial.box_sizing),
                Property::FlexGrow => Value::FlexGrow(initial.flex_grow),
                Property::FlexShrink => Value::FlexShrink(initial.flex_shrink),
                Property::FlexBasis => Value::FlexBasis(initial.flex_basis),
                Property::MinWidth => Value::MinWidth(initial.min_width),
                Property::MaxWidth => Value::MaxWidth(initial.max_width),
                Property::JustifyContent => Value::JustifyContent(initial.justify_content),
                Property::AlignItems => Value::AlignItems(initial.align_items),
                Property::MarginLeft => Value::Margin(initial.margin_left),
                Property::MarginRight => Value::Margin(initial.margin_right),
                Property::MarginTop => Value::Margin(initial.margin_top),
                Property::MarginBottom => Value::Margin(initial.margin_bottom),
                Property::Direction => Value::Direction(parent.unwrap_or(initial).direction),
                Property::TextAlign => Value::TextAlign(parent.unwrap_or(initial).text_align),
                Property::TextDecoration => Value::TextDecoration(initial.text_decoration),
                Property::FontFamily => Value::FontFamily(parent.unwrap_or(initial).font_family),
                Property::FontSize => Value::FontSize(parent.unwrap_or(initial).font_size),
                Property::FontWeight => Value::FontWeight(parent.unwrap_or(initial).font_weight),
                Property::FontStyle => Value::FontStyle(parent.unwrap_or(initial).font_style),
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
                Value::FontFamily(family) if property == Property::FontFamily => {
                    computed.font_family = family;
                }
                Value::Display(display) if property == Property::Display => {
                    computed.display = display;
                }
                Value::Position(position) if property == Property::Position => {
                    computed.positioned.position = position;
                }
                Value::Offset(offset) if property == Property::Top => {
                    computed.positioned.offsets[0] = offset;
                }
                Value::Offset(offset) if property == Property::Right => {
                    computed.positioned.offsets[1] = offset;
                }
                Value::Offset(offset) if property == Property::Bottom => {
                    computed.positioned.offsets[2] = offset;
                }
                Value::Offset(offset) if property == Property::Left => {
                    computed.positioned.offsets[3] = offset;
                }
                Value::Color(color) if property == Property::Color => computed.color = color,
                Value::Width(width) if property == Property::Width => computed.width = width,
                Value::Height(height) if property == Property::Height => computed.height = height,
                Value::FlexGrow(grow) if property == Property::FlexGrow => computed.flex_grow = grow,
                Value::FlexShrink(v) if property == Property::FlexShrink => computed.flex_shrink = v,
                Value::FlexBasis(v) if property == Property::FlexBasis => computed.flex_basis = v,
                Value::MinWidth(v) if property == Property::MinWidth => computed.min_width = v,
                Value::MaxWidth(v) if property == Property::MaxWidth => computed.max_width = v,
                Value::JustifyContent(v) if property == Property::JustifyContent => computed.justify_content = v,
                Value::AlignItems(v) if property == Property::AlignItems => computed.align_items = v,
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
                Value::TextAlign(align) if property == Property::TextAlign => {
                    computed.text_align = align;
                }
                Value::TextDecoration(d) if property == Property::TextDecoration => {
                    computed.text_decoration = d;
                }
                Value::Direction(direction) if property == Property::Direction => {
                    computed.direction = direction;
                }
                Value::FontSize(px) if property == Property::FontSize => computed.font_size = px,
                Value::FontWeight(weight) if property == Property::FontWeight => {
                    computed.font_weight = weight;
                }
                Value::FontWeightRelative { bolder } if property == Property::FontWeight => {
                    computed.font_weight =
                        relative_weight(parent.unwrap_or(initial).font_weight, bolder);
                }
                Value::FontStyle(style) if property == Property::FontStyle => {
                    computed.font_style = style;
                }
                Value::FontSizeRem(_) if property == Property::FontSize => computed.font_size = 0,
                Value::MarginRem(_) => {
                    // Unresolved rem never reaches a real tree (the style tree rewrites or refuses it);
                    // keep the inexact-em marker so downstream geometry refuses instead of using 0.
                    match property {
                        Property::MarginTop => computed.margin_top = Margin::Em(1),
                        Property::MarginRight => computed.margin_right = Margin::Em(1),
                        Property::MarginBottom => computed.margin_bottom = Margin::Em(1),
                        Property::MarginLeft => computed.margin_left = Margin::Em(1),
                        _ => unreachable!("rem margin on a non-margin property"),
                    }
                }
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
        // Em margins resolve against this element's own computed font size,
        // which the property loop has now settled. An inexact or overflowing
        // product keeps the `Em` marker so downstream passes refuse the
        // geometry instead of painting a rounded margin.
        for margin in [
            &mut computed.margin_top,
            &mut computed.margin_right,
            &mut computed.margin_bottom,
            &mut computed.margin_left,
        ] {
            if let Margin::Em(hundredths) = *margin {
                let product = i64::from(computed.font_size) * i64::from(hundredths);
                if product % 10_000 == 0 {
                    if let Ok(px) = i32::try_from(product / 10_000) {
                        *margin = Margin::Px(px);
                    }
                }
            }
        }
        for padding in [
            &mut computed.padding_top,
            &mut computed.padding_right,
            &mut computed.padding_bottom,
            &mut computed.padding_left,
        ] {
            if *padding != PADDING_UNREPRESENTABLE
                && *padding & PADDING_EM_FLAG == 0
                && *padding & PADDING_REM_FLAG != 0
            {
                // Unresolved rem carrier (the style tree resolves it before the cascade): never a px value.
                *padding = PADDING_UNREPRESENTABLE;
            } else if *padding != PADDING_UNREPRESENTABLE && *padding & PADDING_EM_FLAG != 0 {
                let product = u64::from(computed.font_size) * u64::from(*padding & !PADDING_EM_FLAG);
                *padding = if product % 10_000 == 0 {
                    u32::try_from(product / 10_000)
                        .ok()
                        .filter(|px| *px < PADDING_EM_FLAG)
                        .unwrap_or(PADDING_UNREPRESENTABLE)
                } else {
                    PADDING_UNREPRESENTABLE
                };
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
        Property::Position => Value::Position(initial.positioned.position),
        Property::Top => Value::Offset(initial.positioned.offsets[0]),
        Property::Right => Value::Offset(initial.positioned.offsets[1]),
        Property::Bottom => Value::Offset(initial.positioned.offsets[2]),
        Property::Left => Value::Offset(initial.positioned.offsets[3]),
        Property::Color => Value::Color(initial.color),
        Property::Width => Value::Width(initial.width),
        Property::Height => Value::Height(initial.height),
        Property::BoxSizing => Value::BoxSizing(initial.box_sizing),
        Property::FlexGrow => Value::FlexGrow(initial.flex_grow),
        Property::FlexShrink => Value::FlexShrink(initial.flex_shrink),
        Property::FlexBasis => Value::FlexBasis(initial.flex_basis),
        Property::MinWidth => Value::MinWidth(initial.min_width),
        Property::MaxWidth => Value::MaxWidth(initial.max_width),
        Property::JustifyContent => Value::JustifyContent(initial.justify_content),
        Property::AlignItems => Value::AlignItems(initial.align_items),
        Property::MarginLeft => Value::Margin(initial.margin_left),
        Property::MarginRight => Value::Margin(initial.margin_right),
        Property::MarginTop => Value::Margin(initial.margin_top),
        Property::MarginBottom => Value::Margin(initial.margin_bottom),
        Property::Direction => Value::Direction(initial.direction),
        Property::TextAlign => Value::TextAlign(initial.text_align),
        Property::TextDecoration => Value::TextDecoration(initial.text_decoration),
        Property::FontFamily => Value::FontFamily(initial.font_family),
        Property::FontSize => Value::FontSize(initial.font_size),
        Property::FontWeight => Value::FontWeight(initial.font_weight),
        Property::FontStyle => Value::FontStyle(initial.font_style),
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
        Property::Position => Value::Position(parent.unwrap_or(initial).positioned.position),
        Property::Top => Value::Offset(parent.unwrap_or(initial).positioned.offsets[0]),
        Property::Right => Value::Offset(parent.unwrap_or(initial).positioned.offsets[1]),
        Property::Bottom => Value::Offset(parent.unwrap_or(initial).positioned.offsets[2]),
        Property::Left => Value::Offset(parent.unwrap_or(initial).positioned.offsets[3]),
        Property::Color => Value::Color(parent.unwrap_or(initial).color),
        Property::Width => Value::Width(parent.unwrap_or(initial).width),
        Property::Height => Value::Height(parent.unwrap_or(initial).height),
        Property::BoxSizing => Value::BoxSizing(parent.unwrap_or(initial).box_sizing),
        Property::FlexGrow => Value::FlexGrow(parent.unwrap_or(initial).flex_grow),
        Property::FlexShrink => Value::FlexShrink(parent.unwrap_or(initial).flex_shrink),
        Property::FlexBasis => Value::FlexBasis(parent.unwrap_or(initial).flex_basis),
        Property::MinWidth => Value::MinWidth(parent.unwrap_or(initial).min_width),
        Property::MaxWidth => Value::MaxWidth(parent.unwrap_or(initial).max_width),
        Property::JustifyContent => Value::JustifyContent(parent.unwrap_or(initial).justify_content),
        Property::AlignItems => Value::AlignItems(parent.unwrap_or(initial).align_items),
        Property::MarginLeft => Value::Margin(parent.unwrap_or(initial).margin_left),
        Property::MarginRight => Value::Margin(parent.unwrap_or(initial).margin_right),
        Property::MarginTop => Value::Margin(parent.unwrap_or(initial).margin_top),
        Property::MarginBottom => Value::Margin(parent.unwrap_or(initial).margin_bottom),
        Property::Direction => Value::Direction(parent.unwrap_or(initial).direction),
        Property::TextAlign => Value::TextAlign(parent.unwrap_or(initial).text_align),
        Property::TextDecoration => Value::TextDecoration(parent.unwrap_or(initial).text_decoration),
        Property::FontFamily => Value::FontFamily(parent.unwrap_or(initial).font_family),
        Property::FontSize => Value::FontSize(parent.unwrap_or(initial).font_size),
        Property::FontWeight => Value::FontWeight(parent.unwrap_or(initial).font_weight),
        Property::FontStyle => Value::FontStyle(parent.unwrap_or(initial).font_style),
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
            Property::Color
            | Property::Direction
            | Property::TextAlign
            | Property::FontFamily
            | Property::FontSize
            | Property::FontWeight
            | Property::FontStyle
            | Property::LineHeight => inherited,
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
                            | Property::TextAlign
                            | Property::FontFamily
                            | Property::FontSize
                            | Property::FontWeight
                            | Property::FontStyle
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
            positioned: Positioned::INITIAL,
            color: 0x12_34_56,
            width: Length::Px(42),
            height: Length::Auto,
            box_sizing: BoxSizing::ContentBox,
            flex_grow: 0,
            flex_shrink: 1,
            flex_basis: Length::Auto,
            min_width: Length::Auto,
            max_width: Length::Auto,
            justify_content: JustifyContent::FlexStart,
            align_items: AlignItems::Stretch,
            margin_left: Margin::Px(0),
            margin_right: Margin::Px(0),
            margin_top: Margin::Px(0),
            margin_bottom: Margin::Px(0),
            direction: Direction::Ltr,
            text_align: TextAlign::Initial,
        text_decoration: TextDecoration::None,
            font_weight: 400,
            font_style: FontStyle::Normal,
            font_size: 16,
            font_family: None,
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
                positioned: Positioned::INITIAL,
                color: parent.color,
                width: Length::Auto,
                height: Length::Auto,
                box_sizing: BoxSizing::ContentBox,
                flex_grow: 0,
                flex_shrink: 1,
                flex_basis: Length::Auto,
                min_width: Length::Auto,
                max_width: Length::Auto,
                justify_content: JustifyContent::FlexStart,
                align_items: AlignItems::Stretch,
                margin_left: Margin::Px(0),
                margin_right: Margin::Px(0),
                margin_top: Margin::Px(0),
                margin_bottom: Margin::Px(0),
                direction: Direction::Ltr,
                text_align: TextAlign::Initial,
        text_decoration: TextDecoration::None,
                font_weight: 400,
                font_style: FontStyle::Normal,
                font_size: 16,
                font_family: None,
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
            font_weight: 400,
            font_style: FontStyle::Normal,
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
            positioned: Positioned::INITIAL,
            color: 0x22_33_44,
            width: Length::Px(42),
            height: Length::Auto,
            box_sizing: BoxSizing::ContentBox,
            flex_grow: 0,
            flex_shrink: 1,
            flex_basis: Length::Auto,
            min_width: Length::Auto,
            max_width: Length::Auto,
            justify_content: JustifyContent::FlexStart,
            align_items: AlignItems::Stretch,
            margin_left: Margin::Px(0),
            margin_right: Margin::Px(0),
            margin_top: Margin::Px(0),
            margin_bottom: Margin::Px(0),
            direction: Direction::Ltr,
            text_align: TextAlign::Initial,
        text_decoration: TextDecoration::None,
            font_weight: 400,
            font_style: FontStyle::Normal,
            font_size: 16,
            font_family: None,
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
                    positioned: Positioned::INITIAL,
                    color: expected_color,
                    width: Length::Auto,
                    height: Length::Auto,
                    box_sizing: BoxSizing::ContentBox,
                    flex_grow: 0,
                    flex_shrink: 1,
                    flex_basis: Length::Auto,
                    min_width: Length::Auto,
                    max_width: Length::Auto,
                    justify_content: JustifyContent::FlexStart,
                    align_items: AlignItems::Stretch,
                    margin_left: Margin::Px(0),
                    margin_right: Margin::Px(0),
                    margin_top: Margin::Px(0),
                    margin_bottom: Margin::Px(0),
                    direction: Direction::Ltr,
                    text_align: TextAlign::Initial,
        text_decoration: TextDecoration::None,
                    font_weight: 400,
                    font_style: FontStyle::Normal,
                    font_size: 16,
                    font_family: None,
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
