use std::cell::RefCell;
use std::f64::consts::TAU;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{
    CanvasRenderingContext2d, CanvasWindingRule, Document, Event, HtmlCanvasElement, HtmlElement,
    HtmlInputElement, HtmlSelectElement, MouseEvent, Path2d,
};

const W: f64 = 640.0;
const H: f64 = 480.0;
const R: f64 = 110.0;
const PAD: f64 = 10.0; // universe border inset
const ON_COLOR: &str = "#b39ddb"; // lavender-mauve when a region is toggled on

struct State {
    n: usize,          // number of sets (2 or 3)
    toggled: [bool; 8], // region bitmask -> coloured or not
    elements: bool,     // show one labelled dot per region
    dots: [Option<(f64, f64)>; 8], // dot position per region
    canvas: HtmlCanvasElement,
    ctx: CanvasRenderingContext2d,
    doc: Document,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// Circle centres for a 2- or 3-set layout.
fn centers(n: usize) -> Vec<(f64, f64)> {
    match n {
        2 => vec![(W / 2.0 - 65.0, H / 2.0), (W / 2.0 + 65.0, H / 2.0)],
        _ => {
            // Classic 3-way Venn: each circle passes through the other centres.
            let d = R / 3.0_f64.sqrt();
            vec![
                (W / 2.0, H / 2.0 - d),
                (W / 2.0 - 0.866_025_4 * d, H / 2.0 + 0.5 * d),
                (W / 2.0 + 0.866_025_4 * d, H / 2.0 + 0.5 * d),
            ]
        }
    }
}

/// Region bitmask for a point: bit i set when inside circle i.
fn membership(x: f64, y: f64, cs: &[(f64, f64)]) -> usize {
    let mut m = 0;
    for (i, &(cx, cy)) in cs.iter().enumerate() {
        let dx = x - cx;
        let dy = y - cy;
        if dx * dx + dy * dy <= R * R {
            m |= 1 << i;
        }
    }
    m
}

/// Representative point per region: centroid of a fine grid sample.
/// Region 0 (outside all circles) uses the sample farthest from the circles,
/// since its centroid lands inside the circles by symmetry.
fn compute_dots(n: usize, cs: &[(f64, f64)]) -> [Option<(f64, f64)>; 8] {
    let mut acc = [(0.0f64, 0.0f64, 0u32); 8];
    let mut farthest: Option<(f64, f64, f64)> = None; // region 0: x, y, min dist^2 to a circle
    let step = 2.0;
    let mut x = PAD;
    while x <= W - PAD {
        let mut y = PAD;
        while y <= H - PAD {
            let m = membership(x, y, cs);
            let e = &mut acc[m];
            e.0 += x;
            e.1 += y;
            e.2 += 1;
            if m == 0 {
                let d2 = cs
                    .iter()
                    .map(|&(cx, cy)| (x - cx) * (x - cx) + (y - cy) * (y - cy))
                    .fold(f64::INFINITY, f64::min);
                if farthest.is_none_or(|f| d2 > f.2) {
                    farthest = Some((x, y, d2));
                }
            }
            y += step;
        }
        x += step;
    }
    let mut out = [None; 8];
    for m in 0..(1 << n) {
        if acc[m].2 > 0 {
            let x = (acc[m].0 / acc[m].2 as f64).clamp(PAD + 14.0, W - PAD - 14.0);
            let y = (acc[m].1 / acc[m].2 as f64).clamp(PAD + 14.0, H - PAD - 14.0);
            out[m] = Some((x, y));
        }
    }
    if let Some((x, y, _)) = farthest {
        out[0] = Some((x.clamp(PAD + 14.0, W - PAD - 14.0), y.clamp(PAD + 14.0, H - PAD - 14.0)));
    }
    out
}

/// Fill exactly the area matching region bitmask m using nested clips.
fn fill_region(ctx: &CanvasRenderingContext2d, n: usize, cs: &[(f64, f64)], m: usize, color: &str) {
    ctx.save();
    for i in 0..n {
        let (cx, cy) = cs[i];
        let p = Path2d::new().unwrap();
        if m & (1 << i) != 0 {
            // clip to inside of circle i
            p.arc(cx, cy, R, 0.0, TAU).unwrap();
        } else {
            // rect + circle with evenodd => clip to outside of circle i
            p.rect(0.0, 0.0, W, H);
            p.arc(cx, cy, R, 0.0, TAU).unwrap();
        }
        ctx.clip_with_path_2d_and_winding(&p, CanvasWindingRule::Evenodd);
    }
    ctx.set_fill_style_str(color);
    ctx.fill_rect(0.0, 0.0, W, H);
    ctx.restore();
}

fn draw(s: &State) {
    let ctx = &s.ctx;
    ctx.clear_rect(0.0, 0.0, W, H);
    let cs = centers(s.n);

    // Every region is filled exactly once: on-colour or white.
    for m in 0..(1usize << s.n) {
        let color = if s.toggled[m] { ON_COLOR } else { "#ffffff" };
        fill_region(ctx, s.n, &cs, m, color);
    }

    // Universe border and circle outlines.
    ctx.set_line_width(2.0);
    ctx.set_stroke_style_str("#333333");
    ctx.stroke_rect(PAD, PAD, W - 2.0 * PAD, H - 2.0 * PAD);
    for &(cx, cy) in &cs {
        ctx.begin_path();
        ctx.arc(cx, cy, R, 0.0, TAU).unwrap();
        ctx.stroke();
    }

    // Set labels above each circle.
    ctx.set_font("20px sans-serif");
    ctx.set_fill_style_str("#111111");
    ctx.set_text_align("center");
    let labels = ["A", "B", "C"];
    for (i, &(cx, cy)) in cs.iter().enumerate() {
        ctx.fill_text(labels[i], cx, cy - R - 8.0).unwrap();
    }

    // Optional element dots: one per region, labelled with the bitmask value.
    if s.elements {
        ctx.set_font("13px sans-serif");
        ctx.set_text_baseline("middle");
        for m in 0..(1usize << s.n) {
            if let Some((x, y)) = s.dots[m] {
                ctx.begin_path();
                ctx.arc(x, y, 12.0, 0.0, TAU).unwrap();
                ctx.set_fill_style_str("#ffffff");
                ctx.fill();
                ctx.set_line_width(1.5);
                ctx.set_stroke_style_str("#000000");
                ctx.stroke();
                ctx.set_fill_style_str("#000000");
                ctx.fill_text(&m.to_string(), x, y + 0.5).unwrap();
            }
        }
        ctx.set_text_baseline("alphabetic");
    }
}

#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    let doc = web_sys::window().unwrap().document().unwrap();
    let canvas: HtmlCanvasElement = doc.get_element_by_id("venn").unwrap().dyn_into()?;
    canvas.set_width(W as u32);
    canvas.set_height(H as u32);
    let ctx: CanvasRenderingContext2d = canvas.get_context("2d")?.unwrap().dyn_into()?;

