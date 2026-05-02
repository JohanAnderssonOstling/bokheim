use floem::kurbo::Vec2;

// ============================================================================
// Sub-Structs for Servo-inspired Style Splitting
// ============================================================================

/// Font properties (inherited)
#[derive(Clone, Debug)]
pub struct Font {
    pub font_size: f32,
    pub font_weight: u16,
    pub font_style: FontStyle,
    pub font_family: Option<u16>,
    pub font_variant_small_caps: bool,
}

/// Text properties (inherited)
#[derive(Clone, Debug)]
pub struct InheritedText {
    pub color: u32,
    pub line_height: f32,
    pub letter_spacing: f32,
    pub word_spacing: f32,
    pub text_align: TextAlign,
    pub text_align_last: TextAlign,
    pub text_indent: f32,
    pub text_indent_hanging: bool,
    pub text_indent_each_line: bool,
    pub white_space: WhiteSpace,
    pub text_transform: TextTransform,
}

/// Box model properties (reset)
#[derive(Clone, Debug)]
pub struct BoxModel {
    pub display: Display,
    pub vertical_align: VerticalAlignValue,
    pub border_collapse: BorderCollapseMode,
    pub border_spacing_horizontal: f32,
    pub border_spacing_vertical: f32,
    pub empty_cells: EmptyCellsMode,
    pub caption_side: CaptionSide,
    pub width: PreferredSize,
    pub height: PreferredSize,
    pub min_width: PreferredSize,
    pub min_height: PreferredSize,
    pub max_width: PreferredSize,
    pub max_height: PreferredSize,
    pub margin_top: f32,
    pub margin_bottom: f32,
    pub margin_left: f32,
    pub margin_right: f32,
    pub padding_top: f32,
    pub padding_bottom: f32,
    pub padding_left: f32,
    pub padding_right: f32,
}

/// Border properties (reset)
#[derive(Clone, Debug)]
pub struct Border {
    pub border_top_width: f32,
    pub border_right_width: f32,
    pub border_bottom_width: f32,
    pub border_left_width: f32,
    pub border_top_color: u32,
    pub border_right_color: u32,
    pub border_bottom_color: u32,
    pub border_left_color: u32,
    pub border_top_style: BorderStyle,
    pub border_right_style: BorderStyle,
    pub border_bottom_style: BorderStyle,
    pub border_left_style: BorderStyle,
}

/// Background properties (reset)
#[derive(Clone, Debug)]
pub struct Background {
    pub background_color: u32,
    pub text_decoration: u8, // bitflags: 1=underline, 2=overline, 4=line-through
}

/// Style indices - replaces Option<u32> on LayoutBox
#[derive(Clone, Copy, Debug)]
pub struct StyleIndices {
    pub font_idx: u32,
    pub text_idx: u32,
    pub box_idx: u32,
    pub border_idx: u32,
    pub bg_idx: u32,
}

/// Style view - returned by get_style(), provides flat access to all fields
#[derive(Clone, Copy, Debug)]
pub struct StyleView<'a> {
    pub font: &'a Font,
    pub text: &'a InheritedText,
    pub box_model: &'a BoxModel,
    pub border: &'a Border,
    pub background: &'a Background,
}

impl<'a> StyleView<'a> {
    // Font accessors
    pub fn font_size(&self) -> f32 {
        self.font.font_size
    }
    pub fn font_weight(&self) -> u16 {
        self.font.font_weight
    }
    pub fn font_style(&self) -> FontStyle {
        self.font.font_style
    }
    pub fn font_family(&self) -> Option<u16> {
        self.font.font_family
    }
    pub fn font_variant_small_caps(&self) -> bool {
        self.font.font_variant_small_caps
    }

