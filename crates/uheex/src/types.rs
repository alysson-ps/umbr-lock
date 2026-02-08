use core::fmt;
use logos::Logos;
use std::collections::BTreeMap;
use std::process::{Command, exit};
use std::str::FromStr;
use std::time::Duration;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[derive(Logos, Debug, PartialEq, Clone)]
pub enum Token<'a> {
    #[token("window")]
    Window,

    #[token("variable")]
    Variable,

    #[token("declare")]
    Declare,

    #[token("stylesheet")]
    Stylesheet,

    #[token("then")]
    Then,

    #[token("repeat")]
    Repeat,

    #[token("(")]
    LParenthesis,

    #[token(")")]
    RParenthesis,

    #[token(":")]
    Colon,

    #[regex(r";;[^\n]*")]
    Comment,

    #[token(".")]
    Dot,

    #[token("&")]
    Ampersand,

    #[token("$")]
    Dollar,

    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token(",")]
    Comma,

    // === Operators ===
    #[token("-")]
    Minus,

    #[token("+")]
    Plus,

    #[token("*")]
    Mul,

    #[token("/")]
    Div,

    #[token("==")]
    Eq,

    #[token("!=")]
    NotEq,

    #[token("&&")]
    And,

    #[token("||")]
    Or,

    #[token(">")]
    Gt,

    #[token("<")]
    Lt,

    // === Attribute related ===
    #[regex(r"[0-9]+")]
    Number(&'a str),

    #[regex(r#"'[^']*'"#)]
    LiteralString(&'a str),

    // === Misc ===
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*")]
    Identifier(&'a str),

    #[regex(r"[ \t\n\f]+", logos::skip)]
    Whitespace,

    Error,
}

impl fmt::Display for Token<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Window => write!(f, "window"),
            Self::Variable => write!(f, "variable"),
            Self::Declare => write!(f, "declare"),
            Self::Stylesheet => write!(f, "stylesheet"),
            Self::Then => write!(f, "then"),
            Self::Repeat => write!(f, "repeat"),
            Self::Ampersand => write!(f, "&"),
            Self::Dollar => write!(f, "$"),
            Self::LParenthesis => write!(f, "("),
            Self::RParenthesis => write!(f, ")"),
            Self::Gt => write!(f, ">"),
            Self::Lt => write!(f, "<"),
            Self::Plus => write!(f, "+"),
            Self::Mul => write!(f, "*"),
            Self::Div => write!(f, "/"),
            Self::Eq => write!(f, "=="),
            Self::NotEq => write!(f, "!="),
            Self::And => write!(f, "&&"),
            Self::Or => write!(f, "||"),
            Self::Comment => write!(f, "<comment>"),
            Self::Colon => write!(f, ":"),
            Self::Number(v) => write!(f, "{}", v),
            Self::Dot => write!(f, "."),
            Self::Minus => write!(f, "-"),
            Self::LiteralString(v) => write!(f, "{}", v),
            Self::Identifier(v) => write!(f, "{}", v),
            Self::LBracket => write!(f, "["),
            Self::RBracket => write!(f, "]"),
            Self::Comma => write!(f, ","),
            Self::Whitespace => write!(f, "<whitespace>"),
            Self::Error => write!(f, "<error>"),
        }
    }
}

#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[derive(Debug, Clone)]
pub struct Uheex {
    pub globals: Vec<VNode>,
    pub root: VNode,
    pub stylesheet: Option<Stylesheet>,
}

