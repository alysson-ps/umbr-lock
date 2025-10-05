use ariadne::{Color, Label, Report, ReportKind, Source};
use std::collections::BTreeMap;
use std::ops::Range;
use std::time::Duration;

use chumsky::error::Rich;
use chumsky::input::{Stream, ValueInput};
use chumsky::prelude::{choice, end, just, recursive};
use chumsky::span::SimpleSpan;
use chumsky::{IterParser, Parser, extra, select};

use crate::types::{
    BinaryOperator, Declaration, DurationHuman, Expr, Rule, Selector, Stylesheet, Uheex, VNode,
    WidgetKind,
};

use super::types::{Token, Value};

use logos::Logos;

fn parse<'tokens, 'src: 'tokens, I>()
-> impl Parser<'tokens, I, Uheex, extra::Err<Rich<'tokens, Token<'src>>>>
where
    I: ValueInput<'tokens, Token = Token<'src>, Span = SimpleSpan>,
{
    let comments = just(Token::Comment).repeated().ignored();

    // let safe_block = nested_delimiters(Token::LParenthesis, Token::RParenthesis, [], |_span| {
    //     VNode::Error(ErrorKind::Unknown)
    // });

    let declare = declare().padded_by(comments.clone()).or_not();
    let widgets = widgets().padded_by(comments.clone());
    let stylesheet = stylesheet().padded_by(comments.clone()).or_not();

    declare
        .then(widgets)
        .then(stylesheet)
        .padded_by(comments.clone())
        .then_ignore(end())
        .map(|((declare, root), stylesheet)| Uheex {
            globals: declare.unwrap_or(vec![]),
            root,
            stylesheet,
        })
}

fn widgets<'tokens, 'src: 'tokens, I>()
-> impl Parser<'tokens, I, VNode, extra::Err<Rich<'tokens, Token<'src>>>> + Clone
where
    I: ValueInput<'tokens, Token = Token<'src>, Span = SimpleSpan>,
{
    let comments = just(Token::Comment).repeated().ignored();

    recursive(|expr| {
        let ident = select! { Token::Identifier(v) => v.to_string() };

        let string =
            select! { Token::LiteralString(v) => VNode::String(v.trim_matches('\'').to_string()) };
        let number = select! { Token::Number(v) => VNode::Number(v.parse().unwrap_or(0.0)) };
        let binding = just(Token::Ampersand)
            .ignore_then(ident)
            .map(|v| VNode::Binding(v));

        // FIXME: the span is only hanging on the name of the 'then'
        // the correct thing would be in the complete block
        let then = just(Token::Then)
            .ignore_then(attributes())
            .then(expr.clone())
            .try_map(|(attrs, child), span| {
                if attrs.len() < 1 {
                    return Err(Rich::custom(span, "then must have a condition"));
                }

                if let Some(cond) = attrs.get("cond") {
                    Ok(VNode::Then {
                        cond: Box::new(cond.clone()),
                        child: Box::new(child),
                    })
                } else {
                    Err(Rich::custom(span, "then must have a condition"))
                }
            })
            .labelled("then directive");

        let repeat = just(Token::Repeat)
            .ignore_then(attributes())
            .then(expr.clone())
            .try_map(|(attrs, child), span| {
                if attrs.len() < 1 {
                    return Err(Rich::custom(span, "repeat must have a times attribute"));
                }
                if let Some(times) = attrs.get("times") {
                    Ok(VNode::Repeat {
                        times: Box::new(times.clone()),
                        child: Box::new(child),
                    })
                } else {
                    Err(Rich::custom(span, "repeat must have a times attribute"))
                }
            })
            .labelled("repeat directive");

        let widget = ident
            .then(attributes())
            .then(expr.clone().repeated().at_least(1).collect::<Vec<_>>())
            .map(|((kind, attrs), child)| {
                let kind = match kind.as_str() {
                    "label" => WidgetKind::Label,
                    "row" => WidgetKind::Row,
                    "column" => WidgetKind::Column,
                    _ => WidgetKind::Custom,
                };

                VNode::Widget {
                    kind,
                    attributes: attrs,
                    child: Box::new(child),
                }
            });

        let window = just(Token::Window)
            .ignore_then(attributes())
            .then(expr.clone().repeated().at_least(1).collect::<Vec<_>>())
            .map(|(attrs, child)| VNode::Window {
                attributes: attrs,
                child: Box::new(child),
            })
            .labelled("window directive");

        just(Token::LParenthesis)
            .ignore_then(
                window
                    .or(then)
                    .or(repeat)
                    .or(widget)
                    .or(string)
                    .or(number)
                    .or(binding)
                    .labelled("widget"),
            )
            .then_ignore(just(Token::RParenthesis))
            .padded_by(comments.clone())
    })
}