    // Text accessors
    pub fn color(&self) -> u32 {
        self.text.color
    }
    pub fn line_height(&self) -> f32 {
        self.text.line_height
    }
    pub fn letter_spacing(&self) -> f32 {
        self.text.letter_spacing
    }
    pub fn word_spacing(&self) -> f32 {
        self.text.word_spacing
    }
    pub fn text_align(&self) -> TextAlign {
        self.text.text_align
    }
    pub fn text_align_last(&self) -> TextAlign {
        self.text.text_align_last
    }
    pub fn text_indent(&self) -> f32 {
        self.text.text_indent
    }
    pub fn text_indent_hanging(&self) -> bool {
        self.text.text_indent_hanging
    }
    pub fn text_indent_each_line(&self) -> bool {
        self.text.text_indent_each_line
    }
    pub fn white_space(&self) -> WhiteSpace {
        self.text.white_space
    }
    pub fn text_transform(&self) -> TextTransform {
        self.text.text_transform
    }

    // Box model accessors
    pub fn display(&self) -> Display {
        self.box_model.display
    }
    pub fn vertical_align(&self) -> VerticalAlignValue {
        self.box_model.vertical_align
    }
    pub fn border_collapse(&self) -> BorderCollapseMode {
        self.box_model.border_collapse
    }
    pub fn border_spacing_horizontal(&self) -> f32 {
        self.box_model.border_spacing_horizontal
    }
    pub fn border_spacing_vertical(&self) -> f32 {
        self.box_model.border_spacing_vertical
    }
    pub fn empty_cells(&self) -> EmptyCellsMode {
        self.box_model.empty_cells
    }
    pub fn caption_side(&self) -> CaptionSide {
        self.box_model.caption_side
    }
    pub fn width(&self) -> PreferredSize {
        self.box_model.width
    }
    pub fn height(&self) -> PreferredSize {
        self.box_model.height
    }
    pub fn min_width(&self) -> PreferredSize {
        self.box_model.min_width
    }
    pub fn min_height(&self) -> PreferredSize {
        self.box_model.min_height
    }
    pub fn max_width(&self) -> PreferredSize {
        self.box_model.max_width
    }
    pub fn max_height(&self) -> PreferredSize {
        self.box_model.max_height
    }
    pub fn margin_top(&self) -> f32 {
        self.box_model.margin_top
    }
    pub fn margin_bottom(&self) -> f32 {
        self.box_model.margin_bottom
    }
    pub fn margin_left(&self) -> f32 {
        self.box_model.margin_left
    }
    pub fn margin_right(&self) -> f32 {
        self.box_model.margin_right
    }
    pub fn padding_top(&self) -> f32 {
        self.box_model.padding_top
    }
    pub fn padding_bottom(&self) -> f32 {
        self.box_model.padding_bottom
    }
    pub fn padding_left(&self) -> f32 {
        self.box_model.padding_left
    }
    pub fn padding_right(&self) -> f32 {
        self.box_model.padding_right
    }

    // Border accessors
    pub fn border_top_width(&self) -> f32 {
        self.border.border_top_width
    }
    pub fn border_right_width(&self) -> f32 {
        self.border.border_right_width
    }
    pub fn border_bottom_width(&self) -> f32 {
        self.border.border_bottom_width
    }
    pub fn border_left_width(&self) -> f32 {
        self.border.border_left_width
    }
    pub fn border_top_color(&self) -> u32 {
        self.border.border_top_color
    }
    pub fn border_right_color(&self) -> u32 {
        self.border.border_right_color
    }
    pub fn border_bottom_color(&self) -> u32 {
        self.border.border_bottom_color
    }
    pub fn border_left_color(&self) -> u32 {
        self.border.border_left_color
    }
    pub fn border_top_style(&self) -> BorderStyle {
        self.border.border_top_style
    }
    pub fn border_right_style(&self) -> BorderStyle {
        self.border.border_right_style
    }
    pub fn border_bottom_style(&self) -> BorderStyle {
        self.border.border_bottom_style
    }
    pub fn border_left_style(&self) -> BorderStyle {
        self.border.border_left_style
    }

