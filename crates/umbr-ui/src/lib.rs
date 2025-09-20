#![allow(warnings)]
use cairo::RectangleInt;
use gdk4::{Snapshot, Surface};
use gsk4::Renderer;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow};
use gtk4::gdk::Display;
use gtk4::gio::ApplicationFlags;
use gtk4::{self as gtk, CssProvider, Widget};
use std::collections::BTreeMap;

// use gtk4_layer_shell::{Edge, Layer, LayerShell};

use uheex::types::{SExpr, Value};
#[derive(Debug)]
struct Element {
    name: String,
    attributes: BTreeMap<String, Value>,
    children: Vec<SExpr>,
}

pub mod types;
pub mod win;

fn builder(layout: SExpr) -> Widget {
    match layout {
        SExpr::Element {
            name,
            attributes,
            children,
            ..
        } => parser_element(Element {
            name,
            attributes,
            children,
        }),
        SExpr::Text(v) => {
            dbg!(&v);
            gtk::Label::new(Some(format!("text {}", v).as_str())).upcast()
        }
        SExpr::Binding(v) => {
            dbg!(&v);
            gtk::Label::new(Some(format!("bind {}", v).as_str())).upcast()
        }
    }
}

fn parser_element(el: Element) -> Widget {
    dbg!(&el);
    match el.name.as_str() {
        "Button" => {
            let text = el.attributes.iter().collect::<Vec<_>>();

            gtk::Button::new().upcast()
        }
        "Box" => {
            let orientation = match el.attributes.get("direction").map(|v| match v {
                Value::String(v) => v.as_str(),
                _ => "",
            }) {
                Some("row") => gtk::Orientation::Horizontal,
                _ => gtk::Orientation::Vertical,
            };

            let spacing = el.attributes.get("spacing").map(|v| match v {
                Value::Number(v) => v,
                _ => &8.0,
            });

            let container = gtk::Box::new(orientation, spacing.unwrap().round() as i32);

            for chd in el.children {
                container.append(builder(chd).downcast_ref::<Widget>().unwrap());
            }

            container.upcast()
        }
        "Label" => gtk::Label::new(Some(&el.name)).upcast(),
        v => unimplemented!("{}", v.to_string()),
    }
}

pub fn mount_ui(layout: SExpr) -> Option<Vec<u8>> {
    if !gtk::is_initialized() {
        gtk::init().expect("Failed to initialize GTK.");
    }

    let display = Display::default().expect("Could not connect to a display.");

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.set_vexpand(true);
    root.set_hexpand(true);

    let center = gtk::Box::new(gtk::Orientation::Vertical, 0);
    center.set_halign(gtk::Align::Center);
    center.set_valign(gtk::Align::Center);

    let label = gtk::Label::new(Some("Hello, world!"));
    center.append(&label);
    root.append(&center);

    root.realize();
    root.measure(gtk4::Orientation::Horizontal, -1);
    root.measure(gtk4::Orientation::Vertical, -1);
    root.size_allocate(&gtk4::gdk::Rectangle::new(0, 0, 800, 600), -1);

    let snapshot = gtk::Snapshot::new();
    root.snapshot_child(&label, &snapshot);
    let node = snapshot.to_node()?;

    let rc = RectangleInt::new(0, 0, 800, 600);

    let surface = Surface::new_toplevel(&display);
    let renderer = Renderer::for_surface(&surface).unwrap();
    // let rect = gtk4::cairo::Region::create_rectangle(&rc);
    let viewport = graphene::Rect::new(0.0, 0.0, 800 as f32, 600 as f32);
    let texture = renderer.render_texture(&node, Some(&viewport));

    let stride = (800 * 4) as usize;
    let mut rgba = vec![0u8; stride * 600 as usize];
    texture.download(rgba.as_mut_slice(), stride);
    Some(rgba)
}