fn stylesheet<'tokens, 'src: 'tokens, I>()
-> impl Parser<'tokens, I, Stylesheet, extra::Err<Rich<'tokens, Token<'src>>>> + Clone
where
    I: ValueInput<'tokens, Token = Token<'src>, Span = SimpleSpan>,
{
    let comments = just(Token::Comment).repeated();

    let ident = select! { Token::Identifier(v) => v.to_string() }
        .or(just(Token::Window).to("window".to_string()));

    let declarations = recursive(|declarations| {
        let simple = just(Token::Colon)
            .ignore_then(ident.clone())
            .then(expression())
            .map(|(k, v)| Declaration::Simple {
                property: k,
                value: v,
            })
            .labelled("simple declaration");

        let nested = ident.clone()
            .then(declarations.clone().repeated().collect::<Vec<_>>())
            .map(|(k, v)| Declaration::Nested {
                property: k,
                value: v,
            })
            .delimited_by(just(Token::LParenthesis), just(Token::RParenthesis))
            .labelled("nested declaration");

        choice((simple, nested))
    })
    .padded_by(comments.clone())
    .repeated()
    .collect::<Vec<_>>();

    let widget = ident.clone()
        .then(declarations.clone())
        .map(|(selector, declarations)| Rule {
            selector: Selector::Tag(selector),
            declarations,
        })
        .delimited_by(just(Token::LParenthesis), just(Token::RParenthesis))
        .labelled("widget rule");

    let class = just(Token::Dot)
        .ignore_then(ident)
        .then(declarations.clone())
        .map(|(selector, declarations)| Rule {
            selector: Selector::Class(selector),
            declarations,
        })
        .delimited_by(just(Token::LParenthesis), just(Token::RParenthesis))
        .labelled("class rule");

    let selector = choice((widget, class))
        .repeated()
        .collect::<Vec<_>>()
        .padded_by(comments.clone());

    just(Token::Stylesheet)
        .ignore_then(selector)
        .map(|rules| Stylesheet { rules })
        .delimited_by(just(Token::LParenthesis), just(Token::RParenthesis))
}

fn declare<'tokens, 'src: 'tokens, I>()
-> impl Parser<'tokens, I, Vec<VNode>, extra::Err<Rich<'tokens, Token<'src>>>> + Clone
where
    I: ValueInput<'tokens, Token = Token<'src>, Span = SimpleSpan>,
{
    let values = select! {
        Token::LiteralString(v) => Expr::Value(Value::String(v.trim_matches('\'').to_string())),
        Token::Number(v) => Expr::Value(Value::Number(v.parse().unwrap_or(0.0))),
        Token::Identifier(v) => Expr::Value(Value::String(v.to_string())),
    }
    .delimited_by(just(Token::LParenthesis), just(Token::RParenthesis));

    // TODO: improve shell parsing, now it only supports `$(...)`
    let shell = just(Token::Dollar)
        .ignore_then(values.clone())
        .try_map(|v, span| match v {
            Expr::Value(Value::String(s)) => Ok(Expr::Shell(s)),
            _ => Err(Rich::custom(span, "shell must be a string")),
        });

    let variable = just(Token::Variable)
        .ignore_then(attributes())
        .then(shell.or(values))
        .try_map(|(attrs, value), span| {
            let name = match attrs.get("name") {
                Some(Expr::Value(Value::String(s))) => s.clone(),
                _ => return Err(Rich::custom(span, "variable must have a name attribute")),
            };

            let initial = match attrs.get("initial") {
                Some(v) => Some(v.clone()),
                None => None,
            };

            let interval = match attrs.get("interval") {
                Some(Expr::Value(Value::String(s))) => {
                    s.parse::<DurationHuman>().ok().map(|d| d.0).unwrap()
                }
                _ => Duration::from_secs(1),
            };

            Ok(VNode::Variable {
                name,
                initial,
                value,
                interval,
            })
        })
        .delimited_by(just(Token::LParenthesis), just(Token::RParenthesis));

    just(Token::Declare)
        .ignore_then(variable.repeated().collect::<Vec<_>>())
        .delimited_by(just(Token::LParenthesis), just(Token::RParenthesis))
}