    // Background accessors
    pub fn background_color(&self) -> u32 {
        self.background.background_color
    }
    pub fn text_decoration(&self) -> u8 {
        self.background.text_decoration
    }

    // Helper methods (moved from ComputedStyle)
    pub fn get_horizontal_margin_padding(&self) -> f64 {
        self.get_horizontal_margin() + self.get_horizontal_padding()
    }

    pub fn get_horizontal_margin(&self) -> f64 {
        (self.margin_left() + self.margin_right()) as f64
    }

    pub fn get_vertical_margin_padding(&self) -> f64 {
        (self.margin_top() + self.margin_bottom() + self.padding_top() + self.padding_bottom()) as f64
    }

    pub fn get_horizontal_padding(&self) -> f64 {
        (self.padding_left() + self.padding_right()) as f64
    }

    pub fn get_vertical_padding(&self) -> f64 {
        (self.padding_top() + self.padding_bottom()) as f64
    }

    pub fn get_top_left_padding(&self) -> Vec2 {
        Vec2::new(self.padding_left() as f64, self.padding_top() as f64)
    }

    pub fn should_parent_sibling_collapse(&self) -> bool {
        self.get_vertical_padding() == 0. && self.get_horizontal_padding() == 0.
    }
}

// Default implementations for sub-structs
impl Default for Font {
    fn default() -> Self {
        Self { font_size: 16.0, font_weight: 400, font_style: FontStyle::Normal, font_family: None, font_variant_small_caps: false }
    }
}

impl Default for InheritedText {
    fn default() -> Self {
        Self {
            color: 0x000000FF,
            line_height: 0.0,
            letter_spacing: 0.0,
            word_spacing: 0.0,
            text_align: TextAlign::Left,
            text_align_last: TextAlign::Left,
            text_indent: 0.0,
            text_indent_hanging: false,
            text_indent_each_line: false,
            white_space: WhiteSpace::Normal,
            text_transform: TextTransform::None,
        }
    }
}

impl Default for BoxModel {
    fn default() -> Self {
        Self {
            display: Display::Block,
            vertical_align: VerticalAlignValue::Baseline,
            border_collapse: BorderCollapseMode::Separate,
            border_spacing_horizontal: 0.0,
            border_spacing_vertical: 0.0,
            empty_cells: EmptyCellsMode::Show,
            caption_side: CaptionSide::Top,
            width: PreferredSize::Auto,
            height: PreferredSize::Auto,
            min_width: PreferredSize::Auto,
            min_height: PreferredSize::Auto,
            max_width: PreferredSize::Auto,
            max_height: PreferredSize::Auto,
            margin_top: 0.0,
            margin_bottom: 0.0,
            margin_left: 0.0,
            margin_right: 0.0,
            padding_top: 0.0,
            padding_bottom: 0.0,
            padding_left: 0.0,
            padding_right: 0.0,
        }
    }
}

impl Default for Border {
    fn default() -> Self {
        Self {
            border_top_width: 0.0,
            border_right_width: 0.0,
            border_bottom_width: 0.0,
            border_left_width: 0.0,
            border_top_color: 0x000000FF,
            border_right_color: 0x000000FF,
            border_bottom_color: 0x000000FF,
            border_left_color: 0x000000FF,
            border_top_style: BorderStyle::None,
            border_right_style: BorderStyle::None,
            border_bottom_style: BorderStyle::None,
            border_left_style: BorderStyle::None,
        }
    }
}

impl Default for Background {
    fn default() -> Self {
        Self { background_color: 0x00000000, text_decoration: 0 }
    }
}

