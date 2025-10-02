use core::fmt;
use std::str::FromStr;
use logos::Logos;
use std::collections::BTreeMap;
use std::time::Duration;

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

#[derive(Debug, Clone)]
pub struct Uheex {
    pub globals: Vec<VNode>,
    pub root: VNode,
    pub stylesheet: Option<Stylesheet>,
}

impl Uheex {
    pub fn new() -> Self {
        Self {
            globals: vec![],
            root: VNode::Error(ErrorKind::Unknown),
            stylesheet: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Stylesheet {
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub selector: Selector,
    pub declarations: Vec<Declaration>,
}

#[derive(Debug, Clone)]
pub enum Selector {
    Tag(String),
    Class(String),
    Id(String),
}

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
        kind: String,
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

    Error(ErrorKind),
}

#[derive(Debug, Clone)]
pub enum ErrorKind {
    Unknown,
}

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