    let n = 2usize;
    STATE.with(|st| {
        *st.borrow_mut() = Some(State {
            n,
            toggled: [false; 8],
            elements: false,
            dots: compute_dots(n, &centers(n)),
            canvas: canvas.clone(),
            ctx,
            doc: doc.clone(),
        });
    });

    // Click a region to toggle its colour.
    let canvas2 = canvas.clone();
    let on_click = Closure::<dyn FnMut(MouseEvent)>::new(move |e: MouseEvent| {
        let rect = canvas2.get_bounding_client_rect();
        let x = (e.client_x() as f64 - rect.left()) * (W / rect.width());
        let y = (e.client_y() as f64 - rect.top()) * (H / rect.height());
        STATE.with(|st| {
            let mut st = st.borrow_mut();
            let s = st.as_mut().unwrap();
            let m = membership(x, y, &centers(s.n));
            s.toggled[m] = !s.toggled[m];
            draw(s);
        });
    });
    canvas.add_event_listener_with_callback("click", on_click.as_ref().unchecked_ref())?;
    on_click.forget();

    // Dropdown: 2 or 3 sets.
    let sel: HtmlSelectElement = doc.get_element_by_id("nsets").unwrap().dyn_into()?;
    let sel2 = sel.clone();
    let on_sets = Closure::<dyn FnMut(Event)>::new(move |_e: Event| {
        STATE.with(|st| {
            let mut st = st.borrow_mut();
            let s = st.as_mut().unwrap();
            s.n = sel2.value().parse::<usize>().unwrap_or(2).clamp(2, 3);
            s.toggled = [false; 8];
            s.dots = compute_dots(s.n, &centers(s.n));
            draw(s);
        });
    });
    sel.add_event_listener_with_callback("change", on_sets.as_ref().unchecked_ref())?;
    on_sets.forget();

    // Checkbox: show element dots.
    let chk: HtmlInputElement = doc.get_element_by_id("elements").unwrap().dyn_into()?;
    let chk2 = chk.clone();
    let on_elems = Closure::<dyn FnMut(Event)>::new(move |_e: Event| {
        STATE.with(|st| {
            let mut st = st.borrow_mut();
            let s = st.as_mut().unwrap();
            s.elements = chk2.checked();
            draw(s);
        });
    });
    chk.add_event_listener_with_callback("change", on_elems.as_ref().unchecked_ref())?;
    on_elems.forget();

    // Button: download the canvas as a PNG.
    let on_save = Closure::<dyn FnMut(Event)>::new(move |_e: Event| {
        STATE.with(|st| {
            let st = st.borrow();
            let s = st.as_ref().unwrap();
            let data_url = s.canvas.to_data_url().unwrap();
            let a: HtmlElement = s.doc.create_element("a").unwrap().dyn_into().unwrap();
            a.set_attribute("href", &data_url).unwrap();
            a.set_attribute("download", "venn.png").unwrap();
            a.click();
        });
    });
    let btn = doc.get_element_by_id("save").unwrap();
    btn.add_event_listener_with_callback("click", on_save.as_ref().unchecked_ref())?;
    on_save.forget();

    STATE.with(|st| draw(st.borrow().as_ref().unwrap()));
    Ok(())
}