// Default constants for reset groups (used by anonymous boxes)
pub const DEFAULT_BOX_MODEL: BoxModel = BoxModel {
    display: Display::Block,
    vertical_align: VerticalAlignValue::Baseline,
    border_collapse: BorderCollapseMode::Separate,
    border_spacing_horizontal: 0.0,
    border_spacing_vertical: 0.0,
    empty_cells: EmptyCellsMode::Show,
    caption_side: CaptionSide::Top,
    width: PreferredSize::Auto,
    height: PreferredSize::Auto,
    min_width: PreferredSize::Auto,
    min_height: PreferredSize::Auto,
    max_width: PreferredSize::Auto,
    max_height: PreferredSize::Auto,
    margin_top: 0.0,
    margin_bottom: 0.0,
    margin_left: 0.0,
    margin_right: 0.0,
    padding_top: 0.0,
    padding_bottom: 0.0,
    padding_left: 0.0,
    padding_right: 0.0,
};

pub const DEFAULT_BORDER: Border = Border {
    border_top_width: 0.0,
    border_right_width: 0.0,
    border_bottom_width: 0.0,
    border_left_width: 0.0,
    border_top_color: 0x000000FF,
    border_right_color: 0x000000FF,
    border_bottom_color: 0x000000FF,
    border_left_color: 0x000000FF,
    border_top_style: BorderStyle::None,
    border_right_style: BorderStyle::None,
    border_bottom_style: BorderStyle::None,
    border_left_style: BorderStyle::None,
};

pub const DEFAULT_BACKGROUND: Background = Background { background_color: 0x00000000, text_decoration: 0 };

// ============================================================================
// Original ComputedStyle (kept temporarily for backwards compatibility)
// ============================================================================

impl ComputedStyle {
    pub fn get_horizontal_margin_padding(&self) -> f64 {
        self.get_horizontal_margin() + self.get_horizontal_padding()
    }
    pub fn get_horizontal_margin(&self) -> f64 {
        (self.margin_left + self.margin_right) as f64
    }

    pub fn get_vertical_margin_padding(&self) -> f64 {
        (self.margin_top + self.margin_bottom + self.margin_left) as f64
    }

    pub fn get_horizontal_padding(&self) -> f64 {
        (self.padding_left + self.padding_right) as f64
    }

    pub fn get_vertical_padding(&self) -> f64 {
        (self.margin_top + self.margin_bottom) as f64
    }

    pub fn get_top_left_padding(&self) -> Vec2 {
        Vec2::new(self.padding_left as f64, self.padding_top as f64)
    }

    pub fn should_parent_sibling_collapse(&self) -> bool {
        self.get_vertical_padding() == 0. && self.get_horizontal_padding() == 0.
    }
}

pub fn resolve_preffered_size(preferred_size: PreferredSize, auto: f64, parent_size: f64) -> f64 {
    match preferred_size {
        PreferredSize::Auto => auto,
        PreferredSize::Px(px) => px as f64,
        PreferredSize::Percent(pct) => parent_size * pct as f64,
    }
}

#[derive(Clone)]
pub struct ComputedStyle {
    // Font
    pub font_size: f32,
    pub font_weight: u16,
    pub font_style: FontStyle,
    pub font_family: Option<u16>,

    // Text
    pub line_height: f32,
    pub letter_spacing: f32,
    pub word_spacing: f32,
    pub text_align: TextAlign,
    pub text_align_last: TextAlign,
    pub text_indent: f32,
    pub text_indent_hanging: bool,
    pub text_indent_each_line: bool,
    pub text_decoration: u8, // bitflags: 1=underline, 2=overline, 4=line-through
    pub white_space: WhiteSpace,
    pub text_transform: TextTransform,
    pub font_variant_small_caps: bool,
    pub vertical_align: VerticalAlignValue,
    pub border_collapse: BorderCollapseMode,
    pub border_spacing_horizontal: f32,
    pub border_spacing_vertical: f32,
    pub empty_cells: EmptyCellsMode,
    pub caption_side: CaptionSide,

    // Color (RGBA)
    pub color: u32,
    pub background_color: u32,

    // Box model
    pub width: PreferredSize,
    pub height: PreferredSize,
    pub min_width: PreferredSize,
    pub min_height: PreferredSize,
    pub max_width: PreferredSize,
    pub max_height: PreferredSize,
    pub margin_top: f32,
    pub margin_bottom: f32,
    pub margin_left: f32,
    pub margin_right: f32,
    pub padding_top: f32,
    pub padding_bottom: f32,
    pub padding_left: f32,
    pub padding_right: f32,

