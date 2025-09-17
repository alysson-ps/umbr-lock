use core::fmt;
use std::ops::Range;

use chumsky::error::Rich;
use chumsky::input::{Input, Stream, ValueInput};
use chumsky::prelude::{just, recursive};
use chumsky::span::SimpleSpan;
use chumsky::{IterParser, Parser, extra, select};
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
    #[token("=")]
    Equals,

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
            Self::Equals => write!(f, "="),
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
        attributes: Vec<Attr>,
        children: Vec<SExpr>,
        self_closing: bool,
    },
    Text(String),
    Binding(String), // <% @user %>
}

#[derive(Debug, Clone)]
pub enum Value {
    Literal(String), // text="Desbloquear"
    Binding(String), // text={@status}
}

#[derive(Debug, Clone)]
pub enum Attr {
    Property { name: String, value: Value },
    Event { kind: String, handler: String }, // u-click={@login_user}
}

fn parse<'tokens, 'src: 'tokens, I>()
-> impl Parser<'tokens, I, SExpr, extra::Err<Rich<'tokens, Token<'src>>>>
where
    I: ValueInput<'tokens, Token = Token<'src>, Span = SimpleSpan>,
{
    recursive(|expr| {
        let ident = select! { Token::Identifier(v) => v.to_string() };

        let tag_name = ident
            .ignore_then(just(Token::Dot))
            .ignore_then(ident)
            .labelled("Tag name");

        let end_close = just(Token::Slash)
            .then(just(Token::TagEnd))
            .labelled("Self close");
        let start_close = just(Token::TagStart).then(just(Token::Slash));

        let attr_name = just(Token::Colon).ignore_then(ident).labelled("Attr name");

        let attr_value = select! {
            Token::LiteralString(v) => Value::Literal(v.replace("\"", "").to_string()),
            Token::Binding(v) => Value::Binding(v.to_string()),
        }
        .labelled("Attr value");

        let attr = attr_name
            .then_ignore(just(Token::Equals))
            .then(attr_value)
            .map(|(name, value)| Attr::Property {
                name: name,
                value: value,
            });

        let tag_self_closed = just(Token::TagStart)
            .ignore_then(tag_name.clone())
            .then(attr.clone().repeated().collect::<Vec<_>>().or_not())
            .then_ignore(end_close)
            .map(|(name, attrs)| SExpr::Element {
                name: name,
                attributes: attrs.unwrap_or_default(),
                children: vec![],
                self_closing: true,
            })
            .labelled("Element self closed");

        let tag_nornal = just(Token::TagStart)
            .ignore_then(tag_name.clone())
            .then(attr.clone().repeated().collect::<Vec<_>>().or_not())
            .then_ignore(just(Token::TagEnd))
            .then(expr.repeated().collect::<Vec<_>>())
            .then_ignore(start_close.then(tag_name.clone()).then(just(Token::TagEnd)))
            .map(|((name, attrs), chd)| SExpr::Element {
                name: name,
                attributes: attrs.unwrap_or_default(),
                children: chd,
                self_closing: false,
            })
            .labelled("Element With Children");

        let block = select! {Token::BlockBindingValue(v) => v.replace("@", "").to_string()}
            .map(|bind| SExpr::Binding(bind))
            .delimited_by(
                just(Token::BlockBindingOpen),
                just(Token::BlockBindingClose),
            );

        tag_self_closed.or(tag_nornal).or(block)
    })
}

pub fn parser(code: &str) {
    let tokens_iter = Token::lexer(code).spanned().map(|(t, s)| match t {
        Ok(t) => (t, <Range<usize> as Into<SimpleSpan>>::into(s)),
        Err(()) => (Token::Error, s.into()),
    });

    // dbg!(tokens_iter.collect::<Vec<_>>());
    let token_stream = Stream::from_iter(tokens_iter).map((0..code.len()).into(), |(t, s)| (t, s));

    let reseult = parse().parse(token_stream).into_result();

    dbg!(&reseult);
}
