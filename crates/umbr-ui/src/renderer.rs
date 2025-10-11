use std::collections::BTreeMap;

use bytemuck::{Pod, Zeroable};
use pollster::block_on;
use uheex::types::{Declaration, Expr, Stylesheet, Uheex, VNode, Value, WidgetKind};
use umbr_core::{Anyresult, UmbrError};
use wgpu::util::DeviceExt;
use wgpu::{Device, Queue};
use winit::dpi::PhysicalSize;
use winit::event::Event;
use winit::event::WindowEvent;
use winit::event_loop::{ControlFlow, EventLoop};
use winit::platform::run_return::EventLoopExtRunReturn;
use winit::window::{Window, WindowBuilder};

#[derive(Debug, Clone)]
pub struct FrameSnapshot {
    pub pixels: Vec<u8>,
    pub width: i32,
    pub height: i32,
    pub stride: i32,
    pub n_channels: i32,
}

#[derive(Clone, Copy)]
enum Align {
    Start,
    Center,
    End,
}

#[derive(Clone, Copy, Debug, Default)]
struct Insets {
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
}

impl Insets {
    fn horizontal(&self) -> f32 {
        self.left + self.right
    }

    fn vertical(&self) -> f32 {
        self.top + self.bottom
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Color {
    r: f32,
    g: f32,
    b: f32,
    a: f32,
}

impl Color {
    fn from_hex(hex: &str) -> Option<Self> {
        let hex = hex.trim_start_matches('#');
        let (r, g, b) = match hex.len() {
            3 => {
                let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
                let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
                let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
                (r, g, b)
            }
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                (r, g, b)
            }
            _ => return None,
        };

        Some(Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: 1.0,
        })
    }