    // Border (per-side)
    pub border_top_width: f32,
    pub border_right_width: f32,
    pub border_bottom_width: f32,
    pub border_left_width: f32,
    pub border_top_color: u32,
    pub border_right_color: u32,
    pub border_bottom_color: u32,
    pub border_left_color: u32,
    pub border_top_style: BorderStyle,
    pub border_right_style: BorderStyle,
    pub border_bottom_style: BorderStyle,
    pub border_left_style: BorderStyle,

    pub display: Display,
}

impl Default for ComputedStyle {
    fn default() -> Self {
        Self {
            font_size: 16.0,
            font_weight: 400,
            font_style: FontStyle::Normal,
            font_family: None,
            line_height: 0.0,
            letter_spacing: 0.0,
            word_spacing: 0.0,
            text_align: TextAlign::Left,
            text_align_last: TextAlign::Left,
            text_indent: 0.0,
            text_indent_hanging: false,
            text_indent_each_line: false,
            text_decoration: 0,
            white_space: WhiteSpace::Normal,
            text_transform: TextTransform::None,
            font_variant_small_caps: false,
            vertical_align: VerticalAlignValue::Baseline,
            border_collapse: BorderCollapseMode::Separate,
            border_spacing_horizontal: 0.0,
            border_spacing_vertical: 0.0,
            empty_cells: EmptyCellsMode::Show,
            caption_side: CaptionSide::Top,
            color: 0x000000FF,
            background_color: 0x00000000,
            width: PreferredSize::Auto,
            height: PreferredSize::Auto,
            min_width: PreferredSize::Auto,
            min_height: PreferredSize::Auto,
            max_width: PreferredSize::Auto,
            max_height: PreferredSize::Auto,
            margin_top: 0.0,
            margin_bottom: 0.0,
            margin_left: 0.0,
            margin_right: 0.0,
            padding_top: 0.0,
            padding_bottom: 0.0,
            padding_left: 0.0,
            padding_right: 0.0,
            border_top_width: 0.0,
            border_right_width: 0.0,
            border_bottom_width: 0.0,
            border_left_width: 0.0,
            border_top_color: 0x000000FF,
            border_right_color: 0x000000FF,
            border_bottom_color: 0x000000FF,
            border_left_color: 0x000000FF,
            border_top_style: BorderStyle::None,
            border_right_style: BorderStyle::None,
            border_bottom_style: BorderStyle::None,
            border_left_style: BorderStyle::None,
            display: Display::Block,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FontStyle {
    #[default]
    Normal,
    Italic,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlign {
    #[default]
    Left,
    Right,
    Center,
    Justify,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WhiteSpace {
    #[default]
    Normal,
    Pre,
    NoWrap,
    PreWrap,
    PreLine,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BorderStyle {
    #[default]
    None,
    Solid,
    Dashed,
    Dotted,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextTransform {
    #[default]
    None,
    Uppercase,
    Lowercase,
    Capitalize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Display {
    #[default]
    Block,
    Inline,
    None,
    ListItem,
    // Table display types
    Table,
    InlineTable,
    TableRowGroup,
    TableHeaderGroup,
    TableFooterGroup,
    TableRow,
    TableColumnGroup,
    TableColumn,
    TableCell,
    TableCaption,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BorderCollapseMode {
    Collapse,
    #[default]
    Separate,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EmptyCellsMode {
    Hide,
    #[default]
    Show,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CaptionSide {
    Bottom,
    #[default]
    Top,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum VerticalAlignValue {
    #[default]
    Baseline,
    Sub,
    Super,
    Top,
    TextTop,
    Middle,
    Bottom,
    TextBottom,
    Length(f32),
    Percent(f32),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum PreferredSize {
    #[default]
    Auto,
    Px(f32),
    Percent(f32),
}