impl Uheex {
    const BASE_LAYOUT: &str = r#"
    import { LineEdit } from "std-widgets.slint";
    export component BaseLayout inherits Window {
    
    in-out property<string> passwd <=> password.text;

    callback submit <=> password.accepted;

    forward-focus: password;

    Rectangle {
        opacity: 0;
        clip: true;
        width: 10px;
        height: 10px;

        password := LineEdit {
            enabled: true;
            placeholder-text: "Enter password";
            input-type: InputType.password;
        }
    }

    "#;

    pub fn to_slint(&self) -> String {
        let mut slint_code = String::new();

        slint_code.push_str(Self::BASE_LAYOUT);

        self.globals.iter().for_each(|node| match node {
            VNode::Variable {
                name,
                initial,
                value,
                ..
            } => {
                match value {
                    Expr::Value(Value::String(s)) => {
                        slint_code
                            .push_str(&format!("in-out property<string> {}: \"{}\";\n", name, s));
                    }
                    Expr::Value(Value::Number(n)) => {
                        slint_code.push_str(&format!("in-out property<int> {}: {};\n", name, n));
                    }
                    _ => {}
                };
            }
            _ => {}
        });

        slint_code.push_str(self.vnode_to_slint(&self.root, 0).as_str());

        slint_code.push_str("}");

        slint_code
    }

    fn vnode_to_slint(&self, node: &VNode, indent: usize) -> String {
        let mut slint_code = String::new();
        let indent_str = "    ".repeat(indent);

        match node.clone() {
            VNode::Window { attributes, child } => {
                let background = match attributes.get("background") {
                    Some(Expr::Value(Value::String(s))) => s.clone(),
                    _ => "#151515".to_string(),
                };

                slint_code.push_str(&format!("{}Rectangle {{\n", indent_str));
                slint_code.push_str(&format!(
                    "{}background: {};\n",
                    "    ".repeat(indent + 1),
                    background
                ));

                let (anchor_x, anchor_y) = match attributes.get("anchor") {
                    Some(Expr::Array(arr)) => {
                        let x = match arr.get(0) {
                            Some(Expr::Value(Value::String(s))) => s.clone(),
                            _ => "center".to_string(),
                        };
                        let y = match arr.get(1) {
                            Some(Expr::Value(Value::String(s))) => s.clone(),
                            _ => "center".to_string(),
                        };
                        (x, y)
                    }
                    _ => ("center".to_string(), "center".to_string()),
                };

                slint_code.push_str(&format!(
                    "{}HorizontalLayout {{\n",
                    "    ".repeat(indent + 1)
                ));
                slint_code.push_str(&format!(
                    "{}alignment: {}; \n",
                    "        ".repeat(indent + 1),
                    anchor_x
                ));
                
                slint_code.push_str(&format!(
                    "{}VerticalLayout {{\n",
                    "        ".repeat(indent + 1)
                ));
                slint_code.push_str(&format!(
                    "{}alignment: {}; \n",
                    "            ".repeat(indent + 1),
                    anchor_y
                ));

                child.iter().for_each(|c| {
                    slint_code.push_str(&self.vnode_to_slint(c, indent + 1));
                });
                
                slint_code.push_str("}}\n");
                slint_code.push_str(&format!("{}}}\n", indent_str));
            }

            VNode::Widget {
                kind,
                attributes,
                child,
            } => {
                match kind {
                    WidgetKind::Label => {

                        slint_code.push_str(&format!("{}Text {{\n", indent_str));

                        let chd = self.vnode_to_slint(&child.first().unwrap(), indent + 1);

                        slint_code.push_str(&format!(
                            "{}text: {};\n",
                            "    ".repeat(indent + 1),
                            chd.trim()
                        ));

                        let attrs = self.mount_attrs_by_kind(&kind, &attributes);
                        slint_code.push_str(&format!("{}{}\n", "    ".repeat(indent + 1), attrs));

                        slint_code.push_str(&format!("{}}}\n", indent_str));
                    }
                    WidgetKind::Row => {
                        slint_code.push_str(&format!("{}HorizontalLayout {{\n", indent_str));

                        let attrs = self.mount_attrs_by_kind(&kind, &attributes);

                        slint_code.push_str(&format!("{}{}\n", "    ".repeat(indent + 1), attrs));

                        child.iter().for_each(|c| {
                            slint_code.push_str(&self.vnode_to_slint(c, indent + 1));
                        });

                        slint_code.push_str(&format!("{}}}\n", indent_str));
                    }
                    WidgetKind::Column => {
                        slint_code.push_str(&format!("{}VerticalLayout {{\n", indent_str));

                        let attrs = self.mount_attrs_by_kind(&kind, &attributes);

                        slint_code.push_str(&format!("{}{}\n", "    ".repeat(indent + 1), attrs));

                        child.iter().for_each(|c| {
                            slint_code.push_str(&self.vnode_to_slint(c, indent + 1));
                        });

                        slint_code.push_str(&format!("{}}}\n", indent_str));
                    }
                    WidgetKind::Rectangle => {
                        slint_code.push_str(&format!("{}Rectangle {{\n", indent_str));

                        let attrs = self.mount_attrs_by_kind(&kind, &attributes);

                        slint_code.push_str(&format!("{}{}\n", "    ".repeat(indent + 1), attrs));

                        child.iter().for_each(|c| {
                            slint_code.push_str(&self.vnode_to_slint(c, indent + 1));
                        });

                        slint_code.push_str(&format!("{}}}\n", indent_str));
                    }
                    _ => { /* Handle other widget kinds if needed */ }
                }
            }
            VNode::Then { cond, child } => {
                slint_code.push_str(&format!("{}if (", indent_str));
                // Note: This is a simplified representation of the condition
                if let Expr::Binary {
                    left,
                    operator,
                    right,
                } = cond.as_ref()
                {
                    let left_str = match left.as_ref() {
                        Expr::Value(Value::String(s)) => format!("\"{}\"", s),
                        Expr::Value(Value::Number(n)) => n.to_string(),
                        Expr::Binding(name) if name == "count" => {
                            "passwd.character-count".to_string()
                        }
                        _ => "<expr>".to_string(),
                    };

                    let right_str = match right.as_ref() {
                        Expr::Value(Value::String(s)) => format!("\"{}\"", s),
                        Expr::Value(Value::Number(n)) => n.to_string(),
                        Expr::Binding(name) if name == "count" => {
                            "passwd.character-count".to_string()
                        }
                        _ => "<expr>".to_string(),
                    };

                    let op_str = match operator {
                        BinaryOperator::Eq => "==",
                        BinaryOperator::NotEq => "!=",
                        BinaryOperator::Gt => ">",
                        BinaryOperator::Lt => "<",
                        BinaryOperator::Gte => ">=",
                        BinaryOperator::Lte => "<=",
                        _ => "<op>",
                    };

                    slint_code.push_str(&format!("{} {} {}", left_str, op_str, right_str));
                }
                slint_code.push_str("): ");
                slint_code.push_str(&self.vnode_to_slint(child.as_ref(), 0));
                slint_code.push_str(&format!("{}\n", indent_str));
            }

            VNode::Repeat { times, child } => {
                slint_code.push_str(&format!("{}for _ in ", indent_str));
                if let Expr::Value(Value::Number(n)) = times.as_ref() {
                    slint_code.push_str(&format!("{}", *n as usize));
                } else if let Expr::Binding(name) = times.as_ref() {
                    if name == "count" {
                        slint_code.push_str("passwd.character-count");
                    }
                }
                slint_code.push_str(": ");
                slint_code.push_str(&self.vnode_to_slint(child.as_ref(), 0));
                slint_code.push_str(&format!("{}\n", indent_str));
            }

            VNode::Binding(name) => {
                slint_code.push_str(&format!("{}{}\n", indent_str, name));
            }
            VNode::String(s) => {
                slint_code.push_str(&format!("\"{}\"\n", s));
            }
            _ => { /* Handle other VNode types if needed */ }
        }

        slint_code
    }

    fn mount_attrs_by_kind(
        &self,
        kind: &WidgetKind,
        attributes: &BTreeMap<String, Expr>,
    ) -> String {
        let mut attrs = String::new();

        match kind {
            WidgetKind::Label => {
                let color = match attributes.get("color") {
                    Some(Expr::Value(Value::String(s))) => s.clone(),
                    _ => "#FFFFFF".to_string(),
                };

                if let Some((x, y)) = match attributes.get("absolute") {
                    Some(Expr::Array(arr)) => {
                        let x = match arr.get(0) {
                            Some(Expr::Value(Value::String(s))) => s.clone(),
                            Some(Expr::Value(Value::Number(n))) => format!("{}px", *n as i32),
                            _ => "0px".to_string(),
                        };
                        let y = match arr.get(1) {
                            Some(Expr::Value(Value::String(s))) => s.clone(),
                            Some(Expr::Value(Value::Number(n))) => format!("{}px", *n as i32),
                            _ => "0px".to_string(),
                        };
                        Some((x, y))
                    }
                    _ => None,
                } {
                    attrs.push_str(&format!("x: {};\n", x));
                    attrs.push_str(&format!("y: {};\n", y));
                }

                attrs.push_str(&format!("color: {};\n", color));
            }
            WidgetKind::Row => {
                let align = match attributes.get("align") {
                    Some(Expr::Value(Value::String(s))) => s.clone(),
                    _ => "center".to_string(),
                };
                attrs.push_str(&format!("alignment: {};\n", align));

                let spacing = match attributes.get("spacing") {
                    Some(Expr::Value(Value::String(s))) => s.clone(),
                    _ => "0".to_string(),
                };
                attrs.push_str(&format!("spacing: {};\n", spacing));

                let padding = match attributes.get("padding") {
                    Some(Expr::Value(Value::String(s))) => s.clone(),
                    _ => "0".to_string(),
                };
                attrs.push_str(&format!("padding: {};\n", padding));
            }
            WidgetKind::Column => {
                let align = match attributes.get("align") {
                    Some(Expr::Value(Value::String(s))) => s.clone(),
                    _ => "center".to_string(),
                };
                attrs.push_str(&format!("alignment: {};\n", align));

                let spacing = match attributes.get("spacing") {
                    Some(Expr::Value(Value::String(s))) => s.clone(),
                    _ => "0".to_string(),
                };
                attrs.push_str(&format!("spacing: {};\n", spacing));

                let padding = match attributes.get("padding") {
                    Some(Expr::Value(Value::String(s))) => s.clone(),
                    _ => "0".to_string(),
                };
                attrs.push_str(&format!("padding: {};\n", padding));
            }
            WidgetKind::Rectangle => {
                let width = match attributes.get("width") {
                    Some(Expr::Value(Value::String(s))) => s.clone(),
                    _ => "100px".to_string(),
                };

                attrs.push_str(&format!("width: {};\n", width));

                let height = match attributes.get("height") {
                    Some(Expr::Value(Value::String(s))) => s.clone(),
                    _ => "100px".to_string(),
                };

                attrs.push_str(&format!("height: {};\n", height));

                let background = match attributes.get("background") {
                    Some(Expr::Value(Value::String(s))) => s.clone(),
                    _ => "transparent".to_string(),
                };

                attrs.push_str(&format!("background: {};\n", background));
            },
            WidgetKind::Absolute => {}
            _ => {}
        }

        attrs
    }

    #[cfg(feature = "serde")]
    pub fn as_raw(&self) -> String {
        serde_json::to_string_pretty(self).unwrap()
    }

    pub fn resolve_vars(&mut self) {
        self.globals.iter_mut().for_each(|node| {
            if let VNode::Variable { value, .. } = node {
                Self::resolve_expr_vars(value);
            }
        });
    }

    fn resolve_expr_vars(expr: &mut Expr) {
        match expr {
            Expr::Shell(cmd) => {
                let output = Command::new("sh")
                    .arg("-c")
                    .arg(&mut *cmd)
                    .output()
                    .expect("Failed to execute command");

                if output.status.success() {
                    let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    *expr = Expr::Value(Value::String(result));
                } else {
                    eprintln!(
                        "Command '{}' failed with error: {}",
                        cmd,
                        String::from_utf8_lossy(&output.stderr)
                    );
                    exit(1);
                }
            }
            _ => { /* Handle other expression types if needed */ }
        }
    }
}

#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[derive(Debug, Clone)]
pub struct Stylesheet {
    pub rules: Vec<Rule>,
}

#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[derive(Debug, Clone)]
pub struct Rule {
    pub selector: Selector,
    pub declarations: Vec<Declaration>,
}

#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[cfg_attr(feature = "serde", serde(untagged,))]
#[derive(Debug, Clone)]
pub enum Selector {
    Tag(String),
    Class(String),
    Id(String),
}

#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[cfg_attr(feature = "serde", serde(untagged))]
#[derive(Debug, Clone)]
pub enum Declaration {
    Simple {
        property: String,
        value: Expr,
    },
    Nested {
        property: String,
        value: Vec<Declaration>,
    },
}

#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[cfg_attr(feature = "serde", serde(tag = "type", content = "data"))]
#[derive(Debug, Clone)]
pub enum VNode {
    Window {
        attributes: BTreeMap<String, Expr>,
        child: Box<Vec<VNode>>,
    },
    Then {
        cond: Box<Expr>,
        child: Box<VNode>,
    },
    Repeat {
        times: Box<Expr>,
        child: Box<VNode>,
    },
    Widget {
        kind: WidgetKind,
        attributes: BTreeMap<String, Expr>,
        child: Box<Vec<VNode>>,
    },
    Variable {
        name: String,
        initial: Option<Expr>,
        value: Expr,
        interval: Duration,
    },
    Declare {
        vars: Vec<Self>,
    },
    String(String),
    Number(f64),
    Binding(String),