    fn transparent() -> Self {
        Self {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Size {
    width: f32,
    height: f32,
}

#[derive(Clone, Copy, Debug, Default)]
struct Point {
    x: f32,
    y: f32,
}

#[derive(Clone, Debug, Default)]
struct Style {
    background: Option<Color>,
    text_color: Color,
    padding: Insets,
    border: Option<(f32, Color)>,
    font_size: f32,
}

impl Style {
    fn merge(&mut self, other: &Style) {
        if other.background.is_some() {
            self.background = other.background;
        }
        if other.text_color.a > 0.0 {
            self.text_color = other.text_color;
        }
        if other.padding.horizontal() > 0.0 || other.padding.vertical() > 0.0 {
            self.padding = other.padding;
        }
        if other.border.is_some() {
            self.border = other.border;
        }
        if other.font_size != 0.0 {
            self.font_size = other.font_size;
        }
    }
}

#[derive(Clone, Debug)]
enum NodeKind {
    Column,
    Row,
    Label { text: String },
    Absolute { offset: Point },
}

#[derive(Clone, Debug)]
struct LayoutNode {
    kind: NodeKind,
    style: Style,
    children: Vec<LayoutNode>,
    spacing: f32,
    align: (Align, Align),
    flexible: bool,
    size_hint: Option<Size>,
    size: Size,
}

impl LayoutNode {
    fn new(kind: NodeKind, style: Style) -> Self {
        Self {
            kind,
            style,
            children: Vec::new(),
            spacing: 0.0,
            align: (Align::Start, Align::Start),
            flexible: false,
            size_hint: None,
            size: Size::default(),
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct Vertex {
    position: [f32; 2],
    color: [f32; 4],
}

struct SceneVertex {
    position: [f32; 2],
    color: [f32; 4],
}

struct SceneBuilder {
    vertices: Vec<SceneVertex>,
}

impl SceneBuilder {
    fn new() -> Self {
        Self {
            vertices: Vec::new(),
        }
    }

    fn push_rect(&mut self, origin: Point, size: Size, color: Color) {
        if color.a == 0.0 || size.width <= 0.0 || size.height <= 0.0 {
            return;
        }

        let x0 = origin.x;
        let y0 = origin.y;
        let x1 = origin.x + size.width;
        let y1 = origin.y + size.height;

        let color = [color.r, color.g, color.b, color.a];

        self.vertices.extend_from_slice(&[
            SceneVertex {
                position: [x0, y0],
                color,
            },
            SceneVertex {
                position: [x1, y0],
                color,
            },
            SceneVertex {
                position: [x1, y1],
                color,
            },
            SceneVertex {
                position: [x0, y0],
                color,
            },
            SceneVertex {
                position: [x1, y1],
                color,
            },
            SceneVertex {
                position: [x0, y1],
                color,
            },
        ]);
    }

    fn push_border(&mut self, origin: Point, size: Size, width: f32, color: Color) {
        if width <= 0.0 {
            return;
        }

        let top = Size {
            width: size.width,
            height: width,
        };
        let bottom_origin = Point {
            x: origin.x,
            y: origin.y + size.height - width,
        };
        let bottom = Size {
            width: size.width,
            height: width,
        };
        let left = Size {
            width,
            height: size.height,
        };
        let right_origin = Point {
            x: origin.x + size.width - width,
            y: origin.y,
        };
        let right = Size {
            width,
            height: size.height,
        };

        self.push_rect(origin, top, color);
        self.push_rect(bottom_origin, bottom, color);
        self.push_rect(origin, left, color);
        self.push_rect(right_origin, right, color);
    }

    fn into_vertices(self, width: f32, height: f32) -> Vec<Vertex> {
        self.vertices
            .into_iter()
            .map(|v| Vertex {
                position: [
                    (v.position[0] / width) * 2.0 - 1.0,
                    1.0 - (v.position[1] / height) * 2.0,
                ],
                color: v.color,
            })
            .collect()
    }
}

#[derive(Clone)]
struct Glyph {
    width: usize,
    height: usize,
    data: Vec<bool>,
}

impl Glyph {
    fn index(&self, x: usize, y: usize) -> bool {
        self.data[y * self.width + x]
    }
}

#[derive(Clone)]
struct BitmapFont {
    glyphs: BTreeMap<char, Glyph>,
    base_width: f32,
    base_height: f32,
}

impl BitmapFont {
    fn new() -> Self {
        let mut glyphs = BTreeMap::new();

        fn glyph(pattern: &[&str]) -> Glyph {
            let height = pattern.len();
            let width = pattern.first().map(|row| row.len()).unwrap_or(0);
            let mut data = Vec::with_capacity(width * height);
            for row in pattern {
                for ch in row.chars() {
                    data.push(ch == '#');
                }
            }
            Glyph {
                width,
                height,
                data,
            }
        }

        macro_rules! set_glyph {
            ($ch:expr, $pattern:expr) => {
                glyphs.insert($ch, glyph($pattern));
            };
        }

        set_glyph!(
            'A',
            &[
                " ### ", "#   #", "#   #", "#####", "#   #", "#   #", "#   #"
            ]
        );
        set_glyph!(
            'B',
            &[
                "#### ", "#   #", "#   #", "#### ", "#   #", "#   #", "#### "
            ]
        );
        set_glyph!(
            'C',
            &[
                " ### ", "#   #", "#    ", "#    ", "#    ", "#   #", " ### "
            ]
        );
        set_glyph!(
            'D',
            &[
                "#### ", "#   #", "#   #", "#   #", "#   #", "#   #", "#### "
            ]
        );
        set_glyph!(
            'E',
            &[
                "#####", "#    ", "#    ", "#### ", "#    ", "#    ", "#####"
            ]
        );
        set_glyph!(
            'F',
            &[
                "#####", "#    ", "#    ", "#### ", "#    ", "#    ", "#    "
            ]
        );
        set_glyph!(
            'G',
            &[
                " ### ", "#   #", "#    ", "#  ##", "#   #", "#   #", " ### "
            ]
        );
        set_glyph!(
            'H',
            &[
                "#   #", "#   #", "#   #", "#####", "#   #", "#   #", "#   #"
            ]
        );
        set_glyph!(
            'I',
            &[
                " ### ", "  #  ", "  #  ", "  #  ", "  #  ", "  #  ", " ### "
            ]
        );
        set_glyph!(
            'J',
            &["  ###", "   #", "   #", "   #", "#  #", "#  #", " ## "]
        );
        set_glyph!(
            'K',
            &[
                "#   #", "#  # ", "# #  ", "##   ", "# #  ", "#  # ", "#   #"
            ]
        );
        set_glyph!(
            'L',
            &[
                "#    ", "#    ", "#    ", "#    ", "#    ", "#    ", "#####"
            ]
        );
        set_glyph!(
            'M',
            &[
                "#   #", "## ##", "# # #", "#   #", "#   #", "#   #", "#   #"
            ]
        );
        set_glyph!(
            'N',
            &[
                "#   #", "##  #", "# # #", "#  ##", "#   #", "#   #", "#   #"
            ]
        );
        set_glyph!(
            'O',
            &[
                " ### ", "#   #", "#   #", "#   #", "#   #", "#   #", " ### "
            ]
        );
        set_glyph!(
            'P',
            &[
                "#### ", "#   #", "#   #", "#### ", "#    ", "#    ", "#    "
            ]
        );
        set_glyph!(
            'Q',
            &[
                " ### ", "#   #", "#   #", "#   #", "# # #", "#  # ", " ## #"
            ]
        );
        set_glyph!(
            'R',
            &[
                "#### ", "#   #", "#   #", "#### ", "# #  ", "#  # ", "#   #"
            ]
        );
        set_glyph!(
            'S',
            &[
                " ####", "#    ", "#    ", " ### ", "    #", "    #", "#### "
            ]
        );
        set_glyph!(
            'T',
            &[
                "#####", "  #  ", "  #  ", "  #  ", "  #  ", "  #  ", "  #  "
            ]
        );
        set_glyph!(
            'U',
            &[
                "#   #", "#   #", "#   #", "#   #", "#   #", "#   #", " ### "
            ]
        );
        set_glyph!(
            'V',
            &[
                "#   #", "#   #", "#   #", "#   #", "#   #", " # # ", "  #  "
            ]
        );
        set_glyph!(
            'W',
            &[
                "#   #", "#   #", "#   #", "# # #", "# # #", "## ##", "#   #"
            ]
        );
        set_glyph!(
            'X',
            &[
                "#   #", "#   #", " # # ", "  #  ", " # # ", "#   #", "#   #"
            ]
        );
        set_glyph!(
            'Y',
            &[
                "#   #", "#   #", " # # ", "  #  ", "  #  ", "  #  ", "  #  "
            ]
        );
        set_glyph!(
            'Z',
            &[
                "#####", "    #", "   # ", "  #  ", " #   ", "#    ", "#####"
            ]
        );

        set_glyph!(
            'a',
            &[
                "     ", "     ", " ### ", "    #", " ####", "#   #", " ####"
            ]
        );
        set_glyph!(
            'b',
            &[
                "#    ", "#    ", "#### ", "#   #", "#   #", "#   #", "#### "
            ]
        );
        set_glyph!(
            'c',
            &[
                "     ", "     ", " ### ", "#   #", "#    ", "#   #", " ### "
            ]
        );
        set_glyph!(
            'd',
            &[
                "    #", "    #", " ####", "#   #", "#   #", "#   #", " ####"
            ]
        );
        set_glyph!(
            'e',
            &[
                "     ", "     ", " ### ", "#   #", "#####", "#    ", " ### "
            ]
        );
        set_glyph!(
            'f',
            &[
                "  ## ", " #  #", " #   ", "###  ", " #   ", " #   ", " #   "
            ]
        );
        set_glyph!(
            'g',
            &[
                "     ", "     ", " ####", "#   #", "#   #", " ####", "    #"
            ]
        );
        set_glyph!(
            'h',
            &[
                "#    ", "#    ", "#### ", "#   #", "#   #", "#   #", "#   #"
            ]
        );
        set_glyph!(
            'i',
            &[
                "  #  ", "     ", " ##  ", "  #  ", "  #  ", "  #  ", " ### "
            ]
        );
        set_glyph!(
            'j',
            &[
                "   # ", "     ", "  ## ", "   # ", "   # ", "#  # ", " ##  "
            ]
        );
        set_glyph!(
            'k',
            &[
                "#    ", "#    ", "#  # ", "# #  ", "##   ", "# #  ", "#  # "
            ]
        );
        set_glyph!(
            'l',
            &[
                " ##  ", "  #  ", "  #  ", "  #  ", "  #  ", "  #  ", " ### "
            ]
        );
        set_glyph!(
            'm',
            &[
                "     ", "     ", "## # ", "# # #", "# # #", "# # #", "#   #"
            ]
        );
        set_glyph!(
            'n',
            &[
                "     ", "     ", "#### ", "#   #", "#   #", "#   #", "#   #"
            ]
        );
        set_glyph!(
            'o',
            &[
                "     ", "     ", " ### ", "#   #", "#   #", "#   #", " ### "
            ]
        );
        set_glyph!(
            'p',
            &[
                "     ", "     ", "#### ", "#   #", "#   #", "#### ", "#    "
            ]
        );
        set_glyph!(
            'q',
            &[
                "     ", "     ", " ####", "#   #", "#   #", " ####", "    #"
            ]
        );
        set_glyph!(
            'r',
            &[
                "     ", "     ", "# ## ", "##  #", "#    ", "#    ", "#    "
            ]
        );
        set_glyph!(
            's',
            &[
                "     ", "     ", " ####", "#    ", " ### ", "    #", "#### "
            ]
        );
        set_glyph!(
            't',
            &[
                "  #  ", "  #  ", " ### ", "  #  ", "  #  ", "  #  ", "  ## "
            ]
        );
        set_glyph!(
            'u',
            &[
                "     ", "     ", "#   #", "#   #", "#   #", "#   #", " ####"
            ]
        );
        set_glyph!(
            'v',
            &[
                "     ", "     ", "#   #", "#   #", "#   #", " # # ", "  #  "
            ]
        );
        set_glyph!(
            'w',
            &[
                "     ", "     ", "#   #", "# # #", "# # #", "## ##", "#   #"
            ]
        );
        set_glyph!(
            'x',
            &[
                "     ", "     ", "#   #", " # # ", "  #  ", " # # ", "#   #"
            ]
        );
        set_glyph!(
            'y',
            &[
                "     ", "     ", "#   #", "#   #", "#   #", " ####", "    #"
            ]
        );
        set_glyph!(
            'z',
            &[
                "     ", "     ", "#####", "   # ", "  #  ", " #   ", "#####"
            ]
        );

        set_glyph!(
            '0',
            &[
                " ### ", "#   #", "#  ##", "# # #", "##  #", "#   #", " ### "
            ]
        );
        set_glyph!(
            '1',
            &[
                "  #  ", " ##  ", "  #  ", "  #  ", "  #  ", "  #  ", " ### "
            ]
        );
        set_glyph!(
            '2',
            &[
                " ### ", "#   #", "    #", "   # ", "  #  ", " #   ", "#####"
            ]
        );
        set_glyph!(
            '3',
            &[
                " ### ", "#   #", "    #", " ### ", "    #", "#   #", " ### "
            ]
        );
        set_glyph!(
            '4',
            &[
                "   # ", "  ## ", " # # ", "#  # ", "#####", "   # ", "   # "
            ]
        );
        set_glyph!(
            '5',
            &[
                "#####", "#    ", "#    ", "#### ", "    #", "#   #", " ### "
            ]
        );
        set_glyph!(
            '6',
            &[
                " ### ", "#   #", "#    ", "#### ", "#   #", "#   #", " ### "
            ]
        );
        set_glyph!(
            '7',
            &[
                "#####", "    #", "   # ", "   # ", "  #  ", "  #  ", "  #  "
            ]
        );
        set_glyph!(
            '8',
            &[
                " ### ", "#   #", "#   #", " ### ", "#   #", "#   #", " ### "
            ]
        );
        set_glyph!(
            '9',
            &[
                " ### ", "#   #", "#   #", " ####", "    #", "#   #", " ### "
            ]
        );

        set_glyph!(
            ':',
            &[
                "     ", "  #  ", "     ", "     ", "     ", "  #  ", "     "
            ]
        );
        set_glyph!(
            '.',
            &[
                "     ", "     ", "     ", "     ", "     ", "  ## ", "  ## "
            ]
        );
        set_glyph!(
            '-',
            &[
                "     ", "     ", "     ", " ### ", "     ", "     ", "     "
            ]
        );
        set_glyph!(
            ' ',
            &[
                "     ", "     ", "     ", "     ", "     ", "     ", "     "
            ]
        );
        set_glyph!(
            '•',
            &[
                "     ", "     ", " ### ", " ### ", " ### ", "     ", "     "
            ]
        );

        Self {
            glyphs,
            base_width: 6.0,
            base_height: 8.0,
        }
    }

    fn measure_text(&self, text: &str, font_size: f32) -> Size {
        let scale = font_size / 12.0;
        let glyph_advance = self.base_width * scale;
        let height = self.base_height * scale;
        let width = text.chars().count() as f32 * glyph_advance;
        Size { width, height }
    }

    fn draw_text(
        &self,
        text: &str,
        origin: Point,
        font_size: f32,
        color: Color,
        builder: &mut SceneBuilder,
    ) {
        let scale = font_size / 12.0;
        let pixel_w = scale;
        let pixel_h = scale;
        let advance = self.base_width * scale;

        for (index, ch) in text.chars().enumerate() {
            if let Some(glyph) = self.glyphs.get(&ch) {
                let offset_x = origin.x + index as f32 * advance;
                for y in 0..glyph.height {
                    for x in 0..glyph.width {
                        if glyph.index(x, y) {
                            let pos = Point {
                                x: offset_x + x as f32 * pixel_w,
                                y: origin.y + y as f32 * pixel_h,
                            };
                            builder.push_rect(
                                pos,
                                Size {
                                    width: pixel_w,
                                    height: pixel_h,
                                },
                                color,
                            );
                        }
                    }
                }
            }
        }
    }
}

// Further rendering code is defined below...

fn value_as_string(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Value(Value::String(v)) => Some(v.clone()),
        Expr::Value(Value::Number(v)) => Some(v.to_string()),
        _ => None,
    }
}

fn parse_size_component(value: &str) -> f32 {
    let trimmed = value.trim();
    let numeric = trimmed
        .trim_end_matches("px")
        .trim_end_matches("rem")
        .trim_end_matches("em");
    numeric.parse::<f32>().unwrap_or(0.0)
}

fn parse_padding(value: &str) -> Insets {
    let parts: Vec<&str> = value.split_whitespace().collect();
    match parts.len() {
        1 => {
            let size = parse_size_component(parts[0]);
            Insets {
                left: size,
                right: size,
                top: size,
                bottom: size,
            }
        }
        2 => {
            let vertical = parse_size_component(parts[0]);
            let horizontal = parse_size_component(parts[1]);
            Insets {
                left: horizontal,
                right: horizontal,
                top: vertical,
                bottom: vertical,
            }
        }
        4 => Insets {
            top: parse_size_component(parts[0]),
            right: parse_size_component(parts[1]),
            bottom: parse_size_component(parts[2]),
            left: parse_size_component(parts[3]),
        },
        _ => Insets::default(),
    }
}

fn style_from_stylesheet(stylesheet: &Stylesheet) -> BTreeMap<String, Style> {
    let mut styles = BTreeMap::new();

    for rule in &stylesheet.rules {
        if let uheex::types::Selector::Class(class) = &rule.selector {
            let mut style = Style::default();
            style.text_color = Color {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            };
            style.font_size = 12.0;

            for declaration in &rule.declarations {
                match declaration {
                    Declaration::Simple { property, value } => match property.as_str() {
                        "background" => {
                            if let Some(color) =
                                value_as_string(value).and_then(|v| Color::from_hex(&v))
                            {
                                style.background = Some(color);
                            }
                        }
                        "color" => {
                            if let Some(color) =
                                value_as_string(value).and_then(|v| Color::from_hex(&v))
                            {
                                style.text_color = color;
                            }
                        }
                        "padding" => {
                            if let Some(value) = value_as_string(value) {
                                style.padding = parse_padding(&value);
                            }
                        }
                        _ => {}
                    },
                    Declaration::Nested { property, value } => match property.as_str() {
                        "border" => {
                            let mut border_width = 0.0;
                            let mut border_color = Color::transparent();
                            for declaration in value {
                                if let Declaration::Simple { property, value } = declaration {
                                    match property.as_str() {
                                        "width" => {
                                            if let Some(v) = value_as_string(value) {
                                                border_width = parse_size_component(&v);
                                            }
                                        }
                                        "color" => {
                                            if let Some(v) = value_as_string(value) {
                                                if let Some(color) = Color::from_hex(&v) {
                                                    border_color = color;
                                                }
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            if border_width > 0.0 {
                                style.border = Some((border_width, border_color));
                            }
                        }
                        "font" => {
                            for declaration in value {
                                if let Declaration::Simple { property, value } = declaration {
                                    if property == "size" {
                                        if let Some(v) = value_as_string(value) {
                                            let parsed = parse_size_component(&v);
                                            if parsed > 0.0 {
                                                style.font_size = parsed;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    },
                }
            }

            styles.insert(class.clone(), style);
        }
    }

    styles
}

fn base_style() -> Style {
    Style {
        background: None,
        text_color: Color {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 1.0,
        },
        padding: Insets::default(),
        border: None,
        font_size: 12.0,
    }
}

fn compose_style(class: Option<String>, styles: &BTreeMap<String, Style>) -> Style {
    let mut style = base_style();
    if let Some(class) = class {
        if let Some(found) = styles.get(&class) {
            style.merge(found);
        }
    }
    style
}

fn parse_align(attributes: &BTreeMap<String, Expr>) -> Option<(Align, Align)> {
    attributes.get("align").and_then(|expr| match expr {
        Expr::Array(values) if values.len() == 2 => Some((
            parse_single_align(&values[0]),
            parse_single_align(&values[1]),
        )),
        _ => None,
    })
}

fn parse_single_align(expr: &Expr) -> Align {
    match expr {
        Expr::Value(Value::String(value)) => match value.as_str() {
            "center" => Align::Center,
            "end" | "bottom" | "right" => Align::End,
            _ => Align::Start,
        },
        _ => Align::Start,
    }
}

fn parse_spacing(attributes: &BTreeMap<String, Expr>) -> f32 {
    attributes
        .get("spacing")
        .and_then(|expr| match expr {
            Expr::Value(Value::Number(value)) => Some(*value as f32),
            Expr::Value(Value::String(value)) => Some(parse_size_component(value)),
            _ => None,
        })
        .unwrap_or(0.0)
}

fn parse_flexible(attributes: &BTreeMap<String, Expr>) -> bool {
    attributes
        .get("flexible")
        .and_then(|expr| match expr {
            Expr::Value(Value::String(value)) => Some(value == "true"),
            _ => None,
        })
        .unwrap_or(false)
}

fn parse_size_hint(attributes: &BTreeMap<String, Expr>) -> Option<Size> {
    attributes.get("size").and_then(|expr| match expr {
        Expr::Array(values) if values.len() == 2 => {
            let width = match &values[0] {
                Expr::Value(Value::String(value)) => parse_size_component(value),
                Expr::Value(Value::Number(value)) => *value as f32,
                _ => 0.0,
            };
            let height = match &values[1] {
                Expr::Value(Value::String(value)) => parse_size_component(value),
                Expr::Value(Value::Number(value)) => *value as f32,
                _ => 0.0,
            };
            Some(Size { width, height })
        }
        _ => None,
    })
}

fn parse_offset(attributes: &BTreeMap<String, Expr>) -> Point {
    let x = attributes
        .get("x")
        .and_then(|expr| match expr {
            Expr::Value(Value::String(value)) => value.parse::<f32>().ok(),
            Expr::Value(Value::Number(value)) => Some(*value as f32),
            _ => None,
        })
        .unwrap_or(0.0);

    let y = attributes
        .get("y")
        .and_then(|expr| match expr {
            Expr::Value(Value::String(value)) => value.parse::<f32>().ok(),
            Expr::Value(Value::Number(value)) => Some(*value as f32),
            _ => None,
        })
        .unwrap_or(0.0);

    Point { x, y }
}

fn build_layout_nodes(
    nodes: &[VNode],
    styles: &BTreeMap<String, Style>,
    font: &BitmapFont,
) -> Vec<LayoutNode> {
    nodes
        .iter()
        .filter_map(|node| build_layout_node(node, styles, font))
        .collect()
}

fn build_layout_node(
    node: &VNode,
    styles: &BTreeMap<String, Style>,
    font: &BitmapFont,
) -> Option<LayoutNode> {
    match node {
        VNode::Widget {
            kind,
            attributes,
            child,
        } => {
            let class = attributes.get("class").and_then(|expr| match expr {
                Expr::Value(Value::String(value)) => Some(value.clone()),
                _ => None,
            });

            let style = compose_style(class, styles);
            let spacing = parse_spacing(attributes);
            let align = parse_align(attributes).unwrap_or((Align::Start, Align::Start));
            let flexible = parse_flexible(attributes);
            let size_hint = parse_size_hint(attributes);

            let mut layout = match kind {
                WidgetKind::Column => LayoutNode::new(NodeKind::Column, style.clone()),
                WidgetKind::Row => LayoutNode::new(NodeKind::Row, style.clone()),
                WidgetKind::Label => {
                    let text = child
                        .first()
                        .and_then(|node| match node {
                            VNode::String(value) => Some(value.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    LayoutNode::new(NodeKind::Label { text }, style.clone())
                }
                WidgetKind::Absolute => LayoutNode::new(
                    NodeKind::Absolute {
                        offset: parse_offset(attributes),
                    },
                    style.clone(),
                ),
                _ => return None,
            };

            layout.spacing = spacing;
            layout.align = align;
            layout.flexible = flexible;
            layout.size_hint = size_hint;

            if !matches!(layout.kind, NodeKind::Label { .. }) {
                layout.children = build_layout_nodes(child, styles, font);
            }

            compute_size(&mut layout, font);

            Some(layout)
        }
        VNode::Fragment(nodes) => {
            let mut container = LayoutNode::new(NodeKind::Column, base_style());
            container.children = build_layout_nodes(nodes, styles, font);
            if container.children.is_empty() {
                None
            } else {
                compute_size(&mut container, font);
                Some(container)
            }
        }
        VNode::Empty => None,
        _ => None,
    }
}

fn compute_size(node: &mut LayoutNode, font: &BitmapFont) {
    match &mut node.kind {
        NodeKind::Label { text } => {
            let mut size = font.measure_text(text, node.style.font_size);
            size.width += node.style.padding.horizontal();
            size.height += node.style.padding.vertical();
            if let Some(size_hint) = node.size_hint {
                if size_hint.width > 0.0 {
                    size.width = size_hint.width;
                }
                if size_hint.height > 0.0 {
                    size.height = size_hint.height;
                }
            }
            node.size = size;
        }
        NodeKind::Column => {
            let mut width = 0.0;
            let mut height = node.style.padding.vertical();
            for child in &mut node.children {
                compute_size(child, font);
                width = width.max(child.size.width);
                height += child.size.height;
            }
            if !node.children.is_empty() {
                height += node.spacing * (node.children.len() as f32 - 1.0);
            }
            width += node.style.padding.horizontal();

            if let Some(size_hint) = node.size_hint {
                if size_hint.width > 0.0 {
                    width = size_hint.width;
                }
                if size_hint.height > 0.0 {
                    height = size_hint.height;
                }
            }

            node.size = Size { width, height };
        }
        NodeKind::Row => {
            let mut width = node.style.padding.horizontal();
            let mut height = 0.0;
            for child in &mut node.children {
                compute_size(child, font);
                width += child.size.width;
                height = height.max(child.size.height);
            }
            if !node.children.is_empty() {
                width += node.spacing * (node.children.len() as f32 - 1.0);
            }
            height += node.style.padding.vertical();

            if let Some(size_hint) = node.size_hint {
                if size_hint.width > 0.0 {
                    width = size_hint.width;
                }
                if size_hint.height > 0.0 {
                    height = size_hint.height;
                }
            }

            node.size = Size { width, height };
        }
        NodeKind::Absolute { .. } => {
            if let Some(child) = node.children.first_mut() {
                compute_size(child, font);
                node.size = child.size;
            }
        }
    }
}

fn place_node(
    node: &LayoutNode,
    origin: Point,
    available: Size,
    builder: &mut SceneBuilder,
    font: &BitmapFont,
) {
    let mut node_width = node.size.width;
    let mut node_height = node.size.height;

    if node.flexible {
        if available.width > 0.0 {
            node_width = available.width;
        }
        if available.height > 0.0 {
            node_height = available.height;
        }
    }

    if let Some(color) = node.style.background {
        builder.push_rect(
            origin,
            Size {
                width: node_width,
                height: node_height,
            },
            color,
        );
    }

    if let Some((width, color)) = node.style.border {
        builder.push_border(
            origin,
            Size {
                width: node_width,
                height: node_height,
            },
            width,
            color,
        );
    }

    let content_origin = Point {
        x: origin.x + node.style.padding.left,
        y: origin.y + node.style.padding.top,
    };
    let content_size = Size {
        width: (node_width - node.style.padding.horizontal()).max(0.0),
        height: (node_height - node.style.padding.vertical()).max(0.0),
    };

    match &node.kind {
        NodeKind::Label { text } => {
            font.draw_text(
                text,
                content_origin,
                node.style.font_size,
                node.style.text_color,
                builder,
            );
        }
        NodeKind::Column => {
            let mut cursor_y = content_origin.y;
            let flexible_children = node.children.iter().filter(|child| child.flexible).count();
            let fixed_height: f32 = node
                .children
                .iter()
                .filter(|child| !child.flexible)
                .map(|child| child.size.height)
                .sum();
            let spacing_total = if node.children.is_empty() {
                0.0
            } else {
                node.spacing * (node.children.len().saturating_sub(1) as f32)
            };
            let mut remaining = (content_size.height - fixed_height - spacing_total).max(0.0);
            let per_flexible = if flexible_children > 0 {
                remaining / flexible_children as f32
            } else {
                0.0
            };

            for (index, child) in node.children.iter().enumerate() {
                let mut child_height = child.size.height;
                if child.flexible {
                    child_height = child_height.max(per_flexible);
                }
                let child_available = Size {
                    width: content_size.width,
                    height: child_height,
                };

                let horizontal_space = (content_size.width - child.size.width).max(0.0);
                let child_x = match node.align.0 {
                    Align::Start => content_origin.x,
                    Align::Center => content_origin.x + horizontal_space / 2.0,
                    Align::End => content_origin.x + horizontal_space,
                };

                place_node(
                    child,
                    Point {
                        x: child_x,
                        y: cursor_y,
                    },
                    child_available,
                    builder,
                    font,
                );

                cursor_y += child_height;
                if index + 1 < node.children.len() {
                    cursor_y += node.spacing;
                }
            }
        }
        NodeKind::Row => {
            let mut cursor_x = content_origin.x;
            let flexible_children = node.children.iter().filter(|child| child.flexible).count();
            let fixed_width: f32 = node
                .children
                .iter()
                .filter(|child| !child.flexible)
                .map(|child| child.size.width)
                .sum();
            let spacing_total = if node.children.is_empty() {
                0.0
            } else {
                node.spacing * (node.children.len().saturating_sub(1) as f32)
            };
            let mut remaining = (content_size.width - fixed_width - spacing_total).max(0.0);
            let per_flexible = if flexible_children > 0 {
                remaining / flexible_children as f32
            } else {
                0.0
            };

            for (index, child) in node.children.iter().enumerate() {
                let mut child_width = child.size.width;
                if child.flexible {
                    child_width = child_width.max(per_flexible);
                }
                let child_available = Size {
                    width: child_width,
                    height: content_size.height,
                };

                let vertical_space = (content_size.height - child.size.height).max(0.0);
                let child_y = match node.align.1 {
                    Align::Start => content_origin.y,
                    Align::Center => content_origin.y + vertical_space / 2.0,
                    Align::End => content_origin.y + vertical_space,
                };

                place_node(
                    child,
                    Point {
                        x: cursor_x,
                        y: child_y,
                    },
                    child_available,
                    builder,
                    font,
                );

                cursor_x += child_width;
                if index + 1 < node.children.len() {
                    cursor_x += node.spacing;
                }
            }
        }
        NodeKind::Absolute { offset } => {
            if let Some(child) = node.children.first() {
                let position = Point {
                    x: content_origin.x + offset.x,
                    y: content_origin.y + offset.y,
                };
                place_node(child, position, child.size, builder, font);
            }
        }
    }
}

fn parse_anchor(attributes: &BTreeMap<String, Expr>) -> (Align, Align) {
    attributes
        .get("anchor")
        .and_then(|expr| match expr {
            Expr::Array(values) if values.len() == 2 => Some((
                parse_single_align(&values[0]),
                parse_single_align(&values[1]),
            )),
            _ => None,
        })
        .unwrap_or((Align::Start, Align::Start))
}

fn extract_window_children(layout: &Uheex) -> (&BTreeMap<String, Expr>, Vec<VNode>) {
    if let VNode::Window { attributes, child } = &layout.root {
        (attributes, child.clone().into_vec())
    } else {
        (&BTreeMap::new(), Vec::new())
    }
}

fn build_scene_vertices(layout: &Uheex, width: u32, height: u32) -> Vec<Vertex> {
    let font = BitmapFont::new();
    let styles = layout
        .stylesheet
        .as_ref()
        .map(style_from_stylesheet)
        .unwrap_or_default();
    let (attributes, children) = extract_window_children(layout);
    let anchor = parse_anchor(attributes);
    let mut nodes = build_layout_nodes(&children, &styles, &font);
    let mut builder = SceneBuilder::new();

    if nodes.is_empty() {
        return builder.into_vertices(width as f32, height as f32);
    }

    let content_width = nodes
        .iter()
        .map(|node| node.size.width)
        .fold(0.0f32, f32::max)
        .min(width as f32);
    let content_height = nodes
        .iter()
        .map(|node| node.size.height)
        .fold(0.0f32, f32::max)
        .min(height as f32);

    let offset_x = match anchor.0 {
        Align::Start => 0.0,
        Align::Center => ((width as f32 - content_width) / 2.0).max(0.0),
        Align::End => (width as f32 - content_width).max(0.0),
    };

    let offset_y = match anchor.1 {
        Align::Start => 0.0,
        Align::Center => ((height as f32 - content_height) / 2.0).max(0.0),
        Align::End => (height as f32 - content_height).max(0.0),
    };

    for node in &nodes {
        place_node(
            node,
            Point {
                x: offset_x,
                y: offset_y,
            },
            Size {
                width: content_width,
                height: content_height,
            },
            &mut builder,
            &font,
        );
    }

    builder.into_vertices(width as f32, height as f32)
}

fn create_pipeline(device: &Device, format: wgpu::TextureFormat) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("solid_shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("shaders/solid.wgsl").into()),
    });

    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("solid_pipeline_layout"),
        bind_group_layouts: &[],
        push_constant_ranges: &[],
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("solid_pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: "vs_main",
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[
                    wgpu::VertexAttribute {
                        offset: 0,
                        shader_location: 0,
                        format: wgpu::VertexFormat::Float32x2,
                    },
                    wgpu::VertexAttribute {
                        offset: 8,
                        shader_location: 1,
                        format: wgpu::VertexFormat::Float32x4,
                    },
                ],
            }],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: "fs_main",
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview: None,
    })
}

fn request_device(
    surface: Option<&wgpu::Surface>,
) -> Anyresult<(wgpu::Instance, wgpu::Adapter, Device, Queue)> {
    let instance = wgpu::Instance::default();
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: surface,
        force_fallback_adapter: false,
    }))
    .ok_or_else(|| UmbrError::Generic("failed to acquire GPU adapter".into()))?;

    let (device, queue) = block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("umbr-ui-gpu-device"),
            features: wgpu::Features::empty(),
            limits: wgpu::Limits::downlevel_defaults(),
        },
        None,
    ))
    .map_err(|err| UmbrError::Generic(format!("failed to request GPU device: {err}")))?;

    Ok((instance, adapter, device, queue))
}

pub struct WgpuRenderer {
    device: Device,
    queue: Queue,
    pipeline: wgpu::RenderPipeline,
    format: wgpu::TextureFormat,
}

impl WgpuRenderer {
    pub fn new() -> Anyresult<Self> {
        let (_, _, device, queue) = request_device(None)?;
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let pipeline = create_pipeline(&device, format);
        Ok(Self {
            device,
            queue,
            pipeline,
            format,
        })
    }

    pub fn render(&mut self, layout: &Uheex, width: u32, height: u32) -> Anyresult<FrameSnapshot> {
        let vertices = build_scene_vertices(layout, width, height);

        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen_texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("offscreen_encoder"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("offscreen_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: true,
                    },
                })],
                depth_stencil_attachment: None,
            });

            if !vertices.is_empty() {
                let buffer = self
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("offscreen_vertices"),
                        contents: bytemuck::cast_slice(&vertices),
                        usage: wgpu::BufferUsages::VERTEX,
                    });

                pass.set_pipeline(&self.pipeline);
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..vertices.len() as u32, 0..1);
            }
        }

        self.queue.submit(Some(encoder.finish()));

        let bytes_per_pixel = 4u32;
        let padded_bytes_per_row =
            wgpu::util::align_to(width * bytes_per_pixel, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let output_buffer_size = padded_bytes_per_row as u64 * height as u64;
        let output_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("offscreen_output_buffer"),
            size: output_buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut copy_encoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("offscreen_copy_encoder"),
                });