fn attributes<'tokens, 'src: 'tokens, I>()
-> impl Parser<'tokens, I, BTreeMap<String, Expr>, extra::Err<Rich<'tokens, Token<'src>>>> + Clone
where
    I: ValueInput<'tokens, Token = Token<'src>, Span = SimpleSpan>,
{
    let comments = just(Token::Comment).repeated().ignored();

    let ident = select! { Token::Identifier(v) => v.to_string() };

    just(Token::Colon)
        .ignore_then(ident)
        .then(expression())
        .padded_by(comments.clone())
        .map(|(k, v)| (k, v))
        .repeated()
        .collect::<BTreeMap<_, _>>()
}

fn expression<'tokens, 'src: 'tokens, I>()
-> impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token<'src>>>> + Clone
where
    I: ValueInput<'tokens, Token = Token<'src>, Span = SimpleSpan>,
{
    let comments = just(Token::Comment).repeated().ignored();

    let ident = select! { Token::Identifier(v) => v.to_string() };

    let values = select! {
        Token::LiteralString(v) => Expr::Value(Value::String(v.trim_matches('\'').to_string())),
        Token::Number(v) => Expr::Value(Value::Number(v.parse().unwrap_or(0.0))),
        Token::Identifier(v) => Expr::Value(Value::String(v.to_string())),
    };

    let binding = just(Token::Ampersand)
        .ignore_then(ident)
        .map(|v| Expr::Binding(v));

    let op = select! {
        Token::Gt => BinaryOperator::Gt,
        Token::Lt => BinaryOperator::Lt,
        Token::Plus => BinaryOperator::Add,
        Token::Minus => BinaryOperator::Sub,
        Token::Mul => BinaryOperator::Mul,
        Token::Div => BinaryOperator::Div,
        Token::Eq => BinaryOperator::Eq,
        Token::NotEq => BinaryOperator::NotEq,
        Token::And => BinaryOperator::And,
        Token::Or => BinaryOperator::Or,
    };

    let atom = values.or(binding.clone());

    let expr = atom
        .clone()
        .foldl_with(op.then(atom).repeated(), |left, (operator, right), _e| {
            Expr::Binary {
                kind: "".to_string(),
                left: Box::new(left),
                operator,
                right: Box::new(right),
            }
        })
        .delimited_by(just(Token::LParenthesis), just(Token::RParenthesis));

    expr.or(values).or(binding).padded_by(comments)
}

pub fn parser(code: &str) -> Option<Uheex> {
    let tokens_iter = Token::lexer(code).spanned().map(|(t, s)| match t {
        Ok(t) => (t, <Range<usize> as Into<SimpleSpan>>::into(s)),
        Err(()) => (Token::Error, s.into()),
    });

    use chumsky::input::Input;
    let token_stream = Stream::from_iter(tokens_iter).map((0..code.len()).into(), |(t, s)| (t, s));

    let (result, errs) = parse().parse(token_stream).into_output_errors();

    for err in errs {
        Report::build(ReportKind::Error, ("config.uheex", err.span().into_range()))
            .with_config(ariadne::Config::new().with_index_type(ariadne::IndexType::Byte))
            .with_code(1)
            .with_message(err.to_string())
            .with_label(
                Label::new(("config.uheex", err.span().into_range()))
                    .with_message(err.reason().to_string())
                    .with_color(Color::Red),
            )
            .finish()
            .print(("config.uheex", Source::from(code)))
            .unwrap();
    }

    if let Some(mut result) = result {
        result.evaluate();

        Some(result)
    } else {
        None
    }
}
