use floem::kurbo::Vec2;

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

pub fn resolve_preffered_size(preferred_size: PreferredSize,auto: f64, parent_size: f64) -> f64 {
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