    Empty,
    Fragment(Box<Vec<VNode>>),
}

#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[derive(Debug, Clone)]
pub enum WidgetKind {
    Label,
    Rectangle,
    Row,
    Column,
    Absolute,
    Custom,
}

// #[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
// #[derive(Debug, Clone)]
// pub enum ErrorKind {
//     Unknown,
// }

#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[cfg_attr(feature = "serde", serde(untagged))]
#[derive(Debug, Clone)]
pub enum Expr {
    Value(Value),
    Array(Vec<Expr>),
    Binary {
        left: Box<Expr>,
        operator: BinaryOperator,
        right: Box<Expr>,
    },
    Binding(String),
    Shell(String),
}

#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[derive(Debug, Clone)]
pub enum BinaryOperator {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    NotEq,
    And,
    Or,
    Gt,
    Lt,
    Gte,
    Lte,
}

#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[cfg_attr(feature = "serde", serde(untagged))]
#[derive(Debug, Clone)]
pub enum Value {
    String(String),
    Number(f64),
    Binding(String),
}

pub struct DurationHuman(pub Duration);

impl FromStr for DurationHuman {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.ends_with("ms") {
            let num = &s[..s.len() - 2];
            match num.parse::<u64>() {
                Ok(v) => Ok(DurationHuman(Duration::from_millis(v))),
                Err(_) => Err(format!("Invalid duration: {}", s)),
            }
        } else if s.ends_with('s') {
            let num = &s[..s.len() - 1];
            match num.parse::<u64>() {
                Ok(v) => Ok(DurationHuman(Duration::from_secs(v))),
                Err(_) => Err(format!("Invalid duration: {}", s)),
            }
        } else if s.ends_with('m') {
            let num = &s[..s.len() - 1];
            match num.parse::<u64>() {
                Ok(v) => Ok(DurationHuman(Duration::from_secs(v * 60))),
                Err(_) => Err(format!("Invalid duration: {}", s)),
            }
        } else if s.ends_with('h') {
            let num = &s[..s.len() - 1];
            match num.parse::<u64>() {
                Ok(v) => Ok(DurationHuman(Duration::from_secs(v * 3600))),
                Err(_) => Err(format!("Invalid duration: {}", s)),
            }
        } else {
            Err(format!("Invalid duration: {}", s))
        }
    }
}
