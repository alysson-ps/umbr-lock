use core::fmt;
use std::collections::BTreeMap;
use logos::Logos;

#[derive(Logos, Debug, PartialEq, Clone)]
pub enum Token<'a> {
    // === Tag delimiters ===
    #[token("<%")]
    BlockBindingOpen,

    #[token("%>")]
    BlockBindingClose,

    #[regex(r"@[\w\-]+", priority = 2)]
    BlockBindingValue(&'a str),

    #[token("<")]
    TagStart,

    #[token("/")]
    Slash,

    #[token(">")]
    TagEnd,

    #[token(".")]
    Dot,

    #[token(":")]
    Colon,

    #[token("-")]
    Hifen,

    // === Attribute related ===
    #[regex(r"[0-9]+")]
    Number(&'a str),

    #[regex(r#""[^"]*""#)]
    LiteralString(&'a str),

    #[regex(r"\{@[A-Za-z0-9_]+\}")]
    Binding(&'a str),

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
            Self::BlockBindingOpen => write!(f, "<%"),
            Self::BlockBindingClose => write!(f, "%>"),
            Self::BlockBindingValue(v) => write!(f, "{}", v),
            Self::Slash => write!(f, "/"),
            Self::Colon => write!(f, ";"),
            Self::TagEnd => write!(f, ">"),
            Self::TagStart => write!(f, "<"),
            Self::Number(v) => write!(f, "{}", v),
            Self::Dot => write!(f, "."),
            Self::Hifen => write!(f, "-"),
            Self::LiteralString(v) => write!(f, "{}", v),
            Self::Binding(v) => write!(f, "{}", v),
            Self::Identifier(v) => write!(f, "{}", v),
            Self::Whitespace => write!(f, "<whitespace>"),
            Self::Error => write!(f, "<error>"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum SExpr {
    Element {
        name: String,
        attributes: BTreeMap<String, Value>,
        children: Vec<SExpr>,
        self_closing: bool,
    },
    Text(String),
    Binding(String), // <% @user %>
}

#[derive(Debug, Clone)]
pub enum Value {
    String(String),
    Number(f64), 
    Binding(String),
}