        copy_encoder.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &output_buffer,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        self.queue.submit(Some(copy_encoder.finish()));

        let slice = output_buffer.slice(..);
        block_on(slice.map_async(wgpu::MapMode::Read))
            .map_err(|_| UmbrError::Generic("failed to map GPU buffer".into()))?;
        self.device.poll(wgpu::Maintain::Wait);

        let data = slice.get_mapped_range();
        let mut pixels = vec![0u8; (width * height * bytes_per_pixel) as usize];

        for row in 0..height as usize {
            let src_offset = row * padded_bytes_per_row as usize;
            let dst_offset = row * (width as usize * bytes_per_pixel as usize);
            let src = &data[src_offset..src_offset + width as usize * bytes_per_pixel as usize];
            pixels[dst_offset..dst_offset + src.len()].copy_from_slice(src);
        }

        drop(data);
        output_buffer.unmap();

        Ok(FrameSnapshot {
            pixels,
            width: width as i32,
            height: height as i32,
            stride: (width * bytes_per_pixel) as i32,
            n_channels: bytes_per_pixel as i32,
        })
    }
}

pub struct PreviewRenderer {
    surface: wgpu::Surface<'static>,
    device: Device,
    queue: Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
}

impl PreviewRenderer {
    pub fn new(window: &Window) -> Anyresult<Self> {
        let instance = wgpu::Instance::default();
        let surface = unsafe { instance.create_surface(window) }
            .map_err(|err| UmbrError::Generic(format!("failed to create surface: {err}")))?;
        let (_, adapter, device, queue) = request_device(Some(&surface))?;

        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .first()
            .copied()
            .unwrap_or(wgpu::TextureFormat::Bgra8Unorm);

        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: capabilities
                .alpha_modes
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Opaque),
            view_formats: vec![],
        };

        surface.configure(&device, &config);
        let pipeline = create_pipeline(&device, config.format);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
        })
    }

    pub fn resize(&mut self, new_size: PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    pub fn render(&mut self, layout: &Uheex) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let vertices = build_scene_vertices(layout, self.config.width, self.config.height);
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("preview_encoder"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("preview_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: true,
                    },
                })],
                depth_stencil_attachment: None,
            });

            if !vertices.is_empty() {
                let buffer = self
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("preview_vertices"),
                        contents: bytemuck::cast_slice(&vertices),
                        usage: wgpu::BufferUsages::VERTEX,
                    });

                pass.set_pipeline(&self.pipeline);
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..vertices.len() as u32, 0..1);
            }
        }

        self.queue.submit(Some(encoder.finish()));
        output.present();

        Ok(())
    }
}

pub fn run_preview(layout: &Uheex) -> Anyresult<()> {
    let event_loop = EventLoop::new().map_err(|err| UmbrError::Generic(err.to_string()))?;
    let window = WindowBuilder::new()
        .with_title("Umbr Locker Preview")
        .with_inner_size(winit::dpi::LogicalSize::new(1000.0, 560.0))
        .with_resizable(false)
        .build(&event_loop)
        .map_err(|err| UmbrError::Generic(err.to_string()))?;

    let mut renderer = PreviewRenderer::new(&window)?;
    let layout_clone = layout.clone();

    let mut control_flow = ControlFlow::Poll;

    event_loop.run_return(|event, _, flow| {
        *flow = control_flow;
        match event {
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => {
                    *flow = ControlFlow::Exit;
                }
                WindowEvent::Resized(new_size) => {
                    renderer.resize(new_size);
                }
                _ => {}
            },
            Event::RedrawRequested(_) => {
                if let Err(err) = renderer.render(&layout_clone) {
                    eprintln!("Preview render failed: {err}");
                    *flow = ControlFlow::Exit;
                }
            }
            Event::MainEventsCleared => {
                window.request_redraw();
            }
            _ => {}
        }
        control_flow = *flow;
    });

    Ok(())
}
