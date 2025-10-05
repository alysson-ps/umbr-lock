use core::fmt;
use logos::Logos;
use std::collections::BTreeMap;
use std::process::Command;
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
    pub fn generate_css(&self) -> Option<String> {
        if let Some(stylesheet) = &self.stylesheet {
            let mut css = String::new();

            stylesheet.rules.iter().for_each(|rule| {
                css.push_str(&rule.to_css());
            });

            Some(css)
        } else {
            None
        }
    }

    #[cfg(feature = "serde")]
    pub fn as_raw(&self) -> String {
        serde_json::to_string_pretty(self).unwrap()
    }

    pub fn evaluate(&mut self) {
        // Convert all bindings to their evaluated forms
        self.resolve_binds();

        if let VNode::Window { child, .. } = &self.root {
            let mut new_chd: Box<Vec<VNode>> = child.clone();

            new_chd.iter_mut().for_each(|node| {
                self.evaluate_vnode(node);
            });

            self.root = VNode::Window {
                attributes: BTreeMap::new(),
                child: new_chd.clone(),
            };
        }
    }

    fn evaluate_vnode(&mut self, node: &mut VNode) {
        match node {
            VNode::Widget { child, .. } => {
                let mut new_chd: Box<Vec<VNode>> = child.clone();
                new_chd.iter_mut().for_each(|n| self.evaluate_vnode(n));
                *child = new_chd;
            }
            VNode::Then { cond, child } => {
                if let Expr::Binary {
                    left,
                    operator,
                    right,
                    ..
                } = cond.as_ref()
                {
                    let val_left = match left.as_ref() {
                        Expr::Value(Value::String(s)) => s.clone(),
                        Expr::Value(Value::Number(n)) => n.to_string(),
                        _ => unimplemented!("Unsupported left expression in Then condition"),
                    };

                    let val_right = match right.as_ref() {
                        Expr::Value(Value::String(s)) => s.clone(),
                        Expr::Value(Value::Number(n)) => n.to_string(),
                        _ => unimplemented!("Unsupported right expression in Then condition"),
                    };

                    let condition_met = match operator {
                        BinaryOperator::Eq => val_left == val_right,
                        BinaryOperator::NotEq => val_left != val_right,
                        BinaryOperator::Gt => val_left > val_right,
                        BinaryOperator::Lt => val_left < val_right,
                        BinaryOperator::Gte => val_left >= val_right,
                        BinaryOperator::Lte => val_left <= val_right,
                        _ => unimplemented!("Unsupported operator in Then condition"),
                    };

                    if condition_met {
                        self.evaluate_vnode(child);
                        *node = *child.clone();
                    } else {
                        *node = VNode::Empty;
                    }
                }
            }
            VNode::Repeat { times, child } => {
                if let Expr::Value(Value::Number(n)) = times.as_ref() {
                    let count = *n as usize;
                    let mut repeated_nodes = Vec::new();

                    if count == 0 {
                        *node = VNode::Empty;
                        return;
                    }

                    for _ in 0..count {
                        repeated_nodes.push(child.as_ref().clone());
                    }

                    *node = VNode::Fragment(Box::new(repeated_nodes));
                }
            }
            _ => {}
        }
    }

    fn resolve_binds(&mut self) {
        let mut binds: BTreeMap<String, VNode> = BTreeMap::new();

        self.globals.iter().for_each(|node| match node {
            VNode::Variable { name, value, .. } => {
                match value {
                    Expr::Value(Value::String(value)) => {
                        binds.insert(name.clone(), VNode::String(value.clone()));
                    }

                    Expr::Value(Value::Number(value)) => {
                        binds.insert(name.clone(), VNode::Number(*value));
                    }

                    Expr::Shell(cmd) => {
                        let command = Command::new("bash").args(&["-c", cmd]).output();

                        if let Ok(output) = command {
                            if output.status.success() {
                                if let Ok(result) = String::from_utf8(output.stdout) {
                                    binds.insert(
                                        name.clone(),
                                        VNode::String(result.trim().to_string()),
                                    );
                                }
                            }
                        }
                    }

                    _ => { /* Handle other expression types if needed */ }
                }
            }
            _ => {}
        });

        self.root = self.replace_binds_in_vnode(&self.root, &binds);
    }

    fn replace_binds_in_vnode(&self, node: &VNode, binds: &BTreeMap<String, VNode>) -> VNode {
        match node {
            VNode::Binding(name) => {
                if let Some(replacement) = binds.get(name) {
                    replacement.clone()
                } else {
                    node.clone()
                }
            }
            VNode::Window { attributes, child } => {
                let new_attributes = attributes
                    .iter()
                    .map(|(k, v)| (k.clone(), self.replace_binds_in_expr(v, binds)))
                    .collect();

                let new_child = Box::new(
                    child
                        .iter()
                        .map(|c| self.replace_binds_in_vnode(c, binds))
                        .collect(),
                );

                VNode::Window {
                    attributes: new_attributes,
                    child: new_child,
                }
            }
            VNode::Then { cond, child } => {
                let new_cond = Box::new(self.replace_binds_in_expr(cond, binds));
                let new_child = Box::new(self.replace_binds_in_vnode(child, binds));

                VNode::Then {
                    cond: new_cond,
                    child: new_child,
                }
            }
            VNode::Repeat { times, child } => {
                let new_times = Box::new(self.replace_binds_in_expr(times, binds));
                let new_child = Box::new(self.replace_binds_in_vnode(child, binds));

                VNode::Repeat {
                    times: new_times,
                    child: new_child,
                }
            }
            VNode::Widget {
                kind,
                attributes,
                child,
            } => {
                let new_attributes = attributes
                    .iter()
                    .map(|(k, v)| (k.clone(), self.replace_binds_in_expr(v, binds)))
                    .collect();

                let new_child = Box::new(
                    child
                        .iter()
                        .map(|c| self.replace_binds_in_vnode(c, binds))
                        .collect(),
                );

                VNode::Widget {
                    kind: kind.clone(),
                    attributes: new_attributes,
                    child: new_child,
                }
            }
            VNode::Variable {
                name,
                initial,
                value,
                interval,
            } => {
                let new_initial = initial
                    .as_ref()
                    .map(|init| self.replace_binds_in_expr(init, binds));
                let new_value = self.replace_binds_in_expr(value, binds);

                VNode::Variable {
                    name: name.clone(),
                    initial: new_initial,
                    value: new_value,
                    interval: *interval,
                }
            }

            _ => node.clone(), // For String, Number, Error, return as is
        }
    }

    fn replace_binds_in_expr(&self, expr: &Expr, binds: &BTreeMap<String, VNode>) -> Expr {
        match expr {
            Expr::Binding(name) => {
                match binds.get(name).unwrap() {
                    VNode::String(s) => Expr::Value(Value::String(s.clone())),
                    VNode::Number(n) => Expr::Value(Value::Number(*n)),
                    _ => expr.clone(), // If the replacement is not a simple value, keep the original
                }
            }
            Expr::Binary {
                kind,
                left,
                operator,
                right,
            } => {
                let new_left = Box::new(self.replace_binds_in_expr(left, binds));
                let new_right = Box::new(self.replace_binds_in_expr(right, binds));

                Expr::Binary {
                    kind: kind.clone(),
                    left: new_left,
                    operator: operator.clone(),
                    right: new_right,
                }
            }
            _ => expr.clone(), // For Value and Shell, return as is
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

impl Rule {
    pub fn to_css(&self) -> String {
        let mut css = String::new();

        let selector = match &self.selector {
            Selector::Tag(s) => s.clone(),
            Selector::Class(s) => format!(".{}", s),
            Selector::Id(s) => format!("#{}", s),
        };

        css.push_str(&format!("{} {{\n", selector));
        for declaration in &self.declarations {
            css.push_str(&format!("  {}\n", declaration.to_css(None)));
        }
        css.push_str("}\n");

        css
    }
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

impl Declaration {
    pub fn to_css(&self, prefix: Option<&str>) -> String {
        match self {
            Declaration::Simple { property, value } => {
                let value_str = match value {
                    Expr::Value(Value::String(s)) => s.clone(),
                    Expr::Value(Value::Number(n)) => n.to_string(),
                    _ => unimplemented!("Unsupported expression in declaration value"),
                };
                format!("{}{}: {};", prefix.unwrap_or(""), property, value_str)
            }
            Declaration::Nested { property, value } => {
                let mut css = String::new();

                value.into_iter().for_each(|d| {
                    if matches!(d, Self::Simple { .. }) {
                        css.push_str(&d.to_css(Some(&format!("{}-", property))));
                        css.push('\n');
                    }
                });

                css
            }
        }
    }
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
    Row,
    Column,
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
    Binary {
        kind: String,
